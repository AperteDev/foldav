//! Operations on files and folders: each is one request to the server, whatever the size of the
//! folder. Paths are relative to the Files root, as in `File::path`. A move or deletion is kept
//! in the journal, to undo (`Client::undo`); the ones that fail midway keep what went through.

use crate::dav::{NC, percent_decode};
use crate::undo::Change;
use crate::{Client, Error, Result};
use std::collections::HashSet;

/// Whether `name` can be given to a file or folder.
pub fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.chars().any(char::is_control)
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.into()
    } else {
        format!("{dir}/{name}")
    }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(p, _)| p)
}

fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `path` is `folder` or below it.
fn inside(path: &str, folder: &str) -> bool {
    path == folder
        || path
            .strip_prefix(folder)
            .is_some_and(|r| r.starts_with('/'))
}

/// The name of a copy: "a.txt" gives "a (copy).txt", then "a (copy 2).txt"; a folder's whole
/// name is the stem.
pub(crate) fn copy_name(name: &str, n: u32, folder: bool) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if !folder && i > 0 => name.split_at(i),
        _ => (name, ""),
    };
    match n {
        1 => format!("{stem} (copy){ext}"),
        n => format!("{stem} (copy {n}){ext}"),
    }
}

/// A name taken: the front-end says which.
fn taken(e: Error, name: &str) -> Error {
    match e {
        Error::Conflict => Error::Exists(name.into()),
        e => e,
    }
}

impl Client {
    /// The href of `path` of account `id`.
    fn href(&self, id: i64, path: &str) -> Result<String> {
        Ok(format!("{}{path}", self.files_home(id)?))
    }

    /// Run `f`, which adds what it did to `done`; the journal keeps that, also when it fails.
    fn run(&self, f: impl FnOnce(&mut Vec<Change>) -> Result<()>) -> Result<Option<u64>> {
        let mut done = Vec::new();
        let r = f(&mut done);
        let step = self.journal.lock().unwrap().record(done);
        r.map(|()| step)
    }

    /// Make folder `name` in folder `parent` (empty: the root). `Error::Exists` when something
    /// has that name.
    pub fn new_folder(&self, id: i64, parent: &str, name: &str) -> Result<()> {
        if !valid_name(name) {
            return Err(Error::Invalid(name.into()));
        }
        let href = self.href(id, &join(parent, name))?;
        self.dav(id)?.mkcol(&href).map_err(|e| taken(e, name))
    }

    /// Give the file or folder at `path` another name; the step to undo it, None when the name
    /// is the same.
    pub fn rename(&self, id: i64, path: &str, name: &str) -> Result<Option<u64>> {
        if !valid_name(name) {
            return Err(Error::Invalid(name.into()));
        }
        self.move_to_names(id, [(path.to_string(), join(parent(path), name))])
    }

    /// Move files and folders into folder `dest` of the same account; one step to undo.
    /// `Error::Exists` where one has that name already, `Invalid` for a folder into itself.
    pub fn move_to(&self, id: i64, paths: &[String], dest: &str) -> Result<Option<u64>> {
        self.move_to_names(
            id,
            paths
                .iter()
                .map(|p| (p.clone(), join(dest, name_of(p))))
                .collect::<Vec<_>>(),
        )
    }

