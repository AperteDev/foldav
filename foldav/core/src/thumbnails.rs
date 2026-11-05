//! Previews of files (what the server has made of images, documents, videos), kept on this
//! device by the file's ETag: a changed file is another preview, an unchanged one is never asked
//! for twice.

use crate::transfer::{dirs, io, older};
use crate::{Client, Error, File, Result};
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

impl Client {
    /// Where the preview of `file` (account `id`, `size` pixels at most each way) is kept, once
    /// it was fetched.
    pub fn thumbnail_path(&self, id: i64, file: &File, size: u32) -> Option<PathBuf> {
        if !file.has_preview || file.folder || file.fileid.is_empty() {
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
                .join(size.to_string())
                .join(format!("{}-{etag}", file.fileid)),
        )
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
        let dir = path.parent().ok_or_else(|| Error::Io("no folder".into()))?;
        std::fs::create_dir_all(dir).map_err(io)?;
        // The earlier previews of the file go: its ETag was another.
        let prefix = format!("{}-", file.fileid);
        for old in dirs(dir) {
            if old
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(&prefix))
            {
                let _ = std::fs::remove_file(old);
            }
        }
        // Whole or not there at all.
        static PARTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = PARTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let part = path.with_extension(format!("{n}.part"));
        std::fs::write(&part, bytes).map_err(io)?;
        std::fs::rename(&part, &path).map_err(io)?;
        Ok(Some(path))
    }

    /// Delete the previews kept longer than `max_age`.
    pub fn prune_thumbnails(&self, max_age: Duration) {
        for account in dirs(&self.cache_dir.join("thumbs")) {
            for size in dirs(&account) {
                for thumb in dirs(&size) {
                    if older(&thumb, max_age) {
                        let _ = std::fs::remove_file(thumb);
                    }
                }
            }
        }
    }
}
