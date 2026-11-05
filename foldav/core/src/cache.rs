//! What was listed of each folder, kept on this device: shown at once, and the server asked
//! only by the folder's ETag (`Client::list`). Nothing here is the user's own: it is all
//! what the server said, and goes with the account.

use crate::files::sort;
use crate::{Client, File, Result};
use rusqlite::{Connection, params};

impl Client {
    /// What was last listed of folder `path` of account `id`, as the server had it then, with
    /// no request; None when it was never listed.
    pub fn cached(&self, id: i64, path: &str) -> Option<Vec<File>> {
        let db = self.db.lock().unwrap();
        let path = path.trim_matches('/');
        let listed: bool = db
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM listings WHERE account = ? AND path = ?)",
                params![id, path],
                |r| r.get(0),
            )
            .ok()?;
        if !listed {
            return None;
        }
        let mut st = db
            .prepare(
                "SELECT name, path, folder, size, modified, mime, favorite, has_preview, etag,
                    fileid FROM entries WHERE account = ? AND dir = ?",
            )
            .ok()?;
        let mut files: Vec<File> = st
            .query_map(params![id, path], |r| {
                Ok(File {
                    name: r.get(0)?,
                    path: r.get(1)?,
                    folder: r.get(2)?,
                    size: r.get::<_, Option<i64>>(3)?.map(|s| s as u64),
                    modified: r.get(4)?,
                    mime: r.get(5)?,
                    favorite: r.get(6)?,
                    has_preview: r.get(7)?,
                    etag: r.get(8)?,
                    fileid: r.get(9)?,
                })
            })
            .ok()?
            .collect::<rusqlite::Result<_>>()
            .ok()?;
        sort(&mut files);
        Some(files)
    }

    /// The ETag folder `path` had when it was last listed.
    pub(crate) fn listing_etag(&self, id: i64, path: &str) -> Option<String> {
        let db = self.db.lock().unwrap();
        db.query_row(
            "SELECT etag FROM listings WHERE account = ? AND path = ?",
            params![id, path.trim_matches('/')],
            |r| r.get(0),
        )
        .ok()
    }

    /// Keep `files` as the listing of folder `path`, with its `etag`. The folders that were in
    /// it and are not any more take what was kept of them with them.
    pub(crate) fn store_listing(
        &self,
        id: i64,
        path: &str,
        etag: &str,
        files: &[File],
    ) -> Result<()> {
        let path = path.trim_matches('/');
        let db = self.db.lock().unwrap();
        let tx = db.unchecked_transaction()?;
        let old: Vec<String> = {
            let mut st = tx
                .prepare("SELECT path FROM entries WHERE account = ? AND dir = ? AND folder = 1")?;
            st.query_map(params![id, path], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        tx.execute(
            "DELETE FROM entries WHERE account = ? AND dir = ?",
            params![id, path],
        )?;
        let mut insert = tx.prepare_cached(
            "INSERT INTO entries (account, dir, path, name, folder, size, modified, mime,
                favorite, has_preview, etag, fileid) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )?;
        for f in files {
            insert.execute(params![
                id,
                path,
                f.path,
                f.name,
                f.folder,
                f.size.map(|s| s as i64),
                f.modified,
                f.mime,
                f.favorite,
                f.has_preview,
                f.etag,
                f.fileid
            ])?;
        }
        drop(insert);
        tx.execute(
            "INSERT OR REPLACE INTO listings (account, path, etag) VALUES (?, ?, ?)",
            params![id, path, etag],
        )?;
        let folders: std::collections::HashSet<&str> = files
            .iter()
            .filter(|f| f.folder)
            .map(|f| f.path.as_str())
            .collect();
        for gone in old.iter().filter(|p| !folders.contains(p.as_str())) {
            drop_tree(&tx, id, gone)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Folder `path` is not on the server any more: neither is what was kept of it.
    pub(crate) fn drop_listing(&self, id: i64, path: &str) {
        let db = self.db.lock().unwrap();
        let _ = drop_tree(&db, id, path.trim_matches('/'));
    }
}

/// Forget the listing of folder `path` and of every folder below it.
fn drop_tree(db: &Connection, account: i64, path: &str) -> Result<()> {
    db.execute(
        "DELETE FROM entries WHERE account = ?1
           AND (dir = ?2 OR substr(dir, 1, length(?2) + 1) = ?2 || '/')",
        params![account, path],
    )?;
    db.execute(
        "DELETE FROM listings WHERE account = ?1
           AND (path = ?2 OR substr(path, 1, length(?2) + 1) = ?2 || '/')",
        params![account, path],
    )?;
    Ok(())
}

/// Forget everything kept of account `id`.
pub(crate) fn clear(db: &Connection, id: i64) -> Result<()> {
    db.execute("DELETE FROM entries WHERE account = ?", [id])?;
    db.execute("DELETE FROM listings WHERE account = ?", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, folder: bool) -> File {
        File {
            name: path.rsplit('/').next().unwrap().into(),
            path: path.into(),
            folder,
            size: (!folder).then_some(7),
            modified: 5,
            mime: if folder { "" } else { "text/plain" }.into(),
            favorite: false,
            has_preview: false,
            etag: "e".into(),
            fileid: "1".into(),
        }
    }

    #[test]
    fn listings_are_kept_per_account_and_a_gone_folder_takes_its_own_along() {
        let dir = std::env::temp_dir().join(format!("foldav-test-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let c = Client::open(dir.to_string_lossy().into()).unwrap();
        assert_eq!(c.cached(1, ""), None, "never listed");

        c.store_listing(1, "", "r1", &[file("b.txt", false), file("A", true)])
            .unwrap();
        c.store_listing(1, "A", "a1", &[file("A/x", true), file("A/f.txt", false)])
            .unwrap();
        c.store_listing(1, "A/x", "x1", &[file("A/x/y.txt", false)])
            .unwrap();
        c.store_listing(2, "", "other", &[file("z", false)])
            .unwrap();
        let names = |id, p| -> Option<Vec<String>> {
            c.cached(id, p)
                .map(|f| f.into_iter().map(|f| f.path).collect())
        };
        // Folders first, then by name.
        assert_eq!(names(1, ""), Some(vec!["A".to_string(), "b.txt".into()]));
        assert_eq!(c.listing_etag(1, "A").as_deref(), Some("a1"));
        assert_eq!(c.cached(1, "A/").unwrap().len(), 2);
        assert_eq!(names(2, ""), Some(vec!["z".to_string()]));
        // An empty folder was listed too.
        c.store_listing(1, "A/x", "x2", &[]).unwrap();
        assert_eq!(c.cached(1, "A/x"), Some(vec![]));

        // A: its folder x gone, and what was kept of x with it; its own listing replaced.
        c.store_listing(1, "A", "a2", &[file("A/f.txt", false)])
            .unwrap();
        assert_eq!(c.cached(1, "A/x"), None);
        assert_eq!(names(1, "A"), Some(vec!["A/f.txt".to_string()]));
        // The root still has A; removed with the account's cache only account 1 goes.
        c.drop_listing(1, "A");
        assert_eq!(c.cached(1, "A"), None);
        clear(&c.db.lock().unwrap(), 1).unwrap();
        assert_eq!(c.cached(1, ""), None);
        assert!(c.cached(2, "").is_some());
    }
}
