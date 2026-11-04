//! Main window: the welcome page (sign-in), the places sidebar and a folder's files.

use crate::core;
use crate::i18n::{gettext, gettext_noop};
use adw::prelude::*;
use foldav_core::File;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

mod ops;

/// Where to support development, listed in About (and in the README), as in Todav.
const DONATIONS: [(&str, &str); 3] = [
    (
        gettext_noop("Support on Ko-fi"),
        "https://ko-fi.com/N5E229GJPZ",
    ),
    (
        gettext_noop("Sponsor on GitHub"),
        "https://github.com/sponsors/SelfRef",
    ),
    (
        gettext_noop("Donate on Liberapay"),
        "https://liberapay.com/SelfRef/donate",
    ),
];

struct Ui {
    window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    stack: gtk::Stack,
    /// Welcome page: the steps adding the first account (`add_account`).
    welcome: adw::Bin,
    add_account: RefCell<Option<Rc<crate::add_account::AddAccount>>>,
    places: gtk::ListBox,
    /// The account of each row of `places`.
    place_accounts: RefCell<Vec<i64>>,
    folder_title: adw::WindowTitle,
    files_stack: gtk::Stack,
    files: gtk::ListBox,
    error: adw::StatusPage,
    update_banner: adw::Banner,
    /// The folder shown: its account and path (relative to the Files root).
    at: RefCell<Option<(i64, String)>>,
    /// Bumped with every load; a load only shows while its number is current.
    load: Cell<u64>,
    /// What the list shows, in the order of its rows.
    listing: RefCell<Vec<File>>,
    /// Cut or copied files, to paste (`ops`).
    clipboard: RefCell<Option<ops::Clipboard>>,
    /// The menu of a right click on the list.
    file_menu: gio::MenuModel,
}

thread_local! {
    static UI: RefCell<Option<Rc<Ui>>> = const { RefCell::new(None) };
}

fn ui() -> Option<Rc<Ui>> {
    UI.with(|u| u.borrow().clone())
}

/// A view compiled by `build.rs` from `ui/<name>.blp`.
macro_rules! view {
    ($name:literal) => {
        gtk::Builder::from_string(include_str!(concat!(env!("OUT_DIR"), "/", $name, ".ui")))
    };
}
pub(crate) use view;

pub fn get<T: IsA<glib::Object>>(b: &gtk::Builder, id: &str) -> T {
    b.object(id).unwrap_or_else(|| panic!("no `{id}` in view"))
}

pub fn note(text: &str) -> adw::Toast {
    let t = adw::Toast::new(text);
    t.set_use_markup(false);
    t
}

pub fn toast(msg: &str) {
    if let Some(ui) = ui() {
        ui.toasts.add_toast(note(msg));
    }
}

pub fn setup_actions(app: &adw::Application) {
    let quit = gio::SimpleAction::new("quit", None);
    quit.connect_activate(glib::clone!(
        #[weak]
        app,
        move |_, _| app.quit()
    ));
    let restart = gio::SimpleAction::new("restart", None);
    restart.connect_activate(glib::clone!(
        #[weak]
        app,
        move |_, _| crate::update::restart(&app)
    ));
    let shortcuts = gio::SimpleAction::new("shortcuts", None);
    shortcuts.connect_activate(|_, _| {
        if let Some(ui) = ui() {
            get::<adw::Dialog>(&view!("shortcuts"), "dialog").present(Some(&ui.window));
        }
    });
    // Filled from the metainfo file (bundled, see icons/icons.gresource.xml).
    let about = gio::SimpleAction::new("about", None);
    about.connect_activate(|_, _| {
        let Some(ui) = ui() else { return };
        let d = adw::AboutDialog::from_appdata(
            "/dev/aperte/Foldav/metainfo.xml",
            Some(env!("CARGO_PKG_VERSION")),
        );
        d.set_copyright("© 2026 SelfRef");
        about_support(&d);
        d.present(Some(&ui.window));
    });
    for a in [&quit, &restart, &shortcuts, &about] {
        app.add_action(a);
    }
    // Listed in ui/shortcuts.blp.
    app.set_accels_for_action("app.quit", &["<Control>q"]);
    app.set_accels_for_action("app.shortcuts", &["<Control>question"]);
    app.set_accels_for_action("win.preferences", &["<Control>comma"]);
    app.set_accels_for_action("win.up", &["<Alt>Up"]);
    app.set_accels_for_action("win.reload", &["F5", "<Control>r"]);
    app.set_accels_for_action("win.toggle-sidebar", &["F9"]);
    ops::accels(app);
}

