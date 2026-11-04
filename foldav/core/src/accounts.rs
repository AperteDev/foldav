//! Adding, reconnecting and removing Nextcloud accounts, with the app passwords the front-end
//! keeps in its platform's keyring. Blocking: call from a background thread.

use crate::{Account, Client};

/// Where a front-end keeps the app passwords of accounts signed in in Foldav (an empty
/// `Account::source`), by server and user.
pub trait Keyring {
    fn store(&self, url: &str, user: &str, password: &str) -> Result<(), String>;
    fn clear(&self, url: &str, user: &str);
}

/// Why an account could not be added, reconnected or removed.
#[derive(Debug)]
pub enum Error {
    /// The account is here already.
    Exists,
    /// It is gone (removed meanwhile).
    Gone,
    /// The keyring did not take the password.
    SavePassword(String),
    Core(crate::Error),
}

impl From<crate::Error> for Error {
    fn from(e: crate::Error) -> Self {
        match e {
            crate::Error::Exists(_) => Error::Exists,
            e => Error::Core(e),
        }
    }
}

/// Add a Nextcloud account at `url` and connect it; `source` says where its password comes from
/// (empty: signed in in Foldav, the password goes into the keyring). With `settings`, settings
/// sync through it.
pub fn add(
    c: &Client,
    keyring: &dyn Keyring,
    url: &str,
    user: &str,
    password: String,
    source: String,
    settings: bool,
) -> Result<i64, Error> {
    let url = url.trim_end_matches('/').to_string();
    let empty = source.is_empty();
    let id = c.add_account(url.clone(), user.into(), source)?;
    if empty && let Err(e) = keyring.store(&url, user, &password) {
        let _ = c.remove_account(id);
        return Err(Error::SavePassword(e));
    }
    c.connect(id, password)?;
    if settings {
        let _ = c.set_settings_account(Some(id));
    }
    Ok(id)
}

/// Account `id` (signed in in Foldav) at `url` with `password`: a new app password, or the same
/// user on a moved server.
pub fn reconnect(
    c: &Client,
    keyring: &dyn Keyring,
    id: i64,
    url: &str,
    password: String,
) -> Result<(), Error> {
    let a = c.account(id).ok_or(Error::Gone)?;
    let url = url.trim_end_matches('/').to_string();
    keyring
        .store(&url, &a.user, &password)
        .map_err(Error::SavePassword)?;
    if url == a.url {
        return Ok(c.connect(id, password)?);
    }
    c.move_account(id, url, password)?;
    keyring.clear(&a.url, &a.user);
    Ok(())
}

/// Remove account `id` from this device, with its saved password. Nothing is kept here that
/// the server does not have, so nothing is lost.
pub fn remove(c: &Client, keyring: &dyn Keyring, id: i64) -> Result<(), Error> {
    let Some(a) = c.account(id) else {
        return Ok(());
    };
    if a.source.is_empty() {
        keyring.clear(&a.url, &a.user);
    }
    Ok(c.remove_account(id)?)
}

/// The host of a Nextcloud account's server, as its default name shows it ("user@host").
pub fn host(a: &Account) -> &str {
    let host = a.url.split_once("://").map_or(a.url.as_str(), |(_, h)| h);
    host.trim_end_matches('/')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A keyring in memory, refusing to store when `refuse`.
    #[derive(Default)]
    struct Memory {
        kept: RefCell<Vec<(String, String)>>,
        refuse: bool,
    }

    impl Keyring for Memory {
        fn store(&self, url: &str, user: &str, _: &str) -> Result<(), String> {
            if self.refuse {
                return Err("locked".into());
            }
            self.kept.borrow_mut().push((url.into(), user.into()));
            Ok(())
        }

        fn clear(&self, url: &str, user: &str) {
            self.kept
                .borrow_mut()
                .retain(|k| k != &(url.into(), user.into()));
        }
    }

    fn client() -> Client {
        let dir = std::env::temp_dir().join(format!(
            "foldav-accounts-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        Client::open(dir.to_string_lossy().into()).unwrap()
    }

    #[test]
    fn added_kept_and_removed_with_its_password() {
        let (c, k) = (client(), Memory::default());
        let id = add(
            &c,
            &k,
            "https://cloud.example/",
            "ann",
            "pw".into(),
            "".into(),
            true,
        )
        .unwrap();
        assert_eq!(c.account(id).unwrap().url, "https://cloud.example");
        assert_eq!(c.settings_account(), Some(id));
        assert_eq!(
            *k.kept.borrow(),
            [("https://cloud.example".into(), "ann".into())]
        );
        let again = add(
            &c,
            &k,
            "https://cloud.example",
            "ann",
            "pw".into(),
            "".into(),
            false,
        );
        assert!(matches!(again, Err(Error::Exists)));
        remove(&c, &k, id).unwrap();
        assert!(c.accounts().is_empty() && k.kept.borrow().is_empty());
    }

    #[test]
    fn a_password_the_keyring_refuses_adds_nothing() {
        let c = client();
        let k = Memory {
            refuse: true,
            ..Default::default()
        };
        let r = add(
            &c,
            &k,
            "https://cloud.example",
            "ann",
            "pw".into(),
            "".into(),
            false,
        );
        assert!(matches!(r, Err(Error::SavePassword(_))));
        assert!(c.accounts().is_empty());
    }

    #[test]
    fn the_host_of_its_name() {
        let a = |url: &str| Account {
            id: 1,
            name: String::new(),
            url: url.into(),
            user: "ann".into(),
            source: String::new(),
            enabled: true,
        };
        assert_eq!(host(&a("https://cloud.example/")), "cloud.example");
        assert_eq!(host(&a("cloud.example/nc")), "cloud.example/nc");
    }
}
