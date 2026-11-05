//! Previews of files (what the server has made of images, documents, videos, and the start of
//! text files), kept on this device by the file's ETag: a changed file is another preview, an
//! unchanged one is never asked for twice. What is kept goes by age and by the room it takes
//! (`prune_thumbnails`, `limit_thumbnails`).

use crate::transfer::{dirs, io, older};
use crate::{Client, Error, File, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The folder (under an account's) a text file's start is kept in, where the previews of images
/// have one for each size.
const TEXT_DIR: &str = "text";

impl Client {
    /// Where what is kept of `file` (account `id`) as `kind` (a size in pixels, or `TEXT_DIR`)
    /// is, once it was fetched.
    fn kept_path(&self, id: i64, file: &File, kind: &str) -> Option<PathBuf> {
        if file.folder || file.fileid.is_empty() {
            return None;
        }
        // The ETag is a quoted hash: its letters and digits tell it apart.
        let etag: String = file
            .etag
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        Some(
            self.cache_dir
                .join("thumbs")
                .join(id.to_string())
                .join(kind)
                .join(format!("{}-{etag}", file.fileid)),
        )
    }

    /// Where the preview of `file` (account `id`, `size` pixels at most each way) is kept, once
    /// it was fetched.
    pub fn thumbnail_path(&self, id: i64, file: &File, size: u32) -> Option<PathBuf> {
        file.has_preview
            .then(|| self.kept_path(id, file, &size.to_string()))
            .flatten()
    }

    /// The preview of `file`, from this device when it is there, else asked of the server (and
    /// kept). None when the file has none to give.
    pub fn thumbnail(&self, id: i64, file: &File, size: u32) -> Result<Option<PathBuf>> {
        let Some(path) = self.thumbnail_path(id, file, size) else {
            return Ok(None);
        };
        if path.exists() {
            return Ok(Some(path));
        }
        let dav = self.dav(id)?;
        let url = format!(
            "{}/index.php/core/preview?fileId={}&x={size}&y={size}&a=true",
            dav.base(),
            file.number()
        );
        let mut dl = match dav.get(&url, None) {
            Ok(Some(dl)) => dl,
            // No preview for it.
            Ok(None) | Err(Error::NotFound(_)) => return Ok(None),
            Err(e) => return Err(e),
        };
        let mut bytes = Vec::new();
        // A preview is small: more than this is not one.
        dl.reader()
            .take(8 << 20)
            .read_to_end(&mut bytes)
            .map_err(|e| Error::Net(e.to_string()))?;
        keep(&path, &file.fileid, &bytes)?;
        Ok(Some(path))
    }

    /// The start of the text file `file` (at most `max` bytes), for a preview: from this device
    /// when it is there, else read from the server (only that much of it) and kept. None when
    /// it is no text (not UTF-8, or it has a NUL), whatever its type says.
    pub fn text_head(&self, id: i64, file: &File, max: usize) -> Result<Option<String>> {
        if !file.is_text() || file.size == Some(0) {
            return Ok(None);
        }
        let kept = self.kept_path(id, file, TEXT_DIR);
        if let Some(text) = kept.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
            return Ok(Some(text));
        }
        let href = format!("{}{}", self.files_home(id)?, file.path);
        let Some(mut dl) = self.dav(id)?.get_head(&href, max as u64)? else {
            return Ok(None);
        };
        let mut bytes = Vec::new();
        dl.reader()
            .take(max as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| Error::Net(e.to_string()))?;
        // The cut may be inside a character: that end is not an error.
        let text = match String::from_utf8(bytes) {
            Ok(t) => t,
            Err(e) if e.utf8_error().error_len().is_none() => {
                let valid = e.utf8_error().valid_up_to();
                String::from_utf8_lossy(&e.into_bytes()[..valid]).into_owned()
            }
            Err(_) => return Ok(None),
        };
        if text.contains('\0') {
            return Ok(None);
        }
        if let Some(path) = kept {
            // Kept for next time; a failure to is no loss.
            let _ = keep(&path, &file.fileid, text.as_bytes());
        }
        Ok(Some(text))
    }

    /// Everything kept: the previews and the starts of texts.
    fn kept_previews(&self) -> Vec<PathBuf> {
        dirs(&self.cache_dir.join("thumbs"))
            .iter()
            .flat_map(|account| dirs(account))
            .flat_map(|kind| dirs(&kind))
            .collect()
    }

    /// Delete the previews kept longer than `max_age`.
    pub fn prune_thumbnails(&self, max_age: Duration) {
        for thumb in self.kept_previews() {
            if older(&thumb, max_age) {
                let _ = std::fs::remove_file(thumb);
            }
        }
    }

    /// Delete the oldest of what is kept while it takes more than `limit` bytes.
    pub fn limit_thumbnails(&self, limit: u64) {
        let mut all: Vec<(SystemTime, u64, PathBuf)> = self
            .kept_previews()
            .into_iter()
            .filter_map(|p| {
                let m = std::fs::metadata(&p).ok()?;
                Some((m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len(), p))
            })
            .collect();
        let mut total: u64 = all.iter().map(|t| t.1).sum();
        all.sort();
        for (_, len, path) in all {
            if total <= limit {
                break;
            }
            if std::fs::remove_file(path).is_ok() {
                total -= len;
            }
        }
    }
}

/// Write `bytes` to `path`, whole or not there at all, and delete what was kept of the file
/// (`fileid`) before: its ETag was another.
fn keep(path: &Path, fileid: &str, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().ok_or_else(|| Error::Io("no folder".into()))?;
    std::fs::create_dir_all(dir).map_err(io)?;
    let prefix = format!("{fileid}-");
    for old in dirs(dir) {
        if old
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(&prefix))
        {
            let _ = std::fs::remove_file(old);
        }
    }
    static PARTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = PARTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let part = path.with_extension(format!("{n}.part"));
    std::fs::write(&part, bytes).map_err(io)?;
    std::fs::rename(&part, path).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_oldest_previews_go_when_they_take_too_much() {
        let dir = std::env::temp_dir().join(format!("foldav-test-limit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let c = Client::open(dir.to_string_lossy().into()).unwrap();
        let kept = c.cache_dir.join("thumbs/1/1024");
        std::fs::create_dir_all(&kept).unwrap();
        for (name, days) in [("old", 3), ("older", 5), ("new", 1)] {
            let p = kept.join(name);
            std::fs::write(&p, [0u8; 10]).unwrap();
            std::fs::File::options()
                .write(true)
                .open(&p)
                .unwrap()
                .set_modified(SystemTime::now() - Duration::from_secs(days * 86400))
                .unwrap();
        }
        c.limit_thumbnails(20);
        assert!(!kept.join("older").exists() && kept.join("old").exists());
        c.limit_thumbnails(10);
        assert!(!kept.join("old").exists() && kept.join("new").exists());
    }
}
