//! Tabs, as Files has them: the tab bar shows when there is more than one, and each tab has its
//! own folder and history. The views are one set, shared: a tab not shown keeps its place (the
//! folder and the history) and the views take the next one's when it is selected, so what was
//! selected or scrolled to in a tab is not kept. Files dragged onto a tab go into its folder.

use super::nav::{self, Go, History};
use super::{Ui, action, get, sidebar};
use crate::i18n::gettext;
use adw::prelude::*;
use gtk::{gdk, glib};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

type Place = (i64, String);

/// A tab: its page's child (what tells it apart), and where it is while it is not shown.
struct Tab {
    child: gtk::Widget,
    at: Option<Place>,
    history: History,
    /// Where it opened, until it is shown the first time.
    start: Option<Place>,
}

pub struct Tabs {
    view: adw::TabView,
    bar: adw::TabBar,
    tabs: RefCell<Vec<Tab>>,
    /// The tab the views show.
    current: RefCell<Option<gtk::Widget>>,
    /// Set while the views change from one tab to another: the search's changes then are not the
    /// user's.
    pub switching: Cell<bool>,
}

impl Tabs {
    pub fn new(b: &gtk::Builder) -> Tabs {
        Tabs {
            view: get(b, "tab_view"),
            bar: get(b, "tab_bar"),
            tabs: RefCell::default(),
            current: RefCell::default(),
            switching: Cell::new(false),
        }
    }
}

/// What a tab is called: its folder's name, as the path bar says it.
fn title(ui: &Ui, at: Option<&Place>) -> String {
    let Some((id, path)) = at else {
        return gettext("Home");
    };
    if foldav_core::places::is_place(path) {
        return sidebar::place_title(ui, *id, path).unwrap_or_default();
    }
    match path.rsplit('/').next().filter(|n| !n.is_empty()) {
        Some(name) => name.to_string(),
        None => gettext("Home"),
    }
}

/// The selected tab is called after the folder shown.
pub fn update_title(ui: &Ui) {
    let Some(page) = ui.tabs.view.selected_page() else {
        return;
    };
    let at = ui.at.borrow().clone();
    let title = title(ui, at.as_ref());
    page.set_title(&title);
    page.set_tooltip(&match &at {
        Some((_, p)) if !p.is_empty() && !foldav_core::places::is_place(p) => format!("/{p}"),
        _ => title,
    });
}

/// A new tab, after the one shown, at `at` (none: where the first one began).
pub fn open(ui: &Rc<Ui>, at: Option<Place>) {
    let t = &ui.tabs;
    let child = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let position = t
        .view
        .selected_page()
        .map_or(0, |p| t.view.page_position(&p) + 1);
    t.tabs.borrow_mut().push(Tab {
        child: child.clone().upcast(),
        at: None,
        history: History::default(),
        start: at.clone(),
    });
    let page = t.view.insert(&child, position);
    page.set_title(&title(ui, at.as_ref()));
    t.view.set_selected_page(&page);
}

/// Where the tab with `child` is: the folder shown if it is the shown one.
fn place_of(ui: &Ui, child: &gtk::Widget) -> Option<Place> {
    if ui.tabs.current.borrow().as_ref() == Some(child) {
        return ui.at.borrow().clone();
    }
    ui.tabs
        .tabs
        .borrow()
        .iter()
        .find(|t| t.child == *child)
        .and_then(|t| t.at.clone().or_else(|| t.start.clone()))
}

/// The views show the tab selected now: what the tab before it had goes into it, and its own
/// folder and history come out.
fn switch(ui: &Rc<Ui>) {
    let t = &ui.tabs;
    let Some(child) = t.view.selected_page().map(|p| p.child()) else {
        return;
    };
    let old = t.current.replace(Some(child.clone()));
    if old.as_ref() == Some(&child) {
        return;
    }
    t.switching.set(true);
    // A search belongs to the tab it was made in.
    ui.header.close_search();
    let (at, history, start) = {
        let mut tabs = t.tabs.borrow_mut();
        if let Some(old) = tabs.iter_mut().find(|x| Some(&x.child) == old.as_ref()) {
            old.at = ui.at.borrow().clone();
            old.history = ui.history.take();
        }
        match tabs.iter_mut().find(|x| x.child == child) {
            Some(next) => (
                next.at.take(),
                std::mem::take(&mut next.history),
                next.start.take(),
            ),
            None => (None, History::default(), None),
        }
    };
    *ui.history.borrow_mut() = history;
    match (start, at) {
        // A tab not shown before: its place is the first in its history.
        (Some((id, path)), _) => nav::go(ui, id, path, Go::Push),
        (None, Some((id, path))) => nav::go(ui, id, path, Go::Keep),
        (None, None) => {}
    }
    t.switching.set(false);
}

/// Close the tab shown, or the window when it is the last.
fn close(ui: &Rc<Ui>) {
    let t = &ui.tabs;
    match t.view.selected_page() {
        Some(page) if t.view.n_pages() > 1 => t.view.close_page(&page),
        _ => ui.window.close(),
    }
}

