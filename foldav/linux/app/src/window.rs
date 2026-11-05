//! Main window: the welcome page (sign-in), and the files as Nautilus shows them: the sidebar
//! (accounts, favorites, tree), the header and the views of a folder. Its parts are in
//! `window/`: `nav` (where the window is, the history, what was loaded), `sidebar`, `header`,
//! `views`, `ops` (what is done to files) and `transfers` (the long actions running).

use crate::core;
use crate::i18n::{gettext, gettext_noop};
use adw::prelude::*;
use foldav_core::File;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

mod columns;
mod dnd;
mod favorites;
mod header;
mod nav;
mod ops;
mod share;
mod sidebar;
mod tags;
mod thumbs;
mod transfers;
mod versions;
mod views;

/// The path bar and its segments, as Nautilus'; the dense tree (`.dense`, as Notav's); the row
/// the star of a favorite; the columns' dividers.
const CSS: &str = "
.pathbar { background-color: alpha(currentColor, 0.08); border-radius: 8px; min-height: 34px;
  padding: 0 2px 0 4px; }
.pathbar button { min-height: 30px; }
.pathbar button.path-segment { padding: 0 8px; border-radius: 6px; }
.pathbar menubutton > button { min-width: 30px; padding: 0; }
.pathbar button.path-segment.current { font-weight: bold; }
.pathbar .path-separator { opacity: 0.5; }
.navigation-sidebar.dense > row { min-height: 28px; margin-bottom: 0; border-radius: 6px; }
.tree-arrow { min-width: 20px; min-height: 20px; padding: 0; }
.favorite-star { color: #f5c211; }
/* The long actions at the foot of the sidebar: rows as the tree's. */
.transfers menubutton > button { min-height: 28px; padding: 4px 8px; border-radius: 6px; }
.transfers label { font-weight: normal; }
/* The folder files would be dropped into. */
.drop-target { background-color: alpha(@accent_bg_color, 0.3); border-radius: 8px; }
/* The window buttons are in a plain toolbar row, not a header bar: its buttons' own background
   must not show behind their circles. */
.toolbar windowcontrols button, .toolbar windowcontrols button:hover,
.toolbar windowcontrols button:active { background: none; box-shadow: none; }
.toolbar windowcontrols button:hover image { background-color: alpha(currentColor, 0.2); }
.toolbar windowcontrols button:active image { background-color: alpha(currentColor, 0.3); }
/* The columns, as the Files extension \"My Computer\" draws them: rows 36px high with a 9px
   radius, touching, the selection grey (the path through the columns) and in the column the
   user works in the accent colour; a divider at the edge of each column, the handle that
   resizes it; the preview column padded. */
.miller-handle { border-right: 1px solid alpha(currentColor, 0.12); }
.miller-preview { padding: 12px; }
listview.miller-list { padding: 6px; }
listview.miller-list > row { padding: 0; border-radius: 9px; }
listview.miller-list > row > .miller-row { padding: 6px 8px; border-radius: 9px; }
listview.miller-list > row:hover > .miller-row { background-color: alpha(currentColor, 0.05); }
/* The column the user works in keeps the selection of the other views (the theme's own); the
   others show the way to it in grey. */
listview.miller-list:not(.miller-active) > row:selected { background: none; }
listview.miller-list:not(.miller-active) > row:selected > .miller-row { background-color: alpha(currentColor, 0.1); }
.miller-list image { -gtk-icon-style: regular; }
/* The files on the window's own background, as in Files, not the view's darker one; the icons'
   tiles rounded and close, as there. */
.files-view, .files-view listview { background: none; }
gridview.files-view > child { border-radius: 12px; margin: 2px; padding: 0; }
";

/// How often (seconds) the folder shown is looked at for changes made elsewhere.
const POLL: u32 = 30;

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

/// The files of folders, by account and path.
type Dirs = HashMap<(i64, String), Rc<Vec<File>>>;

struct Ui {
    window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    stack: gtk::Stack,
    split: adw::OverlaySplitView,
    /// Welcome page: the steps adding the first account (`add_account`).
    welcome: adw::Bin,
    add_account: RefCell<Option<Rc<crate::add_account::AddAccount>>>,
    sidebar: sidebar::Sidebar,
    header: header::Header,
    views: views::Views,
    /// The long actions running (`transfers`).
    transfers: transfers::Transfers,
    files_stack: gtk::Stack,
    error: adw::StatusPage,
    update_banner: adw::Banner,
    /// The folder shown: its account and path (relative to the Files root).
    at: RefCell<Option<(i64, String)>>,
    /// Bumped with every load; a load only shows while its number is current.
    load: Cell<u64>,
    /// The folders loaded, by account and path: for the views, the columns and the tree.
    dirs: RefCell<Dirs>,
    /// The folders open in the sidebar's tree.
    expanded: RefCell<HashSet<(i64, String)>>,
    history: RefCell<nav::History>,
    /// What is starred, by account (`favorites.rs`); the number changes with it, for what is
    /// made from it (the columns) to be made again.
    favorites: RefCell<HashMap<i64, Vec<foldav_core::Favorite>>>,
    fav_version: Cell<u64>,
    /// What each account's server offers, its tags and its quota (`sidebar::load_server`).
    server: RefCell<HashMap<i64, sidebar::Server>>,
    /// Hidden files show (the device-only `show_hidden` setting).
    hidden: Cell<bool>,
    /// Cut or copied files, to paste (`ops`).
    clipboard: RefCell<Option<ops::Clipboard>>,
    /// The menu of a right click on the files.
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

/// A window action running `f`. The keys are the text field's while one has the focus (a name
/// being typed).
fn action(ui: &Rc<Ui>, name: &str, f: fn(&Rc<Ui>)) -> gio::SimpleAction {
    let a = gio::SimpleAction::new(name, None);
    let weak = Rc::downgrade(ui);
    a.connect_activate(move |_, _| {
        if let Some(ui) = weak.upgrade()
            && !GtkWindowExt::focus(&ui.window).is_some_and(|w| w.is::<gtk::Editable>())
        {
            f(&ui);
        }
    });
    ui.window.add_action(&a);
    a
}

/// A menu at `x`, `y` of `widget`.
fn popup_menu(widget: &gtk::Widget, model: &impl IsA<gio::MenuModel>, x: f64, y: f64) {
    let popover = gtk::PopoverMenu::from_model(Some(model));
    popover.set_parent(widget);
    popover.set_has_arrow(false);
    popover.set_halign(gtk::Align::Start);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    popover.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    popover.popup();
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
    app.set_accels_for_action("win.toggle-sidebar", &["F9"]);
    header::accels(app);
    ops::accels(app);
    favorites::accels(app);
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
        if ui.at.borrow().is_none() {
            nav::accounts_changed(&ui);
        }
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
        nav::open(&ui, id, String::new());
    });
    ui.welcome.set_child(Some(&add.root));
    *ui.add_account.borrow_mut() = Some(add);
    ui.stack.set_visible_child_name("welcome");
}

