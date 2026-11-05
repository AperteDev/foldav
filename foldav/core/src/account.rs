//! Accounts: the Nextcloud servers whose files are browsed. A switched-off account is hidden,
//! nothing is deleted.

use crate::dav::Dav;
use crate::{Client, Error, Result, store};
use rusqlite::{OptionalExtension, params};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    pub id: i64,
    /// Given by the user; empty: the front-end names it ("user@host").
    pub name: String,
    /// The server, as signed in to.
    pub url: String,
    pub user: String,
    /// The front-end's note on where the password comes from: on Linux the GNOME Online
    /// Accounts id; empty when signed in in Foldav, the password in its keyring.
    pub source: String,
    /// Off: hidden.
    pub enabled: bool,
}

const COLS: &str = "id, name, url, user, source, enabled";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Account> {
    Ok(Account {
        id: r.get(0)?,
        name: r.get(1)?,
        url: r.get(2)?,
        user: r.get(3)?,
        source: r.get(4)?,
        enabled: r.get(5)?,
    })
}

/// A per-account `kv` key.
pub(crate) fn key(account: i64, name: &str) -> String {
    format!("a{account}_{name}")
}

impl Client {
    /// Every account, in the user's order (`reorder_accounts`; the order added until then).
    pub fn accounts(&self) -> Vec<Account> {
        let db = self.db.lock().unwrap();
        let st = db.prepare(&format!(
            "SELECT {COLS} FROM accounts ORDER BY position, id"
        ));
        st.and_then(|mut st| st.query_map([], row)?.collect())
            .unwrap_or_default()
    }

    pub fn account(&self, id: i64) -> Option<Account> {
        let db = self.db.lock().unwrap();
        db.query_row(
            &format!("SELECT {COLS} FROM accounts WHERE id = ?"),
            [id],
            row,
        )
        .optional()
        .ok()?
    }

