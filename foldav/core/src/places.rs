//! The places of Nextcloud Files that are not folders, as the web app lists them: Recent, the
//! favorites, what is shared, the files of a tag, the trash bin, the results of a search. Each
//! is a list of files the server builds, so it has no folder ETag to cache by; its "path" starts
//! with NUL, which no folder's can, and `Client::list` gives its files as for a folder. The
//! files in it have their own paths (for the trash bin: `trash/<name in the bin>`), so what is
//! done to files works on them as it does anywhere.

use crate::dav::{NC, OC, xml_escape};
use crate::files::entry_file;
use crate::{Client, Error, File, Result};
use std::collections::HashSet;

pub const RECENT: &str = "\0recent";
pub const FAVORITES: &str = "\0favorites";
pub const TRASH: &str = "\0trash";
pub const SHARED_WITH_YOU: &str = "\0shared-with-you";
pub const SHARED_WITH_OTHERS: &str = "\0shared-with-others";
pub const SHARED_BY_LINK: &str = "\0shared-links";
/// The tags, listed as folders.
pub const TAGS: &str = "\0tags";
pub const TAG_PREFIX: &str = "\0tag:";
const SEARCH: &str = "\0search\0";
/// A file in the trash bin has this before its name (what comes after is the bin's own name
/// for it, "a.txt.d1700000000").
pub const IN_TRASH: &str = "\0trash/";

/// A place, not a folder.
pub fn is_place(path: &str) -> bool {
    path.starts_with('\0')
}

/// The files of tag `id`.
pub fn tag_place(id: &str) -> String {
    format!("{TAG_PREFIX}{id}")
}

/// A search for `query` in folder `folder` and below.
pub fn search_place(folder: &str, query: &str) -> String {
    format!("{SEARCH}{folder}\0{query}")
}

/// The folder and query of a search place.
pub fn search_of(path: &str) -> Option<(&str, &str)> {
    path.strip_prefix(SEARCH)?.split_once('\0')
}

/// A tag files can have.
#[derive(Debug, Clone, PartialEq)]
pub struct Tag {
    pub id: String,
    pub name: String,
}

const NOT_A_FOLDER: &str = "<d:not><d:eq><d:prop><d:getcontenttype/></d:prop><d:literal>httpd/unix-directory</d:literal></d:eq></d:not>";

impl Client {
    /// The files of place `path` (see the module), as listed: in the server's order.
    pub fn place(&self, id: i64, path: &str) -> Result<Vec<File>> {
        let home = self.files_home(id)?;
        let dav = self.dav(id)?;
        let scope = |folder: &str| {
            let user = home.trim_end_matches('/');
            let user = user.rsplit('/').next().unwrap_or_default();
            format!("/files/{user}/{folder}")
        };
        let entries = match path {
            RECENT => {
                let since = crate::now() - 30 * 86400;
                let cond = format!(
                    "<d:and><d:gt><d:prop><d:getlastmodified/></d:prop><d:literal>{since}</d:literal></d:gt>{NOT_A_FOLDER}</d:and>"
                );
                dav.search(&scope(""), &cond, 100)?
            }
            FAVORITES => dav.filter_files(&home, "<oc:favorite>1</oc:favorite>")?,
            TRASH => return self.trash(id),
            // The tags as folders, each opening the files it is on.
            TAGS => {
                return Ok(self
                    .tags(id)?
                    .into_iter()
                    .map(|t| File {
                        name: t.name,
                        path: tag_place(&t.id),
                        folder: true,
                        size: None,
                        modified: 0,
                        mime: String::new(),
                        favorite: false,
                        has_preview: false,
                        etag: String::new(),
                        fileid: String::new(),
                    })
                    .collect());
            }
            SHARED_WITH_YOU => return self.shares(id, "shared_with_me=true", |_| true),
            SHARED_WITH_OTHERS => return self.shares(id, "", |_| true),
            SHARED_BY_LINK => return self.shares(id, "", |s| s.share_type == 3),
            p if p.starts_with(TAG_PREFIX) => {
                let rule = format!(
                    "<oc:systemtag>{}</oc:systemtag>",
                    xml_escape(&p[TAG_PREFIX.len()..])
                );
                dav.filter_files(&home, &rule)?
            }
            p => {
                let (folder, query) = search_of(p).ok_or_else(|| Error::NotFound(p.into()))?;
                let cond = format!(
                    "<d:like><d:prop><d:displayname/></d:prop><d:literal>%{}%</d:literal></d:like>",
                    xml_escape(query)
                );
                dav.search(&scope(folder), &cond, 200)?
            }
        };
        Ok(entries
            .into_iter()
            .filter_map(|e| entry_file(&home, e))
            .collect())
    }

