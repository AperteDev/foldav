//! The sidebar, as Nautilus': the accounts (only with more than one; the one shown is
//! selected), the bookmarks of that account, and a dense tree of its folders. Changing the
//! account keeps the folder shown, where the new account has it.

use super::{Ui, nav, popup_menu};
use crate::core;
use crate::i18n::gettext;
use adw::prelude::*;
use foldav_core::Favorite;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

/// How far each folder level is indented in the tree.
const INDENT: i32 = 14;

pub struct Sidebar {
    accounts: gtk::ListBox,
    accounts_separator: gtk::Separator,
    favorites_heading: gtk::Label,
    favorites: gtk::ListBox,
    favorites_separator: gtk::Separator,
    tree: gtk::ListBox,
    /// The account of each row of `accounts`.
    account_ids: RefCell<Vec<i64>>,
    /// The favorite folder of each row of `favorites`.
    favorite_rows: RefCell<Vec<Favorite>>,
    /// The folder of each row of `tree`.
    tree_paths: RefCell<Vec<String>>,
}

impl Sidebar {
    pub fn new(b: &gtk::Builder) -> Sidebar {
        Sidebar {
            accounts: super::get(b, "accounts"),
            accounts_separator: super::get(b, "accounts_separator"),
            favorites_heading: super::get(b, "favorites_heading"),
            favorites: super::get(b, "favorites"),
            favorites_separator: super::get(b, "favorites_separator"),
            tree: super::get(b, "tree"),
            account_ids: RefCell::default(),
            favorite_rows: RefCell::default(),
            tree_paths: RefCell::default(),
        }
    }
}

pub fn connect(ui: &Rc<Ui>) {
    let weak = Rc::downgrade(ui);
    ui.sidebar.accounts.connect_row_activated(move |_, row| {
        let Some(ui) = weak.upgrade() else { return };
        let id = ui
            .sidebar
            .account_ids
            .borrow()
            .get(row.index() as usize)
            .copied();
        if let Some(id) = id {
            nav::switch_account(&ui, id);
        }
    });

    let weak = Rc::downgrade(ui);
    ui.sidebar.favorites.connect_row_activated(move |_, row| {
        let Some(ui) = weak.upgrade() else { return };
        let item = ui
            .sidebar
            .favorite_rows
            .borrow()
            .get(row.index() as usize)
            .cloned();
        let id = ui.at.borrow().as_ref().map(|(id, _)| *id);
        if let (Some(f), Some(id)) = (item, id) {
            nav::open(&ui, id, f.path);
        }
    });

    let weak = Rc::downgrade(ui);
    ui.sidebar.tree.connect_row_activated(move |_, row| {
        let Some(ui) = weak.upgrade() else { return };
        let path = ui
            .sidebar
            .tree_paths
            .borrow()
            .get(row.index() as usize)
            .cloned();
        let id = ui.at.borrow().as_ref().map(|(id, _)| *id);
        if let (Some(path), Some(id)) = (path, id) {
            nav::open(&ui, id, path);
        }
    });

    // Unstarring a favorite, from its right-click menu.
    let remove = gio::SimpleAction::new("favorite-delete", Some(glib::VariantTy::STRING));
    let weak = Rc::downgrade(ui);
    remove.connect_activate(move |_, path| {
        let (Some(ui), Some(path)) = (weak.upgrade(), path.and_then(|v| v.get::<String>())) else {
            return;
        };
        let id = ui.at.borrow().as_ref().map(|(id, _)| *id);
        if let Some(id) = id {
            super::favorites::toggle(&ui, id, vec![(path, true)], false);
        }
    });
    ui.window.add_action(&remove);
}

pub fn refresh(ui: &Rc<Ui>) {
    accounts(ui);
    refresh_favorites(ui);
    refresh_tree(ui);
}

/// A row: an icon and a name.
fn row(icon: &str, name: &str) -> gtk::ListBoxRow {
    let b = gtk::Box::builder().spacing(10).build();
    b.append(&gtk::Image::from_icon_name(icon));
    b.append(
        &gtk::Label::builder()
            .label(name)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build(),
    );
    gtk::ListBoxRow::builder().child(&b).build()
}

fn accounts(ui: &Rc<Ui>) {
    let s = &ui.sidebar;
    s.accounts.remove_all();
    let enabled: Vec<_> = core()
        .accounts()
        .into_iter()
        .filter(|a| a.enabled)
        .collect();
    for a in &enabled {
        s.accounts.append(&row(
            crate::settings::account_icon(a),
            &crate::account_name(a),
        ));
    }
    // With one there is nothing to choose.
    let many = enabled.len() > 1;
    s.accounts.set_visible(many);
    s.accounts_separator.set_visible(many);
    let current = ui.at.borrow().as_ref().map(|(id, _)| *id);
    let at = enabled.iter().position(|a| Some(a.id) == current);
    s.accounts
        .select_row(at.and_then(|i| s.accounts.row_at_index(i as i32)).as_ref());
    *s.account_ids.borrow_mut() = enabled.iter().map(|a| a.id).collect();
}