/// The tab `n` (from 0) of the bar, if there is one.
fn select(ui: &Rc<Ui>, n: i32) {
    let view = &ui.tabs.view;
    if n >= 0 && n < view.n_pages() {
        view.set_selected_page(&view.nth_page(n));
    }
}

/// A middle click on `widget` opens the folder `dest` says in a new tab.
pub fn middle_click(
    ui: &Weak<Ui>,
    widget: &impl IsA<gtk::Widget>,
    dest: impl Fn(&Ui) -> Option<Place> + 'static,
) {
    let click = gtk::GestureClick::builder().button(2).build();
    let weak = ui.clone();
    click.connect_pressed(move |g, _, _, _| {
        if let Some(ui) = weak.upgrade()
            && let Some(at) = dest(&ui)
        {
            g.set_state(gtk::EventSequenceState::Claimed);
            open(&ui, Some(at));
        }
    });
    widget.add_controller(click);
}

pub fn accels(app: &adw::Application) {
    app.set_accels_for_action("win.new-tab", &["<Control>t"]);
    app.set_accels_for_action("win.close-tab", &["<Control>w"]);
    app.set_accels_for_action(
        "win.tab-next",
        &[
            "<Control>Page_Down",
            "<Control>Tab",
            "<Control><Alt>Page_Down",
        ],
    );
    app.set_accels_for_action(
        "win.tab-prev",
        &[
            "<Control>Page_Up",
            "<Control><Shift>Tab",
            "<Control><Alt>Page_Up",
        ],
    );
    app.set_accels_for_action("win.tab-move-left", &["<Control><Shift>Page_Up"]);
    app.set_accels_for_action("win.tab-move-right", &["<Control><Shift>Page_Down"]);
    for n in 1..=9 {
        app.set_accels_for_action(&format!("win.tab-goto({n})"), &[&format!("<Alt>{n}")]);
    }
}

pub fn connect(ui: &Rc<Ui>) {
    let t = &ui.tabs;
    // The shortcuts are the actions' below.
    t.view.set_shortcuts(adw::TabViewShortcuts::empty());
    // The first tab shows what the window shows already.
    let child = gtk::Box::new(gtk::Orientation::Vertical, 0);
    t.tabs.borrow_mut().push(Tab {
        child: child.clone().upcast(),
        at: None,
        history: History::default(),
        start: None,
    });
    t.view.append(&child).set_title(&gettext("Home"));
    *t.current.borrow_mut() = Some(child.upcast());

    let weak = Rc::downgrade(ui);
    t.view.connect_selected_page_notify(move |_| {
        if let Some(ui) = weak.upgrade() {
            switch(&ui);
        }
    });
    let weak = Rc::downgrade(ui);
    t.view.connect_page_detached(move |_, page, _| {
        if let Some(ui) = weak.upgrade() {
            let child = page.child();
            ui.tabs.tabs.borrow_mut().retain(|t| t.child != child);
        }
    });

    action(ui, "new-tab", |ui| {
        let at = ui.at.borrow().clone();
        open(ui, at);
    });
    action(ui, "close-tab", close);
    action(ui, "tab-next", |ui| {
        let view = &ui.tabs.view;
        if !view.select_next_page() {
            select(ui, 0);
        }
    });
    action(ui, "tab-prev", |ui| {
        let view = &ui.tabs.view;
        if !view.select_previous_page() {
            select(ui, view.n_pages() - 1);
        }
    });
    action(ui, "tab-move-left", |ui| {
        if let Some(page) = ui.tabs.view.selected_page() {
            ui.tabs.view.reorder_backward(&page);
        }
    });
    action(ui, "tab-move-right", |ui| {
        if let Some(page) = ui.tabs.view.selected_page() {
            ui.tabs.view.reorder_forward(&page);
        }
    });
    let goto = gtk::gio::SimpleAction::new("tab-goto", Some(glib::VariantTy::INT32));
    let weak = Rc::downgrade(ui);
    goto.connect_activate(move |_, n| {
        if let (Some(ui), Some(n)) = (weak.upgrade(), n.and_then(|n| n.get::<i32>())) {
            select(&ui, n - 1);
        }
    });
    ui.window.add_action(&goto);

    // Files dropped on a tab go into its folder (the bar switches to a tab dragged over, too).
    t.bar.setup_extra_drop_target(
        gdk::DragAction::COPY | gdk::DragAction::MOVE,
        &[
            super::dnd::Dragged::static_type(),
            gdk::FileList::static_type(),
        ],
    );
    let weak = Rc::downgrade(ui);
    t.bar.connect_extra_drag_drop(move |bar, page, value| {
        let Some(ui) = weak.upgrade() else {
            return false;
        };
        let Some((id, path)) = place_of(&ui, &page.child()) else {
            return false;
        };
        // Ctrl held: a copy.
        let copy = bar
            .display()
            .default_seat()
            .and_then(|s| s.keyboard())
            .is_some_and(|k| k.modifier_state().contains(gdk::ModifierType::CONTROL_MASK));
        super::dnd::drop_value(&ui, value, id, path, !copy)
    });
}