    /// The trash bin's address: `/remote.php/dav/trashbin/<user>/`.
    fn trash_home(&self, id: i64) -> Result<String> {
        Ok(self.files_home(id)?.replacen("/files/", "/trashbin/", 1))
    }

    /// What was deleted and is still in the trash bin, the last deleted first.
    fn trash(&self, id: i64) -> Result<Vec<File>> {
        let home = self.trash_home(id)?;
        let props = "<nc:trashbin-filename/><nc:trashbin-original-location/>\
            <nc:trashbin-deletion-time/><d:getcontentlength/><oc:size/><d:resourcetype/>\
            <d:getcontenttype/>";
        let base = format!("{home}trash/");
        let mut files: Vec<File> = self
            .dav(id)?
            .propfind(&base, "1", props)?
            .into_iter()
            .skip(1)
            .filter(|r| r.status == 200)
            .map(|r| {
                let name = r
                    .href
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or_default();
                File {
                    name: r
                        .props
                        .find_text(NC, "trashbin-filename")
                        .unwrap_or_else(|| name.to_string()),
                    path: format!("{IN_TRASH}{name}"),
                    folder: r
                        .props
                        .child(crate::dav::DAV, "resourcetype")
                        .is_some_and(|t| t.child(crate::dav::DAV, "collection").is_some()),
                    size: r
                        .props
                        .find_text(crate::dav::DAV, "getcontentlength")
                        .or_else(|| r.props.find_text(OC, "size"))
                        .and_then(|s| s.parse().ok()),
                    modified: r
                        .props
                        .find_text(NC, "trashbin-deletion-time")
                        .and_then(|t| t.parse().ok())
                        .unwrap_or(0),
                    mime: r
                        .props
                        .find_text(crate::dav::DAV, "getcontenttype")
                        .unwrap_or_default(),
                    favorite: false,
                    has_preview: false,
                    etag: String::new(),
                    fileid: String::new(),
                }
            })
            .collect();
        files.sort_by_key(|f| std::cmp::Reverse(f.modified));
        Ok(files)
    }

    /// Put files of the trash bin (their `path`) back where they were deleted from.
    pub fn trash_restore(&self, id: i64, paths: &[String]) -> Result<()> {
        let home = self.trash_home(id)?;
        let dav = self.dav(id)?;
        for p in paths {
            let name = p.strip_prefix(IN_TRASH).unwrap_or(p);
            let to = dav.url(&format!("{home}restore/{name}"));
            let r = dav.request(
                "MOVE",
                &format!("{home}trash/{name}"),
                &[("Destination", &to)],
                "",
            )?;
            match r.status {
                200..=299 => {}
                404 => return Err(Error::NotFound(name.into())),
                // The place is taken again.
                412 => return Err(Error::Exists(name.into())),
                s => return Err(Error::Http(s, r.body)),
            }
        }
        Ok(())
    }