/// Turns About's Details page into Support: a note and the donation links instead of the
/// description, with the website left on the main page. AdwAboutDialog has no API for that, so
/// this finds its rows (the same in libadwaita 1.8 to 1.10); if they move, About keeps a Details
/// page with the note and the links.
fn about_support(d: &adw::AboutDialog) {
    d.set_comments(&glib::markup_escape_text(&gettext(
        "Foldav is developed in my free time. If it helps you with your files, you can support \
         its development.",
    )));
    let titles: Vec<String> = DONATIONS.iter().map(|(t, _)| gettext(t)).collect();
    for ((_, uri), title) in DONATIONS.iter().zip(&titles) {
        d.add_link(title, uri);
    }
    // Its content: the dialog itself has no children until presented.
    let Some(content) = d.child() else { return };
    let widgets = descendants(&content);
    // The row that opens the page, followed by the website's own (hidden with a Details page).
    let Some(row) = widgets
        .iter()
        .filter_map(|w| w.downcast_ref::<adw::ActionRow>())
        .find(|r| {
            r.action_name().as_deref() == Some("navigation.push")
                && r.action_target_value()
                    .is_some_and(|v| v.str() == Some("details"))
        })
    else {
        return;
    };
    let Some(page) = widgets
        .iter()
        .find_map(|w| w.downcast_ref::<adw::NavigationView>())
        .and_then(|v| v.find_page("details"))
    else {
        return;
    };
    // Translators: the About row that opens the page with the donation links.
    row.set_title(&gettext("_Support"));
    // Translators: title of About's page with the donation links.
    page.set_title(&gettext("Support"));
    if let Some(website) = row.next_sibling() {
        website.set_visible(true);
    }
    for r in descendants(page.upcast_ref()) {
        if let Some(r) = r.downcast_ref::<adw::PreferencesRow>()
            && !titles.iter().any(|t| *t == r.title())
        {
            r.set_visible(false);
        }
    }
}

/// `w` and every widget inside it.
fn descendants(w: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut all = vec![w.clone()];
    let mut i = 0;
    while let Some(w) = all.get(i).cloned() {
        let mut child = w.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            all.push(c);
        }
        i += 1;
    }
    all
}

pub fn present(app: &adw::Application) {
    let ui = ui().unwrap_or_else(|| {
        let ui = build(app);
        UI.with(|u| *u.borrow_mut() = Some(ui.clone()));
        ui
    });
    if core().accounts().is_empty() {
        if ui.add_account.borrow().is_none() {
            welcome(&ui);
        }
    } else {
        ui.stack.set_visible_child_name("main");
        refresh_places(&ui);
    }
    ui.window.present();
}

/// The welcome page, made anew (GNOME Online Accounts may have changed); the window shows the
/// main page once the first account is added.
fn welcome(ui: &Rc<Ui>) {
    let weak = Rc::downgrade(ui);
    let add = crate::add_account::new(true, move |id| {
        let Some(ui) = weak.upgrade() else { return };
        ui.add_account.take();
        ui.welcome.set_child(gtk::Widget::NONE);
        ui.stack.set_visible_child_name("main");
        refresh_places(&ui);
        open(&ui, id, String::new());
    });
    ui.welcome.set_child(Some(&add.root));
    *ui.add_account.borrow_mut() = Some(add);
    ui.stack.set_visible_child_name("welcome");
}

/// No account left: back to the welcome page.
pub fn show_welcome() {
    if let Some(ui) = ui() {
        ui.at.take();
        refresh_places(&ui);
        welcome(&ui);
    }
}

/// Accounts came, went, were switched on or off or renamed (in Preferences): the sidebar shows
/// them as they are now.
pub fn account_changed() {
    if let Some(ui) = ui() {
        refresh_places(&ui);
    }
}

/// Account `id` was added in Preferences: its files show.
pub fn account_added(id: i64) {
    if let Some(ui) = ui() {
        refresh_places(&ui);
        open(&ui, id, String::new());
    }
}

pub fn update_banner() {
    if let Some(ui) = ui() {
        ui.update_banner.set_revealed(true);
    }
}

