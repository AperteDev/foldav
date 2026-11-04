//! Bookmarks: folders the user keeps in the sidebar, per account, on this device.

use crate::{Client, Result};
use rusqlite::params;

#[derive(Debug, Clone, PartialEq)]
pub struct Bookmark {
    pub id: i64,
    /// The folder, relative to the Files root; empty: the root.
    pub path: String,
}

impl Client {
    /// Account `account`'s bookmarks, in the order added.
    pub fn bookmarks(&self, account: i64) -> Vec<Bookmark> {
        let db = self.db.lock().unwrap();
        let st = db.prepare("SELECT id, path FROM bookmarks WHERE account = ? ORDER BY id");
        st.and_then(|mut st| {
            st.query_map([account], |r| {
                Ok(Bookmark {
                    id: r.get(0)?,
                    path: r.get(1)?,
                })
            })?
            .collect()
        })
        .unwrap_or_default()
    }

    /// Bookmark folder `path` of `account`; nothing happens when it is bookmarked already.
    pub fn add_bookmark(&self, account: i64, path: &str) -> Result<()> {
        self.db.lock().unwrap().execute(
            "INSERT OR IGNORE INTO bookmarks (account, path) VALUES (?, ?)",
            params![account, path.trim_matches('/')],
        )?;
        Ok(())
    }

    pub fn remove_bookmark(&self, id: i64) -> Result<()> {
        self.db
            .lock()
            .unwrap()
            .execute("DELETE FROM bookmarks WHERE id = ?", [id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmarks_are_per_account_unique_and_go_with_it() {
        let dir =
            std::env::temp_dir().join(format!("foldav-test-bookmarks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let c = Client::open(dir.to_string_lossy().into()).unwrap();
        let a = c
            .add_account("https://one.example".into(), "kris".into(), "".into())
            .unwrap();
        let b = c
            .add_account("https://two.example".into(), "kris".into(), "".into())
            .unwrap();
        c.add_bookmark(a, "Photos/").unwrap();
        c.add_bookmark(a, "Photos").unwrap();
        c.add_bookmark(a, "").unwrap();
        c.add_bookmark(b, "Photos").unwrap();
        let paths = |id| -> Vec<String> { c.bookmarks(id).into_iter().map(|b| b.path).collect() };
        assert_eq!(paths(a), ["Photos", ""]);
        assert_eq!(paths(b), ["Photos"]);
        let first = c.bookmarks(a)[0].id;
        c.remove_bookmark(first).unwrap();
        assert_eq!(paths(a), [""]);
        c.remove_account(a).unwrap();
        assert!(paths(a).is_empty() && paths(b) == ["Photos"]);
    }
}
