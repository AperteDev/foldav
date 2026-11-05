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
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

pub struct Header {
    pathbar: gtk::Box,
    scroll: gtk::ScrolledWindow,
    toggle: adw::ToggleGroup,
    /// The view menu's button (`options`).
    pub(super) options: gtk::MenuButton,
    /// The path bar and, in its place while shown, the path to type.
    pathbar_box: gtk::Box,
    location_box: gtk::Box,
    location_entry: gtk::Entry,
    location_close: gtk::Button,
    /// The search to type, in the path bar's place while shown.
    search_box: gtk::Box,
    search_entry: gtk::Entry,
    search_toggle: gtk::ToggleButton,
}

impl Header {
    pub fn new(b: &gtk::Builder) -> Header {
        Header {
            pathbar: super::get(b, "pathbar"),
            scroll: super::get(b, "pathbar_scroll"),
            toggle: super::get(b, "view_toggle"),
            options: super::get(b, "view_options"),
            pathbar_box: super::get(b, "pathbar_box"),
            location_box: super::get(b, "location_box"),
            location_entry: super::get(b, "location_entry"),
            location_close: super::get(b, "location_close"),
            search_box: super::get(b, "search_box"),
            search_entry: super::get(b, "search_entry"),
            search_toggle: super::get(b, "search_toggle"),
        }
    }

    /// The search field closed in favour of the path bar, and its query gone; the place shown
    /// is not changed.
    pub fn close_search(&self) {
        if self.search_box.is_visible() {
            self.search_box.set_visible(false);
            self.pathbar_box.set_visible(true);
        }
        self.search_toggle.set_active(false);
        self.search_entry.set_text("");
    }
}

/// Show the search field in the path bar's place, or leave the search: the path bar is back
/// and, if results are shown, the folder they were searched in.
fn show_search(ui: &Rc<Ui>, on: bool) {
    let h = &ui.header;
    if h.search_box.is_visible() == on {
        return;
    }
    if on {
        h.location_box.set_visible(false);
        h.pathbar_box.set_visible(false);
        h.search_box.set_visible(true);
        fade_in(h.search_box.upcast_ref());
        h.search_toggle.set_active(true);
        let entry = h.search_entry.clone();
        glib::idle_add_local_once(move || {
            entry.grab_focus();
        });
        return;
    }
    let searching = ui
        .at
        .borrow()
        .as_ref()
        .is_some_and(|(_, p)| places::search_of(p).is_some());
    h.close_search();
    fade_in(h.pathbar_box.upcast_ref());
    if searching && !ui.tabs.switching.get() {
        nav::back(ui);
    }
}

/// Search for `query` in the folder shown and below it (or, from a search, the folder it was
/// made in); the results are a place, so back leaves them. Nothing typed: nothing changes.
fn run_search(ui: &Rc<Ui>, query: &str) {
    let (query, at) = (query.trim(), ui.at.borrow().clone());
    let Some((id, path)) = at.filter(|_| !query.is_empty() && ui.header.search_box.is_visible())
    else {
        return;
    };
    let searching = places::search_of(&path).map(|(folder, _)| folder.to_string());
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
    nav::go(ui, id, places::search_place(&folder, query), how);
}

