//! Foldav for Linux: the GTK app. There is no background service: closing the window quits.

mod add_account;
mod goa;
mod i18n;
mod settings;
mod update;
mod window;

use adw::prelude::*;
use foldav_core::{Account, ChangeListener, Client, accounts};
use gtk::{gio, glib};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

pub const APP_ID: &str = "dev.aperte.Foldav";

static CORE: OnceLock<Arc<Client>> = OnceLock::new();

pub fn core() -> &'static Arc<Client> {
    CORE.get().expect("core opened at startup")
}

/// Events from background threads, handled on the main thread.
struct Listener;
impl ChangeListener for Listener {
    fn settings_changed(&self) {
        glib::MainContext::default().invoke(|| {
            settings::apply_color_scheme();
        });
    }
}

/// Pull the synced settings, for the account they go through (blocking: a background thread).
pub fn sync_settings() {
    gio::spawn_blocking(|| {
        let _ = core().sync_settings();
    });
}

// --- keyring -----------------------------------------------------------------

/// `FOLDAV_SANDBOX` set: a separate copy for testing other connections (`make -C linux sandbox`).
/// It runs next to the normal app, keeps its data where XDG_DATA_HOME says and its passwords
/// apart, so signing out there leaves the normal app signed in.
pub fn sandbox() -> bool {
    std::env::var_os("FOLDAV_SANDBOX").is_some()
}

fn schema() -> libsecret::Schema {
    let attrs = HashMap::from([
        ("url", libsecret::SchemaAttributeType::String),
        ("user", libsecret::SchemaAttributeType::String),
    ]);
    let name = if sandbox() {
        format!("{APP_ID}.Sandbox")
    } else {
        APP_ID.into()
    };
    libsecret::Schema::new(&name, libsecret::SchemaFlags::NONE, attrs)
}

pub fn password_load(url: &str, user: &str) -> Option<String> {
    let attrs = HashMap::from([("url", url), ("user", user)]);
    libsecret::password_lookup_sync(Some(&schema()), attrs, gio::Cancellable::NONE)
        .ok()?
        .map(|s| s.to_string())
}

pub fn password_store(url: &str, user: &str, password: &str) -> Result<(), glib::Error> {
    let attrs = HashMap::from([("url", url), ("user", user)]);
    let label = format!("Foldav ({user} @ {url})");
    libsecret::password_store_sync(
        Some(&schema()),
        attrs,
        Some(libsecret::COLLECTION_DEFAULT),
        &label,
        password,
        gio::Cancellable::NONE,
    )
}

pub fn password_clear(url: &str, user: &str) {
    let attrs = HashMap::from([("url", url), ("user", user)]);
    let _ = libsecret::password_clear_sync(Some(&schema()), attrs, gio::Cancellable::NONE);
}

/// The Secret Service, for the core's account flows.
struct SecretService;

impl accounts::Keyring for SecretService {
    fn store(&self, url: &str, user: &str, password: &str) -> Result<(), String> {
        password_store(url, user, password).map_err(|e| e.to_string())
    }

    fn clear(&self, url: &str, user: &str) {
        password_clear(url, user);
    }
}

/// What went wrong adding, reconnecting or removing an account, as the user reads it.
fn account_error(e: accounts::Error) -> String {
    match e {
        accounts::Error::Exists => i18n::gettext("This account is already added"),
        accounts::Error::Gone => i18n::gettext("Connecting failed"),
        accounts::Error::SavePassword(e) => {
            i18n::gettext("Cannot save password: {error}").replace("{error}", &e)
        }
        accounts::Error::Core(e) => e.to_string(),
    }
}

// --- accounts ----------------------------------------------------------------

/// What an account is called: the name given to it, else "user@host".
pub fn account_name(a: &Account) -> String {
    if a.name.is_empty() {
        format!("{}@{}", a.user, accounts::host(a))
    } else {
        a.name.clone()
    }
}

/// Connect an account with its saved password: from GNOME Online Accounts (its `source`) or
/// Foldav's keyring entry. False without one (keyring cleared, GOA account gone or with files
/// turned off).
pub fn connect_saved(a: &Account) -> bool {
    let pw = if a.source.is_empty() {
        password_load(&a.url, &a.user)
    } else if goa::accounts().iter().any(|g| g.id == a.source) {
        goa::password(&a.source)
    } else {
        None
    };
    pw.is_some_and(|pw| core().connect(a.id, pw).is_ok())
}

/// Connect every account with its saved password; false when there is none (the welcome page
/// shows).
pub fn restore_accounts() -> bool {
    let all = core().accounts();
    for a in &all {
        connect_saved(a);
    }
    !all.is_empty()
}

