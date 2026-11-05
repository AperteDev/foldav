//! File contents coming from the server: a copy to open a file from (kept, and checked against
//! the server's ETag before it is used again), and files and folders saved to a folder of this
//! device. Both stream to disk and report to a `Progress` the front-end reads from another thread.

use crate::dav::Download;
use crate::ops::copy_name;
use crate::{Client, Error, File, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::time::{Duration, SystemTime};

/// How far a transfer is, and a way to stop it; shared between the thread doing it and the one
/// showing it.
#[derive(Default, Clone)]
pub struct Progress(std::sync::Arc<Counters>);

#[derive(Default)]
struct Counters {
    done: AtomicU64,
    total: AtomicU64,
    cancelled: AtomicBool,
}

impl Progress {
    /// Bytes transferred so far.
    pub fn done(&self) -> u64 {
        self.0.done.load(Relaxed)
    }

    /// Bytes to transfer; 0 while it is not known.
    pub fn total(&self) -> u64 {
        self.0.total.load(Relaxed)
    }

    /// Stop the transfer: it ends with `Error::Cancelled` at its next read.
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Relaxed)
    }

    pub(crate) fn add(&self, n: u64) {
        self.0.done.fetch_add(n, Relaxed);
    }

    pub(crate) fn set_done(&self, n: u64) {
        self.0.done.store(n, Relaxed);
    }

    pub(crate) fn set_total(&self, n: u64) {
        self.0.total.store(n, Relaxed);
    }
}

pub(crate) fn io(e: std::io::Error) -> Error {
    Error::Io(e.to_string())
}

/// Write what `dl` brings to `target`, by way of a part file next to it, so what is there is
/// whole or not there at all.
pub(crate) fn save(mut dl: Download, target: &Path, p: &Progress) -> Result<()> {
    static PARTS: AtomicU64 = AtomicU64::new(0);
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let part = target.with_file_name(format!(".{name}.{}.part", PARTS.fetch_add(1, Relaxed)));
    let copied = (|| {
        let mut out = std::fs::File::create(&part).map_err(io)?;
        let mut body = dl.reader();
        let mut buf = vec![0; 64 * 1024];
        loop {
            if p.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let n = body.read(&mut buf).map_err(|e| Error::Net(e.to_string()))?;
            if n == 0 {
                return Ok(());
            }
            out.write_all(&buf[..n]).map_err(io)?;
            p.add(n as u64);
        }
    })();
    match copied.and_then(|()| std::fs::rename(&part, target).map_err(io)) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

/// What is directly in folder `p`.
pub(crate) fn dirs(p: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(p)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect()
}

/// `p` was last changed longer than `age` ago.
pub(crate) fn older(p: &Path, age: Duration) -> bool {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|d| d > age)
}

/// `name` in `dir`, or the first of its copy names ("a (copy).txt") that is free.
pub(crate) fn free_path(dir: &Path, name: &str, folder: bool) -> PathBuf {
    let free = |p: &Path| std::fs::symlink_metadata(p).is_err();
    let first = dir.join(name);
    if free(&first) {
        return first;
    }
    (1..)
        .map(|n| dir.join(copy_name(name, n, folder)))
        .find(|p| free(p))
        .unwrap_or(first)
}

impl Client {
    /// A copy of `file` of account `id` on this device, for another app to open: the one kept
    /// from before when the server says the file is the same (one small request), else it is
    /// fetched. A change the other app saves to it is sent back (`save_changes`).
    pub fn open_file(&self, id: i64, file: &File, p: &Progress) -> Result<PathBuf> {
        if file.fileid.is_empty() {
            return Err(Error::Invalid(file.name.clone()));
        }
        let dir = self
            .cache_dir
            .join("files")
            .join(id.to_string())
            .join(&file.fileid);
        let (etag_file, kept) = (dir.join("etag"), dir.join("f"));
        let copy = kept.join(&file.name);
        // What was changed in it and not sent yet goes first: fetching would lose it.
        if crate::edits::changed(&dir).is_some() {
            let _ = self.send_copy(id, &dir, p);
        }
        let known = std::fs::read_to_string(&etag_file)
            .ok()
            .filter(|e| !e.is_empty() && copy.exists());
        let href = format!("{}{}", self.files_home(id)?, file.path);
        let etag = match self.dav(id)?.get(&href, known.as_deref())? {
            None => known.unwrap_or_default(),
            Some(dl) => {
                // What was kept goes first: a file renamed since is another name in `f`.
                let _ = std::fs::remove_dir_all(&kept);
                std::fs::create_dir_all(&kept).map_err(io)?;
                p.set_total(dl.length.or(file.size).unwrap_or(0));
                let etag = dl.etag.clone().unwrap_or_default();
                save(dl, &copy, p)?;
                etag
            }
        };
        // Written again also when it did not change: its time says when the copy was last used.
        std::fs::write(&etag_file, etag).map_err(io)?;
        crate::edits::record(&dir, file, &copy)?;
        Ok(copy)
    }

    /// Delete the copies `open_file` made that were not used for `max_age`.
    pub fn prune_cache(&self, max_age: Duration) {
        for account in dirs(&self.cache_dir.join("files")) {
            for copy in dirs(&account) {
                // One with changes not sent yet stays.
                if older(&copy.join("etag"), max_age) && crate::edits::changed(&copy).is_none() {
                    let _ = std::fs::remove_dir_all(copy);
                }
            }
        }
        self.prune_thumbnails(max_age);
    }