/// Restores the last window size; saves it when the window closes. GTK keeps the unmaximized
/// size in default-width/height.
fn restore_size(window: &adw::ApplicationWindow) {
    let size = core().setting("window_size".into()).unwrap_or_default();
    if let Some((w, h)) = size.split_once('x')
        && let (Ok(w), Ok(h)) = (w.parse(), h.parse())
    {
        window.set_default_size(w, h);
    }
    window.set_maximized(core().setting("window_maximized".into()).as_deref() == Some("true"));
    window.connect_close_request(|w| {
        let (width, height) = w.default_size();
        let _ = core().set_setting("window_size".into(), format!("{width}x{height}"));
        let _ = core().set_setting("window_maximized".into(), w.is_maximized().to_string());
        // Upload what changed, before the app quits.
        let _ = core().sync_settings();
        glib::Propagation::Proceed
    });
}

fn build(app: &adw::Application) -> Rc<Ui> {
    let b = view!("window");
    let window: adw::ApplicationWindow = get(&b, "window");
    window.set_application(Some(app));
    // A development build (debug: `cargo run`, `make sandbox`; packages are release builds) has
    // GNOME's striped header bars, so it is not taken for the installed app.
    if cfg!(debug_assertions) {
        window.add_css_class("devel");
    }
    if crate::sandbox() {
        window.set_title(Some("Foldav (sandbox)"));
    }
    restore_size(&window);

    let ui = Rc::new(Ui {
        window,
        toasts: get(&b, "toasts"),
        stack: get(&b, "stack"),
        welcome: get(&b, "welcome"),
        add_account: RefCell::new(None),
        places: get(&b, "places"),
        place_accounts: RefCell::new(Vec::new()),
        folder_title: get(&b, "folder_title"),
        files_stack: get(&b, "files_stack"),
        files: get(&b, "files"),
        error: get(&b, "error"),
        update_banner: get(&b, "update_banner"),
        at: RefCell::new(None),
        load: Cell::new(0),
        listing: RefCell::new(Vec::new()),
        clipboard: RefCell::new(None),
        file_menu: get(&b, "file_menu"),
    });
    ui.update_banner.set_revealed(crate::update::updated());
    connect(&ui, get(&b, "split"));
    ops::connect(&ui);
    ui
}

type Handler = Box<dyn Fn(&Rc<Ui>)>;

/// The window's actions and the sidebar's selection.
fn connect(ui: &Rc<Ui>, split: adw::OverlaySplitView) {
    let action = |name: &str, f: Handler| {
        let a = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(ui);
        a.connect_activate(move |_, _| {
            if let Some(ui) = weak.upgrade() {
                f(&ui);
            }
        });
        ui.window.add_action(&a);
        a
    };
    action(
        "preferences",
        Box::new(|ui| crate::settings::present(&ui.window)),
    );
    action(
        "toggle-sidebar",
        Box::new(move |_| split.set_show_sidebar(!split.shows_sidebar())),
    );
    action("reload", Box::new(reload));
    let up = action(
        "up",
        Box::new(|ui| {
            let at = ui.at.borrow().clone();
            if let Some((id, path)) = at {
                open(ui, id, parent(&path).into());
            }
        }),
    );
    // Only below the Files root (`open`).
    up.set_enabled(false);

    let weak = Rc::downgrade(ui);
    ui.places.connect_row_activated(move |_, row| {
        let Some(ui) = weak.upgrade() else { return };
        let id = ui
            .place_accounts
            .borrow()
            .get(row.index() as usize)
            .copied();
        if let Some(id) = id {
            open(&ui, id, String::new());
        }
    });
    let weak = Rc::downgrade(ui);
    ui.files.connect_row_activated(move |_, row| {
        let Some(ui) = weak.upgrade() else { return };
        // A folder opens; a file does nothing yet.
        let (file, at) = (
            ui.listing.borrow().get(row.index() as usize).cloned(),
            ui.at.borrow().clone(),
        );
        if let (Some(f), Some((id, _))) = (file, at)
            && f.folder
        {
            open(&ui, id, f.path);
        }
    });
}

/// The folder above `path` ("" above a top-level one).
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(p, _)| p)
}

/// The sidebar's rows: the accounts that are switched on. An account switched off or removed
/// while its folder is shown takes the folder with it.
fn refresh_places(ui: &Rc<Ui>) {
    while let Some(row) = ui.places.row_at_index(0) {
        ui.places.remove(&row);
    }
    let accounts: Vec<_> = core()
        .accounts()
        .into_iter()
        .filter(|a| a.enabled)
        .collect();
    for a in &accounts {
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&crate::account_name(a)))
            .activatable(true)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name(crate::settings::account_icon(
            a,
        )));
        ui.places.append(&row);
    }
    *ui.place_accounts.borrow_mut() = accounts.iter().map(|a| a.id).collect();
    let current = ui.at.borrow().as_ref().map(|(id, _)| *id);
    match current {
        Some(id) if accounts.iter().any(|a| a.id == id) => select_place(ui, id),
        _ => match accounts.first() {
            Some(a) => open(ui, a.id, String::new()),
            None => {
                ui.at.take();
                ui.files_stack.set_visible_child_name("empty");
                ui.folder_title.set_title("Foldav");
                ui.folder_title.set_subtitle("");
            }
        },
    }
}

