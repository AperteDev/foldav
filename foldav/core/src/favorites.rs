//! Favorites: Nextcloud's stars on files and folders, as Files shows them. Kept here as the
//! server last said, with the changes made here that it does not have yet (`dirty`), so they
//! work offline and go up when the server is reachable (`sync_favorites`).

use crate::{Client, Error, Result};
use rusqlite::{Connection, params};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub struct Favorite {
    /// Relative to the Files root, no slashes at either end.
    pub path: String,
    pub folder: bool,
}

const SENT: i64 = 0;
const STARRED: i64 = 1;
const UNSTARRED: i64 = 2;

impl Client {
    /// Account `id`'s favorites, by name, case ignored: as the server has them, with the
    /// changes made here, with no request.
    pub fn favorites(&self, id: i64) -> Vec<Favorite> {
        let db = self.db.lock().unwrap();
        let mut favorites = read(&db, id).unwrap_or_default();
        favorites.sort_by_cached_key(|f| f.path.to_lowercase());
        favorites
    }

    /// Star or unstar `path` of account `id`: kept here at once, and sent to the server if it can
    /// be reached now, else with the next sync.
    pub fn set_favorite(&self, id: i64, path: &str, folder: bool, on: bool) -> Result<()> {
        let path = path.trim_matches('/');
        if path.is_empty() {
            // The Files root cannot be one.
            return Err(Error::Invalid(path.into()));
        }
        let dirty = {
            let db = self.db.lock().unwrap();
            let now: Option<i64> = db
                .query_row(
                    "SELECT dirty FROM favorites WHERE account = ? AND path = ?",
                    params![id, path],
                    |r| r.get(0),
                )
                .ok();
            match (on, now) {
                // Starred here and not sent: it never was, so there is nothing to send.
                (false, Some(STARRED)) => {
                    db.execute(
                        "DELETE FROM favorites WHERE account = ? AND path = ?",
                        params![id, path],
                    )?;
                    return Ok(());
                }
                (true, Some(UNSTARRED)) => SENT, // unstarred here, starred again: as on the server
                (true, Some(_)) => return Ok(()),
                (true, None) => STARRED,
                (false, Some(UNSTARRED)) => return Ok(()),
                // Not known here: the server may have it all the same.
                (false, _) => UNSTARRED,
            }
        };
        self.db.lock().unwrap().execute(
            "INSERT INTO favorites (account, path, folder, dirty) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (account, path) DO UPDATE SET dirty = ?4",
            params![id, path, folder, dirty],
        )?;
        if dirty != SENT {
            self.send(id, path, dirty);
        }
        Ok(())
    }

    /// Send the change to `path` to the server; it is as on the server after, or stays to try
    /// again. True when it is.
    fn send(&self, id: i64, path: &str, dirty: i64) -> bool {
        let sent = (|| {
            let href = format!("{}{path}", self.files_home(id)?);
            self.dav(id)?.set_favorite(&href, dirty == STARRED)
        })();
        // Gone from the server: nothing to star.
        if !matches!(sent, Ok(()) | Err(Error::NotFound(_))) {
            return false;
        }
        let db = self.db.lock().unwrap();
        let _ = match dirty {
            STARRED if sent.is_ok() => db.execute(
                "UPDATE favorites SET dirty = 0 WHERE account = ? AND path = ?",
                params![id, path],
            ),
            _ => db.execute(
                "DELETE FROM favorites WHERE account = ? AND path = ?",
                params![id, path],
            ),
        };
        true
    }