/// A Nextcloud account to add.
pub enum Target {
    Goa(goa::Account),
    /// Signed in here (browser or password); the app password goes into Foldav's keyring entry.
    Manual(foldav_core::Credentials),
}

/// Add an account and connect it (`settings`: settings sync through it).
pub async fn add_account(target: Target, settings: bool) -> Result<i64, String> {
    let id = gio::spawn_blocking(move || -> Result<i64, String> {
        let (url, user, pw, source) = match target {
            Target::Goa(a) => {
                let pw = goa::password(&a.id).ok_or_else(|| {
                    i18n::gettext("Online Accounts has no password for this account")
                })?;
                (a.url, a.user, pw, a.id)
            }
            Target::Manual(c) => (c.server, c.login_name, c.app_password, String::new()),
        };
        accounts::add(core(), &SecretService, &url, &user, pw, source, settings)
            .map_err(account_error)
    })
    .await
    .map_err(|_| i18n::gettext("Connecting failed"))??;
    sync_settings();
    Ok(id)
}

/// Account `id` (signed in here) at `url` with `password`: a new app password, or the same
/// user on a moved server.
pub async fn reconnect(id: i64, url: String, password: String) -> Result<(), String> {
    gio::spawn_blocking(move || {
        accounts::reconnect(core(), &SecretService, id, &url, password).map_err(account_error)
    })
    .await
    .map_err(|_| i18n::gettext("Connecting failed"))??;
    Ok(())
}

/// Switch account `id` on or off (off hides its files).
pub async fn enable_account(id: i64, on: bool) {
    let _ = gio::spawn_blocking(move || core().set_account_enabled(id, on)).await;
    window::account_changed();
}

/// Remove account `id` from this device, with its saved password.
pub async fn remove_account(id: i64) -> Result<(), String> {
    gio::spawn_blocking(move || {
        accounts::remove(core(), &SecretService, id).map_err(account_error)
    })
    .await
    .map_err(|_| i18n::gettext("Removing the account failed"))??;
    if core().accounts().is_empty() {
        window::show_welcome();
    } else {
        window::account_changed();
    }
    Ok(())
}

/// The data could not be opened (left by a newer Foldav, say): a window saying so, and nothing
/// else runs; closing it quits.
fn open_failed(e: &foldav_core::Error) -> glib::ExitCode {
    eprintln!("open local database: {e}");
    let description = match e {
        foldav_core::Error::Newer(_) => i18n::gettext(
            "It was saved by a newer version of Foldav. Update Foldav to open it; this version leaves it as it is.",
        ),
        e => e.to_string(),
    };
    let app = adw::Application::new(Some(APP_ID), gio::ApplicationFlags::NON_UNIQUE);
    app.connect_activate(move |app| {
        let page = adw::StatusPage::builder()
            .icon_name("dialog-error-symbolic")
            .title(i18n::gettext("Foldav Cannot Open Its Data"))
            .description(description.as_str())
            .build();
        let view = adw::ToolbarView::new();
        view.add_top_bar(&adw::HeaderBar::new());
        view.set_content(Some(&page));
        adw::ApplicationWindow::builder()
            .application(app)
            .title("Foldav")
            .default_width(480)
            .default_height(360)
            .content(&view)
            .build()
            .present();
    });
    app.run_with_args::<&str>(&[])
}

fn main() -> glib::ExitCode {
    i18n::init();
    gio::resources_register_include!("icons.gresource").expect("register icons");
    let data = glib::user_data_dir().join("foldav");
    let client = match Client::open(data.to_string_lossy().into()) {
        // The copies of files go to the cache folder, where the system can clean them; the
        // sandbox keeps them with its data.
        Ok(c) if sandbox() => c,
        Ok(c) => c.cache_in(glib::user_cache_dir().join("foldav")),
        Err(e) => return open_failed(&e),
    };
    client.subscribe(Box::new(Listener));
    let _ = CORE.set(Arc::new(client));

    let flags = if sandbox() {
        gio::ApplicationFlags::NON_UNIQUE
    } else {
        // A restart after an update starts the new version before this one has quit (Flatpak).
        gio::ApplicationFlags::ALLOW_REPLACEMENT
    };
    let app = adw::Application::new(Some(APP_ID), flags);
    app.connect_startup(|app| {
        window::setup_actions(app);
        settings::apply_color_scheme();
        if restore_accounts() {
            sync_settings();
        }
        update::watch(app);
        // The copies files were opened from, not used for a month.
        gio::spawn_blocking(|| core().prune_cache(std::time::Duration::from_secs(30 * 86400)));
    });
    app.connect_activate(window::present);
    let code = app.run();
    update::exec_if_restarting();
    code
}