    /// Delete files of the trash bin for good.
    pub fn trash_delete(&self, id: i64, paths: &[String]) -> Result<()> {
        let home = self.trash_home(id)?;
        let dav = self.dav(id)?;
        paths.iter().try_for_each(|p| {
            dav.delete(
                &format!("{home}trash/{}", p.strip_prefix(IN_TRASH).unwrap_or(p)),
                None,
            )
        })
    }

    /// Delete everything in the trash bin for good.
    pub fn trash_empty(&self, id: i64) -> Result<()> {
        let home = self.trash_home(id)?;
        self.dav(id)?.delete(&format!("{home}trash"), None)
    }

    /// The shares of the user (`query`: what to ask besides), those `keep` says, as the files
    /// shared, each once.
    fn shares(&self, id: i64, query: &str, keep: impl Fn(&Share) -> bool) -> Result<Vec<File>> {
        let q = if query.is_empty() { "" } else { "?" };
        let data = self.dav(id)?.ocs(
            "GET",
            &format!("apps/files_sharing/api/v1/shares{q}{query}"),
            "",
        )?;
        let mut seen = HashSet::new();
        let mut files = Vec::new();
        for s in data
            .arr()
            .iter()
            .map(|j| Share::of(j, query.contains("shared_with_me")))
        {
            // Waiting to be accepted: not there yet.
            if !keep(&s) || s.state > 0 || s.path.is_empty() || !seen.insert(s.path.clone()) {
                continue;
            }
            files.push(File {
                name: s.path.rsplit('/').next().unwrap_or_default().to_string(),
                path: s.path.trim_matches('/').to_string(),
                folder: s.folder,
                size: None,
                modified: s.time,
                mime: s.mime,
                favorite: false,
                has_preview: false,
                etag: String::new(),
                fileid: s.source,
            });
        }
        files.sort_by_key(|f| std::cmp::Reverse(f.modified));
        Ok(files)
    }

    /// The tags files can have (the server's, shared by its users).
    pub fn tags(&self, id: i64) -> Result<Vec<Tag>> {
        let root = self.files_home(id)?;
        let root = root.split_once("/files/").map_or("", |(r, _)| r);
        let props = "<oc:id/><oc:display-name/><oc:user-visible/>";
        Ok(self
            .dav(id)?
            .propfind(&format!("{root}/systemtags/"), "1", props)?
            .into_iter()
            .skip(1)
            .filter(|r| r.status == 200)
            .filter_map(|r| {
                Some(Tag {
                    id: r.props.find_text(OC, "id")?,
                    name: r.props.find_text(OC, "display-name")?,
                })
            })
            .collect())
    }
}

/// One share of the OCS answer.
struct Share {
    path: String,
    folder: bool,
    mime: String,
    time: i64,
    source: String,
    share_type: i64,
    state: i64,
}

impl Share {
    fn of(j: &crate::json::Json, with_me: bool) -> Share {
        let text = |k| {
            j.get(k)
                .and_then(crate::json::Json::str)
                .unwrap_or_default()
        };
        let num = |k| j.get(k).and_then(crate::json::Json::num).unwrap_or(0.0) as i64;
        // What is shared with the user is at `file_target`, what the user shares at `path`.
        let path = if with_me {
            text("file_target")
        } else {
            text("path")
        };
        Share {
            path: path.to_string(),
            folder: text("item_type") == "folder",
            mime: text("mimetype").to_string(),
            time: num("stime"),
            source: match j.get("file_source") {
                Some(crate::json::Json::Num(n)) => (*n as i64).to_string(),
                Some(crate::json::Json::Str(s)) => s.clone(),
                _ => String::new(),
            },
            share_type: num("share_type"),
            state: num("state"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_are_not_folders() {
        assert!(is_place(RECENT) && is_place(&tag_place("3")) && !is_place("a/b") && !is_place(""));
        let s = search_place("Docs/x", "cat");
        assert_eq!(search_of(&s), Some(("Docs/x", "cat")));
        assert_eq!(search_of(RECENT), None);
    }
}