/// Search the folder shown and below it as the query is typed (a moment after the last key);
/// Escape, or the button again, leaves the search.
fn connect_search(ui: &Rc<Ui>) {
    let h = &ui.header;
    // Ctrl+F: the field, or back to it if it is open.
    action(ui, "search", |ui| {
        if ui.header.search_box.is_visible() {
            ui.header.search_entry.grab_focus();
        } else {
            show_search(ui, true);
        }
    });
    let weak = Rc::downgrade(ui);
    h.search_toggle.connect_toggled(move |t| {
        if let Some(ui) = weak.upgrade() {
            show_search(&ui, t.is_active());
        }
    });

    let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
    let weak = Rc::downgrade(ui);
    h.search_entry.connect_changed(move |entry| {
        if let Some(id) = pending.borrow_mut().take() {
            id.remove();
        }
        let Some(ui) = weak.upgrade().filter(|ui| !ui.tabs.switching.get()) else {
            return;
        };
        let (done, entry) = (pending.clone(), entry.clone());
        let id = glib::timeout_add_local_once(Duration::from_millis(250), move || {
            done.borrow_mut().take();
            run_search(&ui, &entry.text());
        });
        *pending.borrow_mut() = Some(id);
    });
    h.search_entry.connect_icon_press(|entry, position| {
        if position == gtk::EntryIconPosition::Secondary {
            entry.set_text("");
        }
    });
    let key = gtk::EventControllerKey::new();
    let weak = Rc::downgrade(ui);
    key.connect_key_pressed(move |_, keyval, _, _| {
        if keyval == gdk::Key::Escape
            && let Some(ui) = weak.upgrade()
        {
            show_search(&ui, false);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    h.search_entry.add_controller(key);
}

/// The actions of the header and its menus, and the view the user left off with.
/// The path bar turned into a field with the folder's path to edit (a click on the bar's room,
/// Ctrl+L), as Files has it; Enter goes there, Escape or leaving it brings the bar back.
fn edit_location(ui: &Rc<Ui>) {
    let h = &ui.header;
    let text = match ui.at.borrow().as_ref() {
        Some((_, p)) if !places::is_place(p) => format!("/{p}"),
        _ => "/".to_string(),
    };
    // The path replaces a search field too.
    h.close_search();
    h.location_entry.set_text(&text);
    h.pathbar_box.set_visible(false);
    h.location_box.set_visible(true);
    fade_in(h.location_box.upcast_ref());
    // The entry is on screen only after this turn.
    let entry = h.location_entry.clone();
    glib::idle_add_local_once(move || {
        entry.grab_focus();
        entry.select_region(0, -1);
    });
}

/// `widget` fades in, as the path bar and the field do for each other.
fn fade_in(widget: &gtk::Widget) {
    widget.set_opacity(0.0);
    let target = adw::CallbackAnimationTarget::new({
        let widget = widget.clone();
        move |v| widget.set_opacity(v)
    });
    adw::TimedAnimation::new(widget, 0.0, 1.0, 180, target).play();
}

/// The path bar back in place of the field (nothing if it is there already).
fn close_location(ui: &Ui) {
    let h = &ui.header;
    if !h.location_box.is_visible() {
        return;
    }
    h.location_box.set_visible(false);
    if !h.search_box.is_visible() {
        h.pathbar_box.set_visible(true);
        fade_in(h.pathbar_box.upcast_ref());
    }
}

fn connect_location(ui: &Rc<Ui>) {
    let h = &ui.header;
    action(ui, "location", edit_location);

    // A click on the bar's room (not on a segment or the menu), which is no start of dragging
    // the window: a drag takes the click.
    let click = gtk::GestureClick::builder().button(1).build();
    let weak = Rc::downgrade(ui);
    click.connect_released(move |g, _, x, y| {
        let (Some(ui), Some(bar)) = (weak.upgrade(), g.widget()) else {
            return;
        };
        let mut hit = bar.pick(x, y, gtk::PickFlags::DEFAULT);
        while let Some(w) = hit {
            if w == bar {
                return edit_location(&ui);
            }
            if w.is::<gtk::Button>() {
                return;
            }
            hit = w.parent();
        }
    });
    h.pathbar_box.add_controller(click);

    // A click anywhere else in the window (on the room between the files, which takes no focus)
    // brings the bar back too; the click goes on to what it was for.
    let outside = gtk::GestureClick::new();
    outside.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(ui);
    outside.connect_pressed(move |_, _, x, y| {
        let Some(ui) = weak
            .upgrade()
            .filter(|ui| ui.header.location_box.is_visible())
        else {
            return;
        };
        let field = &ui.header.location_box;
        let inside = ui
            .window
            .pick(x, y, gtk::PickFlags::DEFAULT)
            .is_some_and(|w| w == *field || w.is_ancestor(field));
        if !inside {
            close_location(&ui);
        }
    });
    ui.window.add_controller(outside);

    let weak = Rc::downgrade(ui);
    h.location_close.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            close_location(&ui);
        }
    });
    let key = gtk::EventControllerKey::new();
    let weak = Rc::downgrade(ui);
    key.connect_key_pressed(move |_, keyval, _, _| {
        if keyval == gdk::Key::Escape
            && let Some(ui) = weak.upgrade()
        {
            close_location(&ui);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    h.location_entry.add_controller(key);
    // Leaving the field (a click elsewhere) closes it.
    let focus = gtk::EventControllerFocus::new();
    let weak = Rc::downgrade(ui);
    focus.connect_leave(move |_| {
        if let Some(ui) = weak.upgrade() {
            close_location(&ui);
        }
    });
    h.location_entry.add_controller(focus);

    // Enter: the folder typed, if the server has it.
    let weak = Rc::downgrade(ui);
    h.location_entry.connect_activate(move |entry| {
        let Some(ui) = weak.upgrade() else { return };
        let Some((id, _)) = ui.at.borrow().clone() else {
            return;
        };
        let text = entry.text();
        let path = text
            .split('/')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("/");
        glib::spawn_future_local(async move {
            let p = path.clone();
            match gio::spawn_blocking(move || core().list(id, &p)).await {
                Ok(Ok(_)) => nav::open(&ui, id, path),
                _ => ui.toasts.add_toast(super::note(
                    &gettext("“{path}” is not a folder here")
                        .replace("{path}", &format!("/{path}")),
                )),
            }
        });
    });
}

pub fn connect(ui: &Rc<Ui>) {
    connect_location(ui);
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
    app.set_accels_for_action("win.location", &["<Control>l"]);
    app.set_accels_for_action("win.view-icons", &["<Control>1"]);
    app.set_accels_for_action("win.view-list", &["<Control>2"]);
    app.set_accels_for_action("win.view-columns", &["<Control>3"]);
    app.set_accels_for_action("win.show-hidden", &["<Control>h"]);
    app.set_accels_for_action("win.zoom-in", &["<Control>plus", "<Control>equal"]);
    app.set_accels_for_action("win.zoom-out", &["<Control>minus"]);
    app.set_accels_for_action("win.zoom-reset", &["<Control>0"]);
    super::tabs::accels(app);
}

/// The path bar and what depends on the folder shown: back, forward, up, favorites.
pub fn update(ui: &Rc<Ui>) {
    let at = ui.at.borrow().clone();
    super::tabs::update_title(ui);
    close_location(ui);
    let set = |name: &str, on: bool| {
        ui.window
            .lookup_action(name)
            .and_downcast::<gio::SimpleAction>()
            .inspect(|a| a.set_enabled(on));
    };
    // Results of a search are no place to go back, forward or up from: the search is left by
    // Escape or its button.
    let searching = at
        .as_ref()
        .is_some_and(|(_, p)| places::search_of(p).is_some());
    set("back", nav::can_back(ui) && !searching);
    set("forward", nav::can_forward(ui) && !searching);
    set(
        "up",
        !searching && at.as_ref().is_some_and(|(_, p)| !p.is_empty()),
    );
    // A search field left open by going elsewhere (a click in the sidebar) closes.
    if !searching && ui.header.search_box.is_visible() && !ui.tabs.switching.get() {
        ui.header.close_search();
    }
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
    let p = path.clone();
    super::tabs::middle_click(&weak, &button, move |_| Some((id, p.clone())));
    button.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            nav::open(&ui, id, path.clone());
        }
    });
    ui.header.pathbar.append(&button);
}
