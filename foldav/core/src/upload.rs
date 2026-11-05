//! Files and folders of this device going to the server: a file in one request, a big one in
//! chunks (Nextcloud's chunked upload, so a dropped connection costs one chunk, not the file),
//! and a folder as the folders and files in it. Progress and cancel are in a `Progress`, as for
//! downloads.

use crate::dav::Dav;
use crate::json::Json;
use crate::ops::{copy_name, join};
use crate::{Client, Error, Progress, Result};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

/// A file this big or bigger goes in chunks, each of `CHUNK` bytes (the server takes up to
/// 100 MB a chunk, `capabilities.files.chunked_upload`).
const CHUNKED_FROM: u64 = 32 << 20;
const CHUNK: u64 = 16 << 20;
/// A chunk is sent this many times before the upload fails.
const CHUNK_TRIES: u32 = 3;
/// How many chunks of a file are sent at once.
const PARALLEL: usize = 3;

/// What to do with a file or folder when the name is taken in the folder it goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taken {
    /// Under a name that is free ("a (copy).txt"): nothing is overwritten.
    KeepBoth,
    /// A file replaces the one there (the server keeps the old one as a version); a folder is
    /// merged with the one there.
    Replace,
    /// Left out.
    Skip,
}

fn io(e: std::io::Error) -> Error {
    Error::Io(e.to_string())
}

/// What is read of a file on its way: counted (also in `sent`, what this one request took),
/// and stopped when cancelled.
struct Counting<R> {
    inner: R,
    p: Progress,
    sent: Arc<AtomicU64>,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.p.is_cancelled() {
            return Err(std::io::Error::other("cancelled"));
        }
        let n = self.inner.read(buf)?;
        self.p.add(n as u64);
        self.sent.fetch_add(n as u64, Relaxed);
        Ok(n)
    }
}

enum Step {
    Dir(String),
    File {
        local: PathBuf,
        to: String,
        size: u64,
    },
}

/// The steps that put `local` at `to`, and how many bytes the files are.
fn plan(local: &Path, to: &str, steps: &mut Vec<Step>, total: &mut u64) -> Result<()> {
    let meta = std::fs::metadata(local).map_err(io)?;
    if meta.is_file() {
        *total += meta.len();
        steps.push(Step::File {
            local: local.into(),
            to: to.into(),
            size: meta.len(),
        });
        return Ok(());
    }
    steps.push(Step::Dir(to.into()));
    let mut entries: Vec<_> = std::fs::read_dir(local)
        .map_err(io)?
        .collect::<std::io::Result<_>>()
        .map_err(io)?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        // A link to a folder is not followed: it may lead back to where it is.
        let link = e.file_type().map_err(io)?.is_symlink();
        if link && path.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        plan(&path, &join(to, &name), steps, total)?;
    }
    Ok(())
}