    /// A new account; returns its id. Refuses (`Error::Exists`) one already here (same server
    /// and user).
    pub fn add_account(&self, url: String, user: String, source: String) -> Result<i64> {
        let url = url.trim_end_matches('/').to_string();
        let db = self.db.lock().unwrap();
        let same: bool = db.query_row(
            "SELECT EXISTS (SELECT 1 FROM accounts WHERE url = ?1 AND user = ?2)",
            params![url, user],
            |r| r.get(0),
        )?;
        if same {
            return Err(Error::Exists(format!("{user}@{url}")));
        }
        db.execute(
            "INSERT INTO accounts (url, user, source, position)
             VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(position), 0) + 1 FROM accounts))",
            params![url, user, source],
        )?;
        Ok(db.last_insert_rowid())
    }

    /// Put the accounts in this order (ids; ones left out keep their place after them).
    pub fn reorder_accounts(&self, ids: Vec<i64>) -> Result<()> {
        let db = self.db.lock().unwrap();
        let tx = db.unchecked_transaction()?;
        tx.execute(
            "UPDATE accounts SET position = position + ?1",
            [ids.len() as i64],
        )?;
        for (i, id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE accounts SET position = ?2 WHERE id = ?1",
                params![id, i as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Name it; empty gives back the front-end's name.
    pub fn rename_account(&self, id: i64, name: String) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute(
            "UPDATE accounts SET name = ?2 WHERE id = ?1",
            params![id, name.trim()],
        )?;
        Ok(())
    }

    /// Switch an account on or off: off hides it.
    pub fn set_account_enabled(&self, id: i64, enabled: bool) -> Result<()> {
        self.db.lock().unwrap().execute(
            "UPDATE accounts SET enabled = ?2 WHERE id = ?1",
            params![id, enabled],
        )?;
        Ok(())
    }

    /// The password is kept in memory only; front-ends keep it in the platform keyring (or
    /// get it from where `Account::source` says).
    pub fn connect(&self, id: i64, password: String) -> Result<()> {
        let a = self.account(id).ok_or(Error::NoAccount)?;
        let dav = Dav::new(&a.url, &a.user, &password);
        self.dav.lock().unwrap().insert(id, Arc::new(dav));
        Ok(())
    }

    /// Connected for this run: `connect` gave the password. An account stays without one,
    /// e.g. when the keyring no longer has it.
    pub fn connected(&self, id: i64) -> bool {
        self.dav.lock().unwrap().contains_key(&id)
    }

    /// The server moved: same user and password at `url`.
    pub fn move_account(&self, id: i64, url: String, password: String) -> Result<()> {
        self.forget(id)?;
        self.db.lock().unwrap().execute(
            "UPDATE accounts SET url = ?2 WHERE id = ?1",
            params![id, url.trim_end_matches('/')],
        )?;
        self.connect(id, password)
    }

    /// Remove an account and what is kept of it from this device; the files stay on the server.
    pub fn remove_account(&self, id: i64) -> Result<()> {
        self.forget(id)?;
        self.db
            .lock()
            .unwrap()
            .execute("DELETE FROM accounts WHERE id = ?", [id])?;
        self.db
            .lock()
            .unwrap()
            .execute("DELETE FROM favorites WHERE account = ?", [id])?;
        if self.settings_account() == Some(id) {
            self.set_settings_account(None)?;
        }
        self.dav.lock().unwrap().remove(&id);
        self.journal.lock().unwrap().clear();
        Ok(())
    }

    /// Forget what was learnt of account `id`'s server, the folders listed included.
    fn forget(&self, id: i64) -> Result<()> {
        let prefix = key(id, "");
        let db = self.db.lock().unwrap();
        db.execute(
            "DELETE FROM kv WHERE substr(key, 1, ?2) = ?1",
            params![prefix, prefix.len() as i64],
        )?;
        let _ = std::fs::remove_dir_all(self.cache_dir.join("files").join(id.to_string()));
        let _ = std::fs::remove_dir_all(self.cache_dir.join("thumbs").join(id.to_string()));
        crate::cache::clear(&db, id)
    }

    /// The account whose server synced settings go through (the device-only
    /// `settings_account` setting); None: settings stay on this device.
    pub fn settings_account(&self) -> Option<i64> {
        self.setting("settings_account".into())?.parse().ok()
    }

    pub fn set_settings_account(&self, id: Option<i64>) -> Result<()> {
        let v = id.map(|i| i.to_string()).unwrap_or_default();
        self.set_setting("settings_account".into(), v)
    }

    /// Switched on and connected.
    pub fn syncing(&self, id: i64) -> bool {
        self.account(id).is_some_and(|a| a.enabled) && self.connected(id)
    }

    /// The connection of account `id`.
    pub(crate) fn dav(&self, id: i64) -> Result<Arc<Dav>> {
        self.dav
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or(Error::NoAccount)
    }

    /// Ask account `id`'s server what it is (no sign-in needed): its version, maintenance mode.
    pub fn server_info(&self, id: i64) -> Result<crate::dav::ServerInfo> {
        self.dav(id)?.server_info()
    }

    /// The user's Nextcloud Files root on account `id`, `/remote.php/dav/files/<user id>/`. The
    /// user id can differ from the login name (an email address, for one), so it comes from the
    /// principal.
    pub fn files_home(&self, id: i64) -> Result<String> {
        let k = key(id, "files_home");
        if let Some(h) = store::kv_get(&self.db.lock().unwrap(), &k)? {
            return Ok(h);
        }
        let principal = self.dav(id)?.principal()?;
        let user = principal
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or_default();
        let home = format!("/remote.php/dav/files/{user}/");
        store::kv_set(&self.db.lock().unwrap(), &k, &home)?;
        Ok(home)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_are_unique_ordered_and_removed() {
        let dir = std::env::temp_dir().join(format!("foldav-test-accounts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let c = Client::open(dir.to_string_lossy().into()).unwrap();
        let add = |url: &str| c.add_account(url.into(), "kris".into(), "".into());
        let (a, b) = (
            add("https://one.example").unwrap(),
            add("https://two.example/").unwrap(),
        );
        assert!(matches!(add("https://one.example/"), Err(Error::Exists(_))));

        // Switched off: still there.
        c.set_account_enabled(a, false).unwrap();
        assert!(!c.account(a).unwrap().enabled);
        c.set_account_enabled(a, true).unwrap();

        // Removed: with its settings account.
        c.set_settings_account(Some(a)).unwrap();
        c.remove_account(a).unwrap();
        assert_eq!(c.settings_account(), None);

        // The user's order, new ones last.
        c.reorder_accounts(vec![b]).unwrap();
        let three = add("https://three.example").unwrap();
        let order: Vec<i64> = c.accounts().iter().map(|a| a.id).collect();
        assert_eq!(order, [b, three]);
    }
}