    /// Send what was starred and unstarred here, then take what the server has: true when
    /// the favorites are not what they were. Blocking: requests to the server.
    pub fn sync_favorites(&self, id: i64) -> Result<bool> {
        let dav = self.dav(id)?;
        let waiting: Vec<(String, i64)> = {
            let db = self.db.lock().unwrap();
            let mut st =
                db.prepare("SELECT path, dirty FROM favorites WHERE account = ? AND dirty != 0")?;
            st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        for (path, dirty) in waiting {
            self.send(id, &path, dirty);
        }
        let home = self.files_home(id)?;
        let server: Vec<(String, bool)> = dav
            .favorites(&home)?
            .into_iter()
            .filter_map(|(href, folder)| {
                let path = href.strip_prefix(&home)?.trim_matches('/').to_string();
                (!path.is_empty()).then_some((path, folder))
            })
            .collect();
        merge(&self.db.lock().unwrap(), id, &server)
    }
}

fn read(db: &Connection, id: i64) -> Result<Vec<Favorite>> {
    let mut st =
        db.prepare("SELECT path, folder FROM favorites WHERE account = ? AND dirty != 2")?;
    Ok(st
        .query_map([id], |r| {
            Ok(Favorite {
                path: r.get(0)?,
                folder: r.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
}

/// Make what is kept the server's `server` (path, folder), but for the changes made here that
/// it does not have yet. True when the favorites are not what they were.
fn merge(db: &Connection, id: i64, server: &[(String, bool)]) -> Result<bool> {
    let paths = |db: &Connection| -> Result<BTreeSet<String>> {
        Ok(read(db, id)?.into_iter().map(|f| f.path).collect())
    };
    let before = paths(db)?;
    let tx = db.unchecked_transaction()?;
    let kept: Vec<(String, i64)> = {
        let mut st = tx.prepare("SELECT path, dirty FROM favorites WHERE account = ?")?;
        st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    for (path, dirty) in &kept {
        if *dirty == SENT && !server.iter().any(|(p, _)| p == path) {
            tx.execute(
                "DELETE FROM favorites WHERE account = ? AND path = ?",
                params![id, path],
            )?;
        }
    }
    for (path, folder) in server {
        tx.execute(
            "INSERT INTO favorites (account, path, folder, dirty) VALUES (?1, ?2, ?3, 0)
             ON CONFLICT (account, path) DO UPDATE SET folder = ?3 WHERE dirty = 0",
            params![id, path, folder],
        )?;
    }
    tx.commit()?;
    Ok(paths(db)? != before)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> (Client, i64) {
        let dir =
            std::env::temp_dir().join(format!("foldav-test-favorites-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let c = Client::open(dir.to_string_lossy().into()).unwrap();
        let a = c
            .add_account("https://one.example".into(), "kris".into(), "".into())
            .unwrap();
        (c, a)
    }

    fn paths(c: &Client, id: i64) -> Vec<String> {
        c.favorites(id).into_iter().map(|f| f.path).collect()
    }

    fn dirty(c: &Client, id: i64, path: &str) -> Option<i64> {
        c.db.lock()
            .unwrap()
            .query_row(
                "SELECT dirty FROM favorites WHERE account = ? AND path = ?",
                params![id, path],
                |r| r.get(0),
            )
            .ok()
    }

    #[test]
    fn what_is_starred_here_waits_for_the_server_and_the_server_is_merged_in() {
        let (c, a) = client(); // not connected: nothing can be sent
        assert!(matches!(
            c.set_favorite(a, "", true, true),
            Err(Error::Invalid(_))
        ));

        c.set_favorite(a, "Photos/", true, true).unwrap();
        assert_eq!(
            (paths(&c, a), dirty(&c, a, "Photos")),
            (vec!["Photos".to_string()], Some(STARRED))
        );
        // Unstarred before it went up: gone, nothing to send.
        c.set_favorite(a, "Photos", true, false).unwrap();
        assert_eq!((paths(&c, a), dirty(&c, a, "Photos")), (vec![], None));

        // The server's: taken in, and again with no change.
        let server = [("A".to_string(), true), ("b.txt".to_string(), false)];
        assert!(merge(&c.db.lock().unwrap(), a, &server).unwrap());
        assert!(!merge(&c.db.lock().unwrap(), a, &server).unwrap());
        assert_eq!(paths(&c, a), ["A", "b.txt"]);

        // Unstarred here: hidden at once, and the server's stars do not bring it back.
        c.set_favorite(a, "A", true, false).unwrap();
        assert_eq!(
            (paths(&c, a), dirty(&c, a, "A")),
            (vec!["b.txt".to_string()], Some(UNSTARRED))
        );
        assert!(!merge(&c.db.lock().unwrap(), a, &server).unwrap());
        assert_eq!(paths(&c, a), ["b.txt"]);
        // Starred again: as on the server.
        c.set_favorite(a, "A", true, true).unwrap();
        assert_eq!(
            (paths(&c, a), dirty(&c, a, "A")),
            (vec!["A".to_string(), "b.txt".to_string()], Some(SENT))
        );

        // Unstarred on the server by another device: gone here; what waits to go up stays.
        c.set_favorite(a, "new", true, true).unwrap();
        assert!(merge(&c.db.lock().unwrap(), a, &[("A".to_string(), true)]).unwrap());
        assert_eq!(paths(&c, a), ["A", "new"]);
    }
}
