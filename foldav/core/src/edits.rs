//! Changes made to the copy of a file that another app opened (`Client::open_file`): found, and
//! sent back to the server. The ETag the copy was taken at is checked first: a file that changed
//! on the server meanwhile is not overwritten, the changed copy is kept next to it as "a
//! (conflicted copy).txt", as the desktop client does.
//!
//! A folder of the copy (`files/<account>/<file id>/`) holds the copy (`f/<name>`), the ETag it
//! was taken at (`etag`), and what says it was changed: its path on the server (`path`), its
//! name (`name`) and its size and date as they were when taken or sent (`stamp`).

use crate::ops::{join, labeled_name, name_of};
use crate::transfer::{dirs, io, older};
use crate::upload::put;
use crate::{Client, File, Progress, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A copy changed this long ago is left to settle: the other app may be still writing it.
const SETTLE: Duration = Duration::from_secs(2);

/// What became of a changed copy.
#[derive(Debug, PartialEq, Eq)]
pub enum Saved {
    /// Sent: the file on the server is the changed one.
    Uploaded,
    /// The file changed on the server meanwhile: the copy was sent as a new file of this name.
    Conflict(String),
}

fn stamp(copy: &Path) -> Option<String> {
    let m = std::fs::metadata(copy).ok()?;
    let nanos = m
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(format!("{}:{nanos}", m.len()))
}

/// Remember `copy` of `file` as it is now: what a later change is seen against.
pub(crate) fn record(dir: &Path, file: &File, copy: &Path) -> Result<()> {
    std::fs::write(dir.join("name"), &file.name).map_err(io)?;
    std::fs::write(dir.join("path"), &file.path).map_err(io)?;
    std::fs::write(dir.join("stamp"), stamp(copy).unwrap_or_default()).map_err(io)
}

/// The copy in `dir`, when it was changed since it was taken or sent.
pub(crate) fn changed(dir: &Path) -> Option<PathBuf> {
    let name = std::fs::read_to_string(dir.join("name")).ok()?;
    let copy = dir.join("f").join(name);
    let was = std::fs::read_to_string(dir.join("stamp")).ok()?;
    (stamp(&copy)? != was).then_some(copy)
}

impl Client {
    /// The folders of the copies changed and settled, with their account.
    fn changed_copies(&self) -> Vec<(i64, PathBuf)> {
        let mut found = Vec::new();
        for account in dirs(&self.cache_dir.join("files")) {
            let Some(id) = account.file_name().and_then(|n| n.to_str()?.parse().ok()) else {
                continue;
            };
            for dir in dirs(&account) {
                if changed(&dir).is_some_and(|c| older(&c, SETTLE)) {
                    found.push((id, dir));
                }
            }
        }
        found
    }

    /// Some copy was changed and not sent yet: cheap, to ask often.
    pub fn has_changes(&self) -> bool {
        !self.changed_copies().is_empty()
    }

    /// Send the changed copies back; for each its name and what became of it.
    pub fn save_changes(&self, p: &Progress) -> Vec<(String, Result<Saved>)> {
        let copies = self.changed_copies();
        let size = |dir: &PathBuf| changed(dir).and_then(|c| std::fs::metadata(c).ok());
        p.set_total(
            copies
                .iter()
                .filter_map(|(_, d)| size(d))
                .map(|m| m.len())
                .sum(),
        );
        copies
            .into_iter()
            .map(|(id, dir)| {
                let name = std::fs::read_to_string(dir.join("name")).unwrap_or_default();
                (
                    name,
                    self.send_copy(id, &dir, p)
                        .map(|s| s.unwrap_or(Saved::Uploaded)),
                )
            })
            .collect()
    }

    /// Send the copy in `dir`, if it was changed.
    pub(crate) fn send_copy(&self, id: i64, dir: &Path, p: &Progress) -> Result<Option<Saved>> {
        let Some(copy) = changed(dir) else {
            return Ok(None);
        };
        let read = |n: &str| std::fs::read_to_string(dir.join(n)).unwrap_or_default();
        let (path, etag) = (read("path"), read("etag"));
        // What is stored is what the copy was when the sending began: a later change shows.
        let Some(now) = stamp(&copy) else {
            return Ok(None);
        };
        let size = std::fs::metadata(&copy).map_err(io)?.len();
        let (dav, home) = (self.dav(id)?, self.files_home(id)?);
        let sent = if etag.is_empty() {
            Err(crate::Error::Conflict)
        } else {
            put(
                &dav,
                &home,
                &format!("{home}{path}"),
                &copy,
                size,
                Some(&etag),
                p,
            )
        };
        let saved = match sent {
            Ok(new) => {
                let new = match new {
                    Some(e) => e,
                    None => dav
                        .list(&format!("{home}{path}"), "0")?
                        .first()
                        .map(|e| e.etag.clone())
                        .unwrap_or_default(),
                };
                std::fs::write(dir.join("etag"), new).map_err(io)?;
                Saved::Uploaded
            }
            Err(crate::Error::Conflict) => {
                let parent = path.rsplit_once('/').map_or("", |(p, _)| p);
                let names = self.names_in(id, parent)?;
                let name = (1..)
                    .map(|n| labeled_name(name_of(&path), "conflicted copy", n, false))
                    .find(|n| !names.contains(n))
                    .unwrap_or_default();
                put(
                    &dav,
                    &home,
                    &format!("{home}{}", join(parent, &name)),
                    &copy,
                    size,
                    None,
                    p,
                )?;
                // The copy is of an old file now: the next opening fetches the current one.
                std::fs::write(dir.join("etag"), "").map_err(io)?;
                Saved::Conflict(name)
            }
            Err(e) => return Err(e),
        };
        std::fs::write(dir.join("stamp"), now).map_err(io)?;
        Ok(Some(saved))
    }
}