    fn move_to_names(
        &self,
        id: i64,
        moves: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Option<u64>> {
        let dav = self.dav(id)?;
        self.run(|done| {
            for (from, to) in moves {
                if from == to {
                    continue;
                }
                if inside(&to, &from) {
                    return Err(Error::Invalid(name_of(&from).into()));
                }
                dav.mv(&self.href(id, &from)?, &self.href(id, &to)?)
                    .map_err(|e| taken(e, name_of(&to)))?;
                done.push(Change::Moved {
                    account: id,
                    from,
                    to,
                });
            }
            Ok(())
        })
    }

    /// Copy files and folders into folder `dest` of the same account, each a name that is
    /// free there ("a (copy).txt"), so nothing is overwritten. `Invalid` for a folder into
    /// itself.
    pub fn copy_to(&self, id: i64, paths: &[String], dest: &str) -> Result<()> {
        let dav = self.dav(id)?;
        let dir = format!("{}/", self.href(id, dest)?.trim_end_matches('/'));
        // Hidden files count, `list` leaves them out.
        let mut names: HashSet<String> = dav
            .list(&dir, "1")?
            .into_iter()
            .skip(1)
            .map(|e| name_of(percent_decode(&e.href).trim_end_matches('/')).to_string())
            .collect();
        for path in paths {
            if inside(dest, path) {
                return Err(Error::Invalid(name_of(path).into()));
            }
            let from = self.href(id, path)?;
            let name = name_of(path);
            let mut to = name.to_string();
            if names.contains(&to) {
                let folder = dav.list(&from, "0")?.first().is_some_and(|e| e.folder);
                to = (1..)
                    .map(|n| copy_name(name, n, folder))
                    .find(|n| !names.contains(n))
                    .unwrap_or_default();
            }
            dav.copy(&from, &self.href(id, &join(dest, &to))?)
                .map_err(|e| taken(e, &to))?;
            names.insert(to);
        }
        Ok(())
    }

    /// Delete files and folders: to Nextcloud's trash bin where the server has one, and one
    /// step to undo it.
    pub fn delete(&self, id: i64, paths: &[String]) -> Result<Option<u64>> {
        let dav = self.dav(id)?;
        self.run(|done| {
            for path in paths {
                dav.delete(&self.href(id, path)?, None)?;
                done.push(Change::Deleted {
                    account: id,
                    path: path.clone(),
                });
            }
            Ok(())
        })
    }

    /// Undo step `step` (the latest when None): moves go back, deleted files come back from the
    /// trash. `Error::NotFound` when there is nothing to undo.
    pub fn undo(&self, step: Option<u64>) -> Result<()> {
        let changes = self
            .journal
            .lock()
            .unwrap()
            .take(step)
            .ok_or_else(|| Error::NotFound("nothing to undo".into()))?;
        for change in changes.into_iter().rev() {
            match change {
                Change::Moved { account, from, to } => self
                    .dav(account)?
                    .mv(&self.href(account, &to)?, &self.href(account, &from)?)
                    .map_err(|e| taken(e, name_of(&from)))?,
                Change::Deleted { account, path } => self.restore_trashed(account, &path)?,
            }
        }
        Ok(())
    }

    /// Put the file or folder last deleted from `path` back where it was.
    // ponytail: lists the whole trash bin; fine until it is huge, then ask for the one entry.
    fn restore_trashed(&self, id: i64, path: &str) -> Result<()> {
        let dav = self.dav(id)?;
        let trash = self.files_home(id)?.replacen("/files/", "/trashbin/", 1);
        let props = "<nc:trashbin-original-location/><nc:trashbin-deletion-time/>";
        let found = dav
            .propfind(&format!("{trash}trash/"), "1", props)?
            .into_iter()
            .filter(|r| {
                r.status == 200
                    && r.props
                        .find_text(NC, "trashbin-original-location")
                        .as_deref()
                        == Some(path)
            })
            .max_by_key(|r| {
                r.props
                    .find_text(NC, "trashbin-deletion-time")
                    .and_then(|t| t.parse::<i64>().ok())
            })
            .ok_or_else(|| Error::NotFound(path.into()))?;
        let name = found
            .href
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default();
        let to = dav.url(&format!("{trash}restore/{name}"));
        // Not `Dav::mv`: the restore folder answers its `Overwrite: F` with 412.
        let r = dav.request("MOVE", &found.href, &[("Destination", &to)], "")?;
        match r.status {
            200..=299 => Ok(()),
            404 => Err(Error::NotFound(path.into())),
            // The place is taken again.
            412 => Err(Error::Exists(name_of(path).into())),
            s => Err(Error::Http(s, r.body)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_that_can_be_given() {
        for ok in ["a", "My Photos", "a.b", ".hidden", "ż"] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in ["", " ", ".", "..", "a/b", "a\nb"] {
            assert!(!valid_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn paths_and_what_is_inside() {
        assert_eq!(join("", "a"), "a");
        assert_eq!(join("x/y", "a"), "x/y/a");
        assert_eq!((parent("x/y/a"), parent("a")), ("x/y", ""));
        assert_eq!(name_of("x/y/a"), "a");
        assert!(inside("x/y", "x") && inside("x", "x") && !inside("xy", "x"));
        assert!(!inside("a", ""), "the root is not a path");
    }

    #[test]
    fn copies_are_named_after_what_they_copy() {
        assert_eq!(copy_name("a.txt", 1, false), "a (copy).txt");
        assert_eq!(copy_name("a.tar.gz", 2, false), "a.tar (copy 2).gz");
        assert_eq!(copy_name("v1.2", 1, true), "v1.2 (copy)");
        assert_eq!(copy_name(".bashrc", 1, false), ".bashrc (copy)");
        assert_eq!(copy_name("noext", 3, false), "noext (copy 3)");
    }

    #[test]
    fn the_journal_hands_back_steps_latest_first() {
        let mut j = crate::undo::Journal::default();
        let moved = |from: &str| {
            vec![Change::Moved {
                account: 1,
                from: from.into(),
                to: "z".into(),
            }]
        };
        assert_eq!(j.record(vec![]), None);
        let (a, b) = (j.record(moved("a")).unwrap(), j.record(moved("b")).unwrap());
        assert_eq!(j.take(Some(a)), Some(moved("a")));
        assert_eq!(j.take(None), Some(moved("b")));
        assert_eq!((j.take(Some(b)), j.take(None)), (None, None));
    }
}
