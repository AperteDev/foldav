//! The header, as Nautilus': back and forward, the path bar, the view switcher and the menus;
//! and the folder above. Its actions are made here.

use super::nav::Go;
use super::views::Mode;
use super::{Ui, action, nav, sidebar, views};
use crate::core;
use crate::i18n::gettext;
use adw::prelude::*;
use foldav_core::places;
use gtk::{gdk, gio, glib};
use std::rc::Rc;
use std::time::Duration;

pub struct Header {
    pathbar: gtk::Box,
    scroll: gtk::ScrolledWindow,
    toggle: adw::ToggleGroup,
    search_bar: gtk::SearchBar,
    search_entry: gtk::SearchEntry,
}

impl Header {
    pub fn new(b: &gtk::Builder) -> Header {
        Header {
            pathbar: super::get(b, "pathbar"),
            scroll: super::get(b, "pathbar_scroll"),
            toggle: super::get(b, "view_toggle"),
            search_bar: super::get(b, "search_bar"),
            search_entry: super::get(b, "search_entry"),
        }
    }
}

/// Search the folder shown and below it as the query is typed; the results are a place, so
/// back leaves them. An empty query, or Escape, leaves the search.
fn connect_search(ui: &Rc<Ui>) {
    let h = &ui.header;
    h.search_bar.connect_entry(&h.search_entry);
    action(ui, "search", |ui| {
        let on = !ui.header.search_bar.is_search_mode();
        ui.header.search_bar.set_search_mode(on);
    });
    let weak = Rc::downgrade(ui);
    h.search_entry.connect_search_changed(move |entry| {
        let Some(ui) = weak.upgrade() else { return };
        let (query, at) = (entry.text().trim().to_string(), ui.at.borrow().clone());
        let Some((id, path)) = at else { return };
        let searching = places::search_of(&path).map(|(folder, _)| folder.to_string());
        if query.is_empty() {
            if searching.is_some() {
                nav::back(&ui);
            }
            return;
        }
        // From a place that is no search, all the files; from a search, the folder it was in.
        let how = if searching.is_some() {
            Go::Keep
        } else {
            Go::Push
        };
        let folder = match searching {
            Some(folder) => folder,
            None if places::is_place(&path) => String::new(),
            None => path,
        };
        nav::go(&ui, id, places::search_place(&folder, &query), how);
    });
    let weak = Rc::downgrade(ui);
    h.search_entry.connect_stop_search(move |_| {
        let Some(ui) = weak.upgrade() else { return };
        ui.header.search_bar.set_search_mode(false);
    });
    // Closed: the search is left, back to the folder.
    let weak = Rc::downgrade(ui);
    h.search_bar.connect_search_mode_enabled_notify(move |bar| {
        let Some(ui) = weak.upgrade() else { return };
        if !bar.is_search_mode() {
            ui.header.search_entry.set_text("");
        }
    });
}

/// The actions of the header and its menus, and the view the user left off with.
pub fn connect(ui: &Rc<Ui>) {
    action(ui, "up", nav::up);
    action(ui, "back", nav::back);
    action(ui, "forward", nav::forward);
    action(ui, "reload", nav::reload);
    connect_search(ui);
    // The views, by key too (Ctrl+1 to 3): the switcher follows.
    action(ui, "view-icons", |ui| {
        ui.header.toggle.set_active_name(Some("icons"))
    });
    action(ui, "view-list", |ui| {
        ui.header.toggle.set_active_name(Some("list"))
    });
    action(ui, "view-columns", |ui| {
        ui.header.toggle.set_active_name(Some("columns"))
    });

    // The columns' icon: the theme's own where it has one, else ours (the Files extension
    // "My Computer" does the same, and ours is its icon), looked at again when the theme changes.
    if let Some(display) = gdk::Display::default() {
        let theme = gtk::IconTheme::for_display(&display);
        let toggle = ui.header.toggle.downgrade();
        let set = move |theme: &gtk::IconTheme| {
            let Some(columns) = toggle.upgrade().and_then(|t| t.toggle_by_name("columns")) else {
                return;
            };
            columns.set_icon_name(Some(if theme.has_icon("view-column-symbolic") {
                "view-column-symbolic"
            } else {
                "foldav-view-columns-symbolic"
            }));
        };
        set(&theme);
        theme.connect_changed(set);
    }

    let weak = Rc::downgrade(ui);
    ui.header.toggle.connect_active_name_notify(move |g| {
        if let Some(ui) = weak.upgrade() {
            let mode = Mode::from_name(g.active_name().as_deref().unwrap_or_default());
            views::set_mode(&ui, mode);
        }
    });
    let mode = Mode::from_name(&core().setting("view_mode".into()).unwrap_or_default());
    ui.header.toggle.set_active_name(Some(mode.name()));
    views::set_mode(ui, mode);

    // Hidden files: a check in the view menu.
    let hidden = core().setting("show_hidden".into()).as_deref() == Some("true");
    ui.hidden.set(hidden);
    let show = gio::SimpleAction::new_stateful("show-hidden", None, &hidden.to_variant());
    let weak = Rc::downgrade(ui);
    show.connect_activate(move |a, _| {
        let Some(ui) = weak.upgrade() else { return };
        let on = !a.state().and_then(|s| s.get::<bool>()).unwrap_or(false);
        a.set_state(&on.to_variant());
        ui.hidden.set(on);
        let _ = core().set_setting("show_hidden".into(), on.to_string());
        nav::show(&ui);
        sidebar::refresh_tree(&ui);
    });
    ui.window.add_action(&show);
}