/// No account left: back to the welcome page.
pub fn show_welcome() {
    if let Some(ui) = ui() {
        ui.dirs.borrow_mut().clear();
        ui.expanded.borrow_mut().clear();
        nav::accounts_changed(&ui);
        welcome(&ui);
    }
}

/// Accounts came, went, were switched on or off or renamed (in Preferences): the sidebar shows
/// them as they are now.
pub fn account_changed() {
    if let Some(ui) = ui() {
        nav::accounts_changed(&ui);
    }
}

/// Account `id` was added in Preferences: its files show.
pub fn account_added(id: i64) {
    if let Some(ui) = ui() {
        nav::open(&ui, id, String::new());
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
    // The sandbox copy has GNOME's striped header bars, so it is not taken for the normal app
    // (a development build looks as Files does: the header is meant to match it).
    if crate::sandbox() {
        window.add_css_class("devel");
        window.set_title(Some("Foldav (sandbox)"));
    }
    restore_size(&window);

    let css = gtk::CssProvider::new();
    css.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(
        &WidgetExt::display(&window),
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let ui = Rc::new(Ui {
        window,
        toasts: get(&b, "toasts"),
        stack: get(&b, "stack"),
        split: get(&b, "split"),
        welcome: get(&b, "welcome"),
        add_account: RefCell::new(None),
        sidebar: sidebar::Sidebar::new(&b),
        header: header::Header::new(&b),
        views: views::Views::new(&b),
        transfers: transfers::Transfers::new(&b),
        files_stack: get(&b, "files_stack"),
        error: get(&b, "error"),
        update_banner: get(&b, "update_banner"),
        at: RefCell::new(None),
        load: Cell::new(0),
        dirs: RefCell::default(),
        expanded: RefCell::default(),
        history: RefCell::default(),
        favorites: RefCell::default(),
        fav_version: Cell::new(0),
        server: RefCell::default(),
        hidden: Cell::new(false),
        clipboard: RefCell::new(None),
        file_menu: get(&b, "file_menu"),
    });
    ui.update_banner.set_revealed(crate::update::updated());
    action(&ui, "preferences", |ui| {
        crate::settings::present(&ui.window)
    });
    action(&ui, "toggle-sidebar", |ui| {
        ui.split.set_show_sidebar(!ui.split.shows_sidebar())
    });
    // The folder shown is looked at now and then, and when the window comes to the front, for
    // what changed on the server meanwhile.
    let weak = Rc::downgrade(&ui);
    glib::timeout_add_seconds_local(POLL, move || {
        let Some(ui) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if ui.window.is_active() {
            nav::poll(&ui);
        }
        glib::ControlFlow::Continue
    });
    let weak = Rc::downgrade(&ui);
    ui.window.connect_is_active_notify(move |w| {
        if let (true, Some(ui)) = (w.is_active(), weak.upgrade()) {
            nav::poll(&ui);
        }
    });
    views::connect(&ui);
    sidebar::connect(&ui);
    ops::connect(&ui);
    ops::watch_copies(&ui);
    favorites::connect(&ui);
    header::connect(&ui);
    ui
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