    /// A new, empty folder on this device for files to hand to another app at once (the files
    /// of a drag); the ones of earlier hours are removed.
    pub fn scratch(&self) -> Result<PathBuf> {
        static MADE: AtomicU64 = AtomicU64::new(0);
        let base = self.cache_dir.join("drag");
        for old in dirs(&base) {
            if older(&old, Duration::from_secs(3600)) {
                let _ = std::fs::remove_dir_all(old);
            }
        }
        let dir = base.join(format!("{}-{}", crate::now(), MADE.fetch_add(1, Relaxed)));
        std::fs::create_dir_all(&dir).map_err(io)?;
        Ok(dir)
    }

    /// Save `files` (and what folders hold) of account `id` into folder `dest` of this device,
    /// each under a name that is free there, so nothing is overwritten. A folder that is not
    /// whole when it fails or is cancelled is removed.
    // ponytail: a folder is fetched file by file, one request each; `download_zip` is the
    // server's way of doing it in one.
    pub fn download(&self, id: i64, files: &[File], dest: &Path, p: &Progress) -> Result<()> {
        p.set_total(files.iter().filter_map(|f| f.size).sum());
        files.iter().try_for_each(|f| self.save_to(id, f, dest, p))
    }

    /// Save `files`, all in folder `parent` of account `id`, as one ZIP the server makes of them
    /// (one request, however many files), in folder `dest` of this device; where it is. Named
    /// after the file or folder, or after `parent` when there are several, and not taking the
    /// name of anything there.
    pub fn download_zip(
        &self,
        id: i64,
        parent: &str,
        files: &[File],
        dest: &Path,
        p: &Progress,
    ) -> Result<PathBuf> {
        let dav = self.dav(id)?;
        let home = self.files_home(id)?;
        let folder = format!("{}/", format!("{home}{parent}").trim_end_matches('/'));
        let names = crate::json::Json::Arr(
            files
                .iter()
                .map(|f| crate::json::Json::Str(f.name.clone()))
                .collect(),
        );
        let url = format!(
            "{}?files={}&accept=zip",
            dav.url(&folder),
            crate::dav::form_encode(&names.to_string())
        );
        p.set_total(files.iter().filter_map(|f| f.size).sum());
        let dl = dav
            .get_as(&url, "application/zip")?
            .ok_or_else(|| Error::NotFound(parent.into()))?;
        let name = match files {
            [f] => f.name.clone(),
            _ => parent
                .rsplit('/')
                .next()
                .filter(|n| !n.is_empty())
                .unwrap_or("Files")
                .to_string(),
        };
        let target = free_path(dest, &format!("{name}.zip"), false);
        save(dl, &target, p)?;
        Ok(target)
    }

    /// Copy `files` of account `from` into folder `dest` of account `to`, by way of this device
    /// (a download, then an upload: the progress goes through twice). `taken` is as for
    /// `upload`.
    pub fn copy_across(
        &self,
        from: i64,
        files: &[File],
        to: i64,
        dest: &str,
        taken: crate::Taken,
        p: &Progress,
    ) -> Result<()> {
        let dir = self.scratch()?;
        let r = self.download(from, files, &dir, p).and_then(|()| {
            p.set_done(0);
            let items: Vec<PathBuf> = dirs(&dir);
            self.upload(to, dest, &items, taken, p)
        });
        let _ = std::fs::remove_dir_all(&dir);
        r
    }

    fn save_to(&self, id: i64, file: &File, dir: &Path, p: &Progress) -> Result<()> {
        let target = free_path(dir, &file.name, file.folder);
        if !file.folder {
            let href = format!("{}{}", self.files_home(id)?, file.path);
            let dl = self
                .dav(id)?
                .get(&href, None)?
                .ok_or_else(|| Error::NotFound(file.path.clone()))?;
            return save(dl, &target, p);
        }
        std::fs::create_dir(&target).map_err(io)?;
        let r = self.list(id, &file.path).and_then(|inside| {
            inside
                .iter()
                .try_for_each(|f| self.save_to(id, f, &target, p))
        });
        if r.is_err() {
            let _ = std::fs::remove_dir_all(&target);
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_taken_get_a_copy_name() {
        let dir = std::env::temp_dir().join(format!("foldav-test-free-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("d")).unwrap();
        std::fs::write(dir.join("a.txt"), "").unwrap();
        std::fs::write(dir.join("a (copy).txt"), "").unwrap();
        assert_eq!(free_path(&dir, "b.txt", false), dir.join("b.txt"));
        assert_eq!(free_path(&dir, "a.txt", false), dir.join("a (copy 2).txt"));
        assert_eq!(free_path(&dir, "d", true), dir.join("d (copy)"));
    }

    #[test]
    fn copies_not_used_for_long_are_pruned() {
        let dir = std::env::temp_dir().join(format!("foldav-test-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let c = Client::open(dir.to_string_lossy().into()).unwrap();
        let files = c.cache_dir.join("files/1");
        for id in ["7", "8"] {
            std::fs::create_dir_all(files.join(id).join("f")).unwrap();
            std::fs::write(files.join(id).join("etag"), "\"x\"").unwrap();
        }
        let long_ago = SystemTime::now() - Duration::from_secs(40 * 86400);
        std::fs::File::options()
            .write(true)
            .open(files.join("7/etag"))
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
        c.prune_cache(Duration::from_secs(30 * 86400));
        assert!(!files.join("7").exists() && files.join("8").exists());
    }
}