fn select_place(ui: &Ui, id: i64) {
    let at = ui.place_accounts.borrow().iter().position(|a| *a == id);
    ui.places
        .select_row(at.and_then(|i| ui.places.row_at_index(i as i32)).as_ref());
}

fn reload(ui: &Rc<Ui>) {
    let at = ui.at.borrow().clone();
    if let Some((id, path)) = at {
        open(ui, id, path);
    }
}

/// Show folder `path` of account `id`.
fn open(ui: &Rc<Ui>, id: i64, path: String) {
    let Some(account) = core().account(id) else {
        return;
    };
    *ui.at.borrow_mut() = Some((id, path.clone()));
    ops::update_actions(ui);
    select_place(ui, id);
    ui.window
        .lookup_action("up")
        .and_downcast::<gio::SimpleAction>()
        .inspect(|a| a.set_enabled(!path.is_empty()));
    ui.folder_title.set_title(&match path.rsplit('/').next() {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => crate::account_name(&account),
    });
    ui.folder_title.set_subtitle(&if path.is_empty() {
        String::new()
    } else {
        format!("{}/{path}", crate::account_name(&account))
    });
    if !core().connected(id) {
        return show_error(
            ui,
            &gettext("Not connected. Sign in again in Preferences → Accounts."),
        );
    }
    let load = ui.load.get() + 1;
    ui.load.set(load);
    ui.files_stack.set_visible_child_name("loading");
    let weak = Rc::downgrade(ui);
    glib::spawn_future_local(async move {
        let p = path.clone();
        let r = gio::spawn_blocking(move || core().list(id, &p)).await;
        let Some(ui) = weak.upgrade().filter(|u| u.load.get() == load) else {
            return;
        };
        match r {
            Ok(Ok(files)) => show(&ui, &files),
            Ok(Err(e)) => show_error(&ui, &e.to_string()),
            Err(_) => show_error(&ui, &gettext("Loading failed")),
        }
    });
}

fn show_error(ui: &Ui, text: &str) {
    ui.error.set_description(Some(text));
    ui.files_stack.set_visible_child_name("error");
}

fn show(ui: &Ui, files: &[File]) {
    ui.files.remove_all();
    for f in files {
        ui.files.append(&file_row(f));
    }
    *ui.listing.borrow_mut() = files.to_vec();
    ui.files_stack
        .set_visible_child_name(if files.is_empty() { "empty" } else { "files" });
}

/// A folder's row opens it (`connect`).
fn file_row(f: &File) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(glib::markup_escape_text(&f.name))
        .build();
    let when = glib::DateTime::from_unix_local(f.modified)
        .and_then(|d| d.format("%x"))
        .map(|d| d.to_string())
        .unwrap_or_default();
    row.set_subtitle(&match f.size {
        Some(size) => format!("{} · {when}", glib::format_size(size)),
        None => when,
    });
    let icon = if f.folder {
        gio::ThemedIcon::new("folder-symbolic").upcast()
    } else {
        gio::content_type_get_symbolic_icon(if f.mime.is_empty() {
            "application/octet-stream"
        } else {
            &f.mime
        })
    };
    row.add_prefix(&gtk::Image::from_gicon(&icon));
    if f.favorite {
        row.add_suffix(&gtk::Image::from_icon_name("starred-symbolic"));
    }
    if f.folder {
        row.set_activatable(true);
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    }
    row
}

// --- signing in --------------------------------------------------------------

/// What every way of signing in says when the server field is empty.
pub fn no_server() -> String {
    gettext("Enter the Nextcloud server first")
}

/// Full URL from a scheme picker and host entry; empty stays empty, a typed scheme wins over the picker.
pub fn url_text((scheme, row): &(gtk::DropDown, adw::EntryRow)) -> String {
    let host = row.text().trim().to_string();
    if host.is_empty() || host.contains("://") {
        return host;
    }
    let scheme = if scheme.selected() == 0 {
        "https"
    } else {
        "http"
    };
    format!("{scheme}://{host}")
}

/// Bumped on every sign-in and on cancel; a sign-in only goes on while its number is current.
static LOGIN_ATTEMPT: AtomicU64 = AtomicU64::new(0);

