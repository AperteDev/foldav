//! Browsing: what is in a folder of an account's Nextcloud Files.

use crate::dav::percent_decode;
use crate::{Client, Error, Result};

/// A file or folder in a folder listing.
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    /// The name, as shown.
    pub name: String,
    /// Relative to the Files root, no slashes at either end: "Photos/2026/cat.jpg".
    pub path: String,
    pub folder: bool,
    /// Bytes: a file's, or what a folder holds.
    pub size: Option<u64>,
    /// Unix time.
    pub modified: i64,
    pub mime: String,
    pub favorite: bool,
    /// The server has a preview to show instead of a generic icon.
    pub has_preview: bool,
    pub etag: String,
    /// `oc:id`: stays with the file across a rename or move.
    pub fileid: String,
}

impl File {
    /// Hidden (a dot first), as in the web app and Files.
    pub fn hidden(&self) -> bool {
        self.name.starts_with('.')
    }
}

/// Folders first, then by name, case ignored.
pub(crate) fn sort(files: &mut [File]) {
    files.sort_by_cached_key(|f| (!f.folder, f.name.to_lowercase()));
}

impl Client {
    /// What is directly in folder `path` of account `id` (empty: the Files root): folders first,
    /// then by name, case ignored; hidden files (`File::hidden`) included, the front-end
    /// decides. Blocking: a request to the server, a small one when the folder is the same as
    /// when it was last listed (its ETag says so; `cached` has that listing without any).
    pub fn list(&self, id: i64, path: &str) -> Result<Vec<File>> {
        let home = self.files_home(id)?;
        let dir = format!("{home}{}", path.trim_matches('/'));
        let dir = format!("{}/", dir.trim_end_matches('/'));
        let dav = self.dav(id)?;
        if let Some(known) = self.listing_etag(id, path) {
            match dav.list(&dir, "0")?.first() {
                None => {
                    self.drop_listing(id, path);
                    return Err(Error::NotFound(path.into()));
                }
                Some(head) if !head.etag.is_empty() && head.etag == known => {
                    if let Some(files) = self.cached(id, path) {
                        return Ok(files);
                    }
                }
                Some(_) => {}
            }
        }
        let entries = dav.list(&dir, "1")?;
        if entries.is_empty() {
            self.drop_listing(id, path);
            return Err(Error::NotFound(path.into()));
        }
        let etag = entries[0].etag.clone();
        // The listing's hrefs are percent-encoded; the folder's is the first (and the same
        // as `dir`, but for encoding).
        let mut files: Vec<File> = entries
            .into_iter()
            .skip(1)
            .filter_map(|e| {
                let href = percent_decode(&e.href);
                let rel = href.strip_prefix(&home)?.trim_matches('/').to_string();
                Some(File {
                    name: rel.rsplit('/').next()?.to_string(),
                    path: rel,
                    folder: e.folder,
                    size: e.size,
                    modified: e.modified,
                    mime: e.mime,
                    favorite: e.favorite,
                    has_preview: e.has_preview,
                    etag: e.etag,
                    fileid: e.fileid,
                })
            })
            .collect();
        sort(&mut files);
        // A cache that cannot be written is no reason not to show the folder.
        if !etag.is_empty() {
            let _ = self.store_listing(id, path, &etag, &files);
        }
        Ok(files)
    }
}
