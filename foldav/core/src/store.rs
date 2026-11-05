//! SQLite schema, migrations by `PRAGMA user_version`, and the key-value table.

use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension};

/// The schema, then each change to it (none yet), applied in order from the database's
/// `user_version`. Until the first release the schema is edited in place (a database made by an
/// earlier build is deleted, not converted); from the first release on, released steps are
/// never edited: a change is a new step (see `AGENTS.md`).
const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE kv (key TEXT PRIMARY KEY, value TEXT);
-- The Nextcloud servers files are browsed on (`account.rs`).
CREATE TABLE accounts (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL DEFAULT '',        -- given by the user; '': the front-end names it
  url TEXT NOT NULL DEFAULT '',
  user TEXT NOT NULL DEFAULT '',
  source TEXT NOT NULL DEFAULT '',      -- the front-end's: where the password comes from
  enabled INTEGER NOT NULL DEFAULT 1,
  position INTEGER NOT NULL DEFAULT 0
);
-- What was last listed of each folder (`cache.rs`): the folder's ETag, and what was in it.
CREATE TABLE listings (
  account INTEGER NOT NULL,
  path TEXT NOT NULL,                   -- the folder, relative to the Files root, '' the root
  etag TEXT NOT NULL,
  PRIMARY KEY (account, path)
);
CREATE TABLE entries (
  account INTEGER NOT NULL,
  dir TEXT NOT NULL,                    -- the folder it is in
  path TEXT NOT NULL,
  name TEXT NOT NULL,
  folder INTEGER NOT NULL,
  size INTEGER,
  modified INTEGER NOT NULL,
  mime TEXT NOT NULL,
  favorite INTEGER NOT NULL,
  has_preview INTEGER NOT NULL,
  etag TEXT NOT NULL,
  fileid TEXT NOT NULL,
  PRIMARY KEY (account, path)
);
CREATE INDEX entries_dir ON entries (account, dir);
-- Nextcloud's favorites (`favorites.rs`): what the server last said, and the changes made here
-- that it does not have yet.
CREATE TABLE favorites (
  account INTEGER NOT NULL,
  path TEXT NOT NULL,                   -- relative to the Files root
  folder INTEGER NOT NULL,
  dirty INTEGER NOT NULL DEFAULT 0,     -- 0 as on the server, 1 starred here, 2 unstarred here
  PRIMARY KEY (account, path)
);
"#];

pub fn open(path: &str) -> Result<Connection> {
    let db = Connection::open(path)?;
    // WAL with NORMAL syncing: a commit does not wait for the disk. The app crashing loses
    // nothing; a power cut can lose the last few commits (mostly listings, listed again).
    db.execute_batch(
        "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;",
    )?;
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    // A newer Foldav's: its schema may not be what this one reads and writes.
    if version > MIGRATIONS.len() as i64 {
        return Err(Error::Newer(format!(
            "{path}: schema {version}, this version knows up to {}",
            MIGRATIONS.len()
        )));
    }
    for (i, m) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let tx = db.unchecked_transaction()?;
        tx.execute_batch(m)?;
        tx.pragma_update(None, "user_version", i as i64 + 1)?;
        tx.commit()?;
    }
    Ok(db)
}

pub fn kv_get(db: &Connection, key: &str) -> Result<Option<String>> {
    Ok(db
        .query_row("SELECT value FROM kv WHERE key = ?", [key], |r| r.get(0))
        .optional()?)
}

pub fn kv_set(db: &Connection, key: &str, value: &str) -> Result<()> {
    db.execute(
        "INSERT OR REPLACE INTO kv (key, value) VALUES (?, ?)",
        [key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_foldavs_database_is_left_alone() {
        let dir = std::env::temp_dir().join(format!("foldav-test-schema-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("foldav.sqlite").to_string_lossy().to_string();
        drop(open(&path).unwrap());
        // Opened again: nothing to do.
        let db = open(&path).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        db.pragma_update(None, "user_version", version + 1).unwrap();
        drop(db);
        assert!(matches!(open(&path), Err(Error::Newer(_))));
    }
}
