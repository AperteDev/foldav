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
#[derive(Default)]
pub struct Progress {
    done: AtomicU64,
    total: AtomicU64,
    cancelled: AtomicBool,
}

impl Progress {
    /// Bytes transferred so far.
    pub fn done(&self) -> u64 {
        self.done.load(Relaxed)
    }

    /// Bytes to transfer; 0 while it is not known.
    pub fn total(&self) -> u64 {
        self.total.load(Relaxed)
    }

    /// Stop the transfer: it ends with `Error::Cancelled` at its next read.
    pub fn cancel(&self) {
        self.cancelled.store(true, Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Relaxed)
    }
}

fn io(e: std::io::Error) -> Error {
    Error::Io(e.to_string())
}

/// Write what `dl` brings to `target`, by way of a part file next to it, so what is there is
/// whole or not there at all.
fn save(mut dl: Download, target: &Path, p: &Progress) -> Result<()> {
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
            p.done.fetch_add(n as u64, Relaxed);
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

/// `name` in `dir`, or the first of its copy names ("a (copy).txt") that is free.
fn free_path(dir: &Path, name: &str, folder: bool) -> PathBuf {
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
    /// fetched. Kept read-only: a change saved to it would not reach the server.
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
                p.total.store(dl.length.or(file.size).unwrap_or(0), Relaxed);
                let etag = dl.etag.clone().unwrap_or_default();
                save(dl, &copy, p)?;
                let mut perms = std::fs::metadata(&copy).map_err(io)?.permissions();
                perms.set_readonly(true);
                std::fs::set_permissions(&copy, perms).map_err(io)?;
                etag
            }
        };
        // Written again also when it did not change: its time says when the copy was last used.
        std::fs::write(&etag_file, etag).map_err(io)?;
        Ok(copy)
    }

    /// Delete the copies `open_file` made that were not used for `max_age`.
    pub fn prune_cache(&self, max_age: Duration) {
        let old = |p: &Path| {
            std::fs::metadata(p)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| SystemTime::now().duration_since(t).ok())
                .is_some_and(|age| age > max_age)
        };
        let dirs = |p: &Path| -> Vec<PathBuf> {
            std::fs::read_dir(p)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .collect()
        };
        for account in dirs(&self.cache_dir.join("files")) {
            for copy in dirs(&account) {
                if old(&copy.join("etag")) {
                    let _ = std::fs::remove_dir_all(copy);
                }
            }
        }
    }

    /// Save `files` (and what folders hold) of account `id` into folder `dest` of this device,
    /// each under a name that is free there, so nothing is overwritten. A folder that is not
    /// whole when it fails or is cancelled is removed.
    // ponytail: a folder is fetched file by file, one request each; a ZIP from the server is
    // faster where there is one.
    pub fn download(&self, id: i64, files: &[File], dest: &Path, p: &Progress) -> Result<()> {
        p.total
            .store(files.iter().filter_map(|f| f.size).sum(), Relaxed);
        files.iter().try_for_each(|f| self.save_to(id, f, dest, p))
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