/// The favorite folders of the account shown, as Nextcloud Files lists them under "Favorites";
/// none, no heading and no list.
pub fn refresh_favorites(ui: &Rc<Ui>) {
    let s = &ui.sidebar;
    s.favorites.remove_all();
    let at = ui.at.borrow().clone();
    let list = at
        .as_ref()
        .map(|(id, _)| super::favorites::folders(ui, *id))
        .unwrap_or_default();
    for (visible, w) in [
        (
            !list.is_empty(),
            s.favorites_heading.upcast_ref::<gtk::Widget>(),
        ),
        (!list.is_empty(), s.favorites.upcast_ref()),
        (!list.is_empty(), s.favorites_separator.upcast_ref()),
    ] {
        w.set_visible(visible);
    }
    for f in &list {
        let r = row(
            "folder-symbolic",
            f.path.rsplit('/').next().unwrap_or(&f.path),
        );
        let click = gtk::GestureClick::builder().button(3).build();
        let (weak, path) = (r.downgrade(), f.path.clone());
        click.connect_pressed(move |g, _, x, y| {
            g.set_state(gtk::EventSequenceState::Claimed);
            if let Some(r) = weak.upgrade() {
                let menu = gio::Menu::new();
                let item = gio::MenuItem::new(Some(&gettext("Remove from Favorites")), None);
                item.set_action_and_target_value(
                    Some("win.favorite-delete"),
                    Some(&path.to_variant()),
                );
                menu.append_item(&item);
                popup_menu(r.upcast_ref(), &menu, x, y);
            }
        });
        r.add_controller(click);
        s.favorites.append(&r);
    }
    let here = at.and_then(|(_, path)| list.iter().position(|f| f.path == path));
    s.favorites.select_row(
        here.and_then(|i| s.favorites.row_at_index(i as i32))
            .as_ref(),
    );
    *s.favorite_rows.borrow_mut() = list;
}

/// The tree of the folders of the account shown: down to the folder shown, and as far as the
/// user opened it, from what is loaded.
pub fn refresh_tree(ui: &Rc<Ui>) {
    let s = &ui.sidebar;
    s.tree.remove_all();
    s.tree_paths.borrow_mut().clear();
    let Some((id, path)) = ui.at.borrow().clone() else {
        return;
    };
    let mut rows = Vec::new();
    collect(ui, id, "", &gettext("Home"), 0, &mut rows);
    let mut selected = None;
    for (i, (p, name, depth)) in rows.iter().enumerate() {
        s.tree.append(&tree_row(ui, id, p, name, *depth));
        if *p == path {
            selected = Some(i);
        }
        s.tree_paths.borrow_mut().push(p.clone());
    }
    s.tree.select_row(
        selected
            .and_then(|i| s.tree.row_at_index(i as i32))
            .as_ref(),
    );
}

/// The rows of folder `path` and of the folders open below it: (path, name, depth).
fn collect(
    ui: &Ui,
    id: i64,
    path: &str,
    name: &str,
    depth: i32,
    rows: &mut Vec<(String, String, i32)>,
) {
    rows.push((path.to_string(), name.to_string(), depth));
    if !ui.expanded.borrow().contains(&(id, path.to_string())) {
        return;
    }
    let files = ui.dirs.borrow().get(&(id, path.to_string())).cloned();
    for f in files.iter().flat_map(|f| f.iter()) {
        if f.folder && (ui.hidden.get() || !f.hidden()) {
            collect(ui, id, &f.path, &f.name, depth + 1, rows);
        }
    }
}

fn tree_row(ui: &Rc<Ui>, id: i64, path: &str, name: &str, depth: i32) -> gtk::ListBoxRow {
    let open = ui.expanded.borrow().contains(&(id, path.to_string()));
    // Known to have no folders in it: no arrow, but its room, so the names stay in line.
    let leaf = ui
        .dirs
        .borrow()
        .get(&(id, path.to_string()))
        .is_some_and(|f| {
            !f.iter()
                .any(|f| f.folder && (ui.hidden.get() || !f.hidden()))
        });
    let arrow = gtk::Button::builder()
        .icon_name(if open {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        })
        .css_classes(["flat", "tree-arrow"])
        .valign(gtk::Align::Center)
        .build();
    if leaf {
        arrow.set_opacity(0.0);
        arrow.set_can_target(false);
    }
    let (weak, p) = (Rc::downgrade(ui), path.to_string());
    arrow.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            nav::toggle_expanded(&ui, id, &p);
        }
    });
    let b = gtk::Box::builder()
        .spacing(4)
        .margin_start(depth * INDENT)
        .build();
    b.append(&arrow);
    b.append(&gtk::Image::from_icon_name(if path.is_empty() {
        "user-home-symbolic"
    } else {
        "folder-symbolic"
    }));
    b.append(
        &gtk::Label::builder()
            .label(name)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build(),
    );
    gtk::ListBoxRow::builder().child(&b).build()
}