pub fn accels(app: &adw::Application) {
    app.set_accels_for_action("win.up", &["<Alt>Up"]);
    app.set_accels_for_action("win.back", &["<Alt>Left"]);
    app.set_accels_for_action("win.forward", &["<Alt>Right"]);
    app.set_accels_for_action("win.reload", &["F5", "<Control>r"]);
    app.set_accels_for_action("win.search", &["<Control>f"]);
    app.set_accels_for_action("win.view-icons", &["<Control>1"]);
    app.set_accels_for_action("win.view-list", &["<Control>2"]);
    app.set_accels_for_action("win.view-columns", &["<Control>3"]);
    app.set_accels_for_action("win.show-hidden", &["<Control>h"]);
}

/// The path bar and what depends on the folder shown: back, forward, up, favorites.
pub fn update(ui: &Rc<Ui>) {
    let at = ui.at.borrow().clone();
    let set = |name: &str, on: bool| {
        ui.window
            .lookup_action(name)
            .and_downcast::<gio::SimpleAction>()
            .inspect(|a| a.set_enabled(on));
    };
    set("back", nav::can_back(ui));
    set("forward", nav::can_forward(ui));
    set("up", at.as_ref().is_some_and(|(_, p)| !p.is_empty()));
    super::favorites::update_actions(ui);

    let bar = &ui.header.pathbar;
    while let Some(child) = bar.first_child() {
        bar.remove(&child);
    }
    let Some((id, path)) = at else { return };
    // A place is one segment after the root, with its own title.
    if foldav_core::places::is_place(&path) {
        segment(ui, &gettext("Home"), id, "", false);
        let separator = gtk::Label::new(Some("/"));
        separator.add_css_class("path-separator");
        bar.append(&separator);
        let title = super::sidebar::place_title(ui, id, &path).unwrap_or_default();
        segment(ui, &title, id, &path, true);
        return;
    }
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    segment(ui, &gettext("Home"), id, "", segments.is_empty());
    let mut at_path = String::new();
    for (i, name) in segments.iter().enumerate() {
        let separator = gtk::Label::new(Some("/"));
        separator.add_css_class("path-separator");
        bar.append(&separator);
        if !at_path.is_empty() {
            at_path.push('/');
        }
        at_path.push_str(name);
        segment(ui, name, id, &at_path, i + 1 == segments.len());
    }
    // The end of a long path is what shows.
    let scroll = ui.header.scroll.clone();
    glib::timeout_add_local_once(Duration::from_millis(30), move || {
        let a = scroll.hadjustment();
        a.set_value(a.upper() - a.page_size());
    });
}

fn segment(ui: &Rc<Ui>, name: &str, id: i64, path: &str, current: bool) {
    let button = gtk::Button::builder()
        .label(name)
        .css_classes(["flat", "path-segment"])
        .build();
    if current {
        button.add_css_class("current");
    }
    let (weak, path) = (Rc::downgrade(ui), path.to_string());
    let p = path.clone();
    button.add_controller(super::dnd::target(&weak, true, move |_| {
        Some((id, p.clone()))
    }));
    button.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            nav::open(&ui, id, path.clone());
        }
    });
    ui.header.pathbar.append(&button);
}