fn mtime(local: &Path) -> i64 {
    std::fs::metadata(local)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

fn answer(status: u16, href: &str, body: String) -> Result<()> {
    match status {
        200..=299 => Ok(()),
        404 | 409 => Err(Error::NotFound(href.into())),
        412 => Err(Error::Conflict),
        423 => Err(Error::Locked(href.into())),
        s => Err(Error::Http(s, body)),
    }
}

/// The reader of `len` bytes of `local` from `offset`.
fn open(
    local: &Path,
    offset: u64,
    len: u64,
    p: &Progress,
    sent: &Arc<AtomicU64>,
) -> Result<Box<dyn Read + Send>> {
    let mut f = std::fs::File::open(local).map_err(io)?;
    f.seek(SeekFrom::Start(offset)).map_err(io)?;
    Ok(Box::new(Counting {
        inner: f.take(len),
        p: p.clone(),
        sent: sent.clone(),
    }))
}

/// Run `f`; a cancel shows as `Cancelled` whatever the connection made of it.
fn unless_cancelled<T>(p: &Progress, r: Result<T>) -> Result<T> {
    match r {
        Err(_) if p.is_cancelled() => Err(Error::Cancelled),
        r => r,
    }
}

impl Client {
    /// Which of `names` are taken in folder `dest` of account `id`.
    pub fn taken(&self, id: i64, dest: &str, names: &[String]) -> Result<Vec<String>> {
        let there = self.names_in(id, dest)?;
        Ok(names
            .iter()
            .filter(|n| there.contains(*n))
            .cloned()
            .collect())
    }

    /// Put the files and folders `items` of this device into folder `dest` of account `id`.
    /// `taken` says what to do with those whose name is there already. Stops at the first
    /// failure; what was uploaded stays.
    pub fn upload(
        &self,
        id: i64,
        dest: &str,
        items: &[PathBuf],
        taken: Taken,
        p: &Progress,
    ) -> Result<()> {
        let mut names = self.names_in(id, dest)?;
        let (mut steps, mut total) = (Vec::new(), 0);
        for item in items {
            let name = item
                .file_name()
                .ok_or_else(|| Error::Invalid(item.display().to_string()))?;
            let name = name.to_string_lossy().into_owned();
            if !crate::valid_name(&name) {
                return Err(Error::Invalid(name));
            }
            let folder = item.is_dir();
            let to = match (names.contains(&name), taken) {
                (false, _) | (true, Taken::Replace) => name,
                (true, Taken::Skip) => continue,
                (true, Taken::KeepBoth) => (1..)
                    .map(|n| copy_name(&name, n, folder))
                    .find(|n| !names.contains(n))
                    .unwrap_or_default(),
            };
            plan(item, &join(dest, &to), &mut steps, &mut total)?;
            names.insert(to);
        }
        p.set_total(total);
        let dav = self.dav(id)?;
        let home = self.files_home(id)?;
        // Small files wait to go together, in one request.
        let mut batch = Batch::default();
        let mut bulk = true;
        for step in steps {
            if p.is_cancelled() {
                return Err(Error::Cancelled);
            }
            match step {
                Step::Dir(to) => {
                    match dav.mkcol(&format!("{home}{to}")) {
                        // It is there already: a folder merged.
                        Ok(()) | Err(Error::Conflict) => {}
                        Err(e) => return Err(e),
                    }
                }
                Step::File { local, to, size } => {
                    if bulk && size <= BULK_FILE {
                        batch.add(local, to, size);
                        if batch.full() {
                            bulk = batch.send(&dav, &home, p)?;
                        }
                    } else {
                        let href = format!("{home}{to}");
                        unless_cancelled(p, put(&dav, &home, &href, &local, size, None, p))?;
                    }
                }
            }
        }
        batch.send(&dav, &home, p)?;
        Ok(())
    }
}

/// Files up to this size go many in a request (Nextcloud's bulk upload).
const BULK_FILE: u64 = 256 << 10;
/// A request of them holds up to this many bytes and files.
const BULK_BYTES: u64 = 4 << 20;
const BULK_FILES: usize = 100;

/// Small files waiting to be sent together.
#[derive(Default)]
struct Batch {
    files: Vec<(PathBuf, String, u64)>,
    bytes: u64,
}

impl Batch {
    fn add(&mut self, local: PathBuf, to: String, size: u64) {
        self.bytes += size;
        self.files.push((local, to, size));
    }

    fn full(&self) -> bool {
        self.bytes >= BULK_BYTES || self.files.len() >= BULK_FILES
    }

    /// Send what waits. False when the server has no bulk upload (the files were sent one by
    /// one, and the rest should be).
    fn send(&mut self, dav: &Dav, home: &str, p: &Progress) -> Result<bool> {
        let files = std::mem::take(&mut self.files);
        self.bytes = 0;
        // A single file gains nothing from a multipart request.
        if files.len() > 1 && bulk(dav, home, &files, p).is_ok() {
            return Ok(true);
        }
        let several = files.len() > 1;
        for (local, to, size) in files {
            let href = format!("{home}{to}");
            unless_cancelled(p, put(dav, home, &href, &local, size, None, p))?;
        }
        // One file is no sign of what the server can do: only a failed request is.
        Ok(!several)
    }
}

/// Upload `files` (their path on this device, below the Files root, size) in one request:
/// `multipart/related`, each part a file with its path, date and MD5. Nothing is counted as
/// sent unless the answer says every file was stored.
fn bulk(dav: &Dav, home: &str, files: &[(PathBuf, String, u64)], p: &Progress) -> Result<()> {
    let boundary = format!("foldav{:x}", std::process::id());
    let mut body: Vec<u8> = Vec::new();
    for (local, to, _) in files {
        let data = std::fs::read(local).map_err(io)?;
        body.extend(format!(
            "--{boundary}\r\nX-File-Mtime: {}\r\nX-File-Path: /{to}\r\nX-File-MD5: {}\r\nContent-Length: {}\r\n\r\n",
            mtime(local),
            crate::md5::md5_hex(&data),
            data.len()
        ).into_bytes());
        body.extend(data);
        body.extend(b"\r\n");
    }
    body.extend(format!("--{boundary}--\r\n").into_bytes());
    let root = home.split_once("/files/").map_or("", |(r, _)| r);
    let content_type = format!("multipart/related; boundary={boundary}");
    let len = body.len() as u64;
    let r = dav.upload(
        "POST",
        &format!("{root}/bulk"),
        &[("Content-Type", &content_type)],
        len,
        || Ok(Box::new(std::io::Cursor::new(body.clone()))),
    )?;
    let said = Json::parse(&r.body);
    let all_stored = r.status == 200
        && said.as_ref().is_some_and(|j| {
            matches!(j, Json::Obj(m) if m.len() == files.len()
                && m.values().all(|f| f.get("error") == Some(&Json::Bool(false))))
        });
    if !all_stored {
        return Err(Error::Http(r.status, r.body));
    }
    p.add(files.iter().map(|f| f.2).sum());
    Ok(())
}

/// Send `len` bytes of `local` from `offset` as the chunk `chunk`, again if the connection
/// drops (what the failed try counted is taken back).
fn send_chunk(
    dav: &Dav,
    chunk: &str,
    total: &str,
    local: &Path,
    offset: u64,
    len: u64,
    p: &Progress,
) -> Result<()> {
    for attempt in 1.. {
        let sent = Arc::new(AtomicU64::new(0));
        let r = dav
            .upload("PUT", chunk, &[("OC-Total-Length", total)], len, || {
                open(local, offset, len, p, &sent)
            })
            .and_then(|r| answer(r.status, chunk, r.body));
        match r {
            Err(Error::Net(_)) if attempt < CHUNK_TRIES && !p.is_cancelled() => {
                p.sub(sent.load(Relaxed));
            }
            r => return r,
        }
    }
    unreachable!("the loop returns")
}

/// Upload `local` as the file `href`, replacing what is there, if its ETag is `if_match` (when
/// given: `Error::Conflict` if not); the ETag of what was stored.
pub(crate) fn put(
    dav: &Dav,
    home: &str,
    href: &str,
    local: &Path,
    size: u64,
    if_match: Option<&str>,
    p: &Progress,
) -> Result<Option<String>> {
    let modified = mtime(local).to_string();
    let condition: Vec<_> = if_match.map(|e| ("If-Match", e)).into_iter().collect();
    if size < CHUNKED_FROM {
        let headers = [&[("X-OC-Mtime", modified.as_str())][..], &condition].concat();
        let r = dav.upload("PUT", href, &headers, size, || {
            open(local, 0, size, p, &Arc::default())
        })?;
        answer(r.status, href, r.body)?;
        return Ok(r.etag);
    }
    static UPLOADS: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let folder = format!(
        "{}foldav-{}-{nanos}-{}",
        home.replacen("/files/", "/uploads/", 1),
        std::process::id(),
        UPLOADS.fetch_add(1, Relaxed)
    );
    let (total, to) = (size.to_string(), dav.url(href));
    let r = (|| {
        let head = [("Destination", to.as_str()), ("OC-Total-Length", &total)];
        let r = dav.request("MKCOL", &folder, &head, "")?;
        answer(r.status, &folder, r.body)?;
        // The chunks go a few at a time (the server takes five at once).
        let chunks: Vec<(usize, u64, u64)> = (0..size)
            .step_by(CHUNK as usize)
            .enumerate()
            .map(|(i, offset)| (i, offset, CHUNK.min(size - offset)))
            .collect();
        let next = AtomicUsize::new(0);
        let failed: Mutex<Option<Error>> = Mutex::new(None);
        std::thread::scope(|scope| {
            for _ in 0..PARALLEL.min(chunks.len()) {
                scope.spawn(|| {
                    while failed.lock().unwrap().is_none() && !p.is_cancelled() {
                        let Some(&(i, offset, len)) = chunks.get(next.fetch_add(1, Relaxed)) else {
                            break;
                        };
                        let chunk = format!("{folder}/{:05}", i + 1);
                        let sent = send_chunk(dav, &chunk, &total, local, offset, len, p);
                        if let Err(e) = sent {
                            failed.lock().unwrap().get_or_insert(e);
                        }
                    }
                });
            }
        });
        if let Some(e) = failed.into_inner().unwrap() {
            return Err(e);
        }
        if p.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let headers = [
            &[
                ("Destination", to.as_str()),
                ("OC-Total-Length", &total),
                ("X-OC-Mtime", &modified),
                ("Overwrite", "T"),
            ][..],
            &condition,
        ]
        .concat();
        let r = dav.request("MOVE", &format!("{folder}/.file"), &headers, "")?;
        answer(r.status, href, r.body)?;
        Ok(r.etag)
    })();
    if r.is_err() {
        // What was sent so far is of no use.
        let _ = dav.request("DELETE", &folder, &[], "");
    }
    r
}
