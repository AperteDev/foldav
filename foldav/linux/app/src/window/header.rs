//! The header, as Nautilus': back and forward, the path bar, the view switcher and the menus;
//! and the folder above. Its actions are made here.

use super::views::Mode;
use super::{Ui, action, nav, sidebar, views};
use crate::core;
use crate::i18n::gettext;
use adw::prelude::*;
use gtk::{gio, glib};
use std::rc::Rc;
use std::time::Duration;

pub struct Header {
    pathbar: gtk::Box,
    scroll: gtk::ScrolledWindow,
    toggle: adw::ToggleGroup,
}

impl Header {
    pub fn new(b: &gtk::Builder) -> Header {
        Header {
            pathbar: super::get(b, "pathbar"),
            scroll: super::get(b, "pathbar_scroll"),
            toggle: super::get(b, "view_toggle"),
        }
    }
}

/// The actions of the header and its menus, and the view the user left off with.
pub fn connect(ui: &Rc<Ui>) {
    action(ui, "up", nav::up);
    action(ui, "back", nav::back);
    action(ui, "forward", nav::forward);
    action(ui, "reload", nav::reload);
    action(ui, "bookmark-add", |ui| bookmark(ui, true));
    action(ui, "bookmark-remove", |ui| bookmark(ui, false));
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
    app.set_accels_for_action("win.view-icons", &["<Control>1"]);
    app.set_accels_for_action("win.view-list", &["<Control>2"]);
    app.set_accels_for_action("win.view-columns", &["<Control>3"]);
    app.set_accels_for_action("win.show-hidden", &["<Control>h"]);
}

/// Bookmark (or stop bookmarking) the folder shown.
fn bookmark(ui: &Rc<Ui>, add: bool) {
    let Some((id, path)) = ui.at.borrow().clone() else {
        return;
    };
    if add {
        let _ = core().add_bookmark(id, &path);
    } else if let Some(b) = core().bookmarks(id).into_iter().find(|b| b.path == path) {
        let _ = core().remove_bookmark(b.id);
    }
    sidebar::refresh(ui);
    update(ui);
}

/// The path bar and what depends on the folder shown: back, forward, up, bookmarking.
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
    let bookmarked = at
        .as_ref()
        .is_some_and(|(id, p)| core().bookmarks(*id).iter().any(|b| b.path == *p));
    set("bookmark-add", at.is_some() && !bookmarked);
    set("bookmark-remove", bookmarked);

    let bar = &ui.header.pathbar;
    while let Some(child) = bar.first_child() {
        bar.remove(&child);
    }
    let Some((id, path)) = at else { return };
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
    button.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            nav::open(&ui, id, path.clone());
        }
    });
    ui.header.pathbar.append(&button);
}