/// Stop a running `browser_login`; it then returns `Ok(None)`.
pub fn cancel_login() {
    LOGIN_ATTEMPT.fetch_add(1, Ordering::SeqCst);
}

/// Login Flow v2: open the browser (or copy the link) and wait until the user approves.
/// `Ok(None)`: cancelled, or another sign-in started.
pub async fn browser_login(
    server: String,
    copy: bool,
) -> Result<Option<foldav_core::Credentials>, String> {
    let attempt = LOGIN_ATTEMPT.fetch_add(1, Ordering::SeqCst) + 1;
    let current = move || LOGIN_ATTEMPT.load(Ordering::SeqCst) == attempt;
    let flow = gio::spawn_blocking(move || foldav_core::login_flow_start(server))
        .await
        .map_err(|_| gettext("Cannot start login"))?
        .map_err(|e| gettext("Cannot start login: {error}").replace("{error}", &e.to_string()))?;
    if !current() {
        return Ok(None);
    }
    if copy {
        if let Some(d) = gtk::gdk::Display::default() {
            d.clipboard().set_text(&flow.login_url);
        }
        toast(&gettext("Login link copied to clipboard"));
    } else {
        let parent = ui().map(|u| u.window.clone());
        gtk::UriLauncher::new(&flow.login_url).launch(
            parent.as_ref(),
            gio::Cancellable::NONE,
            |_| {},
        );
    }
    let flow = std::sync::Arc::new(flow);
    for _ in 0..600 {
        glib::timeout_future_seconds(2).await;
        if !current() {
            return Ok(None);
        }
        let f = flow.clone();
        match gio::spawn_blocking(move || foldav_core::login_flow_poll(&f)).await {
            Ok(Ok(None)) => continue,
            Ok(Ok(Some(c))) => return Ok(current().then_some(c)),
            Ok(Err(e)) => {
                return Err(gettext("Login failed: {error}").replace("{error}", &e.to_string()));
            }
            Err(_) => return Err(gettext("Login failed")),
        }
    }
    Err(gettext("Login timed out"))
}

/// Ask for a login name (`login`, when signing in again) and (app) password for `server` and
/// check them there; `done` gets the credentials once they work.
pub fn password_login(
    parent: &impl IsA<gtk::Widget>,
    server: String,
    login: Option<String>,
    done: impl Fn(foldav_core::Credentials) + 'static,
) {
    let b = view!("password-login");
    let dialog: adw::Dialog = get(&b, "dialog");
    let user: adw::EntryRow = get(&b, "user");
    let password: adw::PasswordEntryRow = get(&b, "password");
    let sign_in: gtk::Button = get(&b, "sign_in");
    let error: adw::Banner = get(&b, "error");
    if let Some(login) = login {
        user.set_text(&login);
    }
    let filled = {
        let (user, password, sign_in) = (user.clone(), password.clone(), sign_in.clone());
        let error = error.clone();
        move || {
            error.set_revealed(false);
            sign_in.set_sensitive(!user.text().trim().is_empty() && !password.text().is_empty())
        }
    };
    let f = filled.clone();
    user.connect_changed(move |_| f());
    password.connect_changed(move |_| filled());
    let d = dialog.clone();
    get::<gtk::Button>(&b, "cancel").connect_clicked(move |_| {
        d.close();
    });
    let (d, done) = (dialog.clone(), Rc::new(done));
    sign_in.connect_clicked(move |button| {
        let (login, pw) = (user.text().trim().to_string(), password.text().to_string());
        let server = server.clone();
        button.set_sensitive(false);
        let (d, done, error, button) = (d.clone(), done.clone(), error.clone(), button.clone());
        glib::spawn_future_local(async move {
            let r =
                gio::spawn_blocking(move || foldav_core::password_login(server, login, pw)).await;
            let msg = match r {
                Ok(Ok(c)) => {
                    d.close();
                    return done(c);
                }
                Ok(Err(foldav_core::Error::Auth)) => gettext("Wrong login name or password"),
                Ok(Err(e)) => gettext("Sign-in failed: {error}").replace("{error}", &e.to_string()),
                Err(_) => gettext("Sign-in failed"),
            };
            error.set_title(&msg);
            error.set_revealed(true);
            button.set_sensitive(true);
        });
    });
    dialog.present(Some(parent));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parent_of_a_path() {
        assert_eq!(parent("Photos/2026"), "Photos");
        assert_eq!(parent("Photos"), "");
        assert_eq!(parent(""), "");
    }
}
