//! The sidebar, as Nautilus': the accounts (only with more than one; the one shown is
//! selected), the bookmarks of that account, and a dense tree of its folders. Changing the
//! account keeps the folder shown, where the new account has it.

use super::{Ui, nav, popup_menu};
use crate::core;
use crate::i18n::gettext;
use adw::prelude::*;
use foldav_core::places::{self, Tag};
use foldav_core::{Favorite, Features, Quota};
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
    places: gtk::ListBox,
    places_separator: gtk::Separator,
    quota_box: gtk::Box,
    quota: gtk::Label,
    quota_bar: gtk::ProgressBar,
    /// The place (path) of each row of `places`.
    place_paths: RefCell<Vec<String>>,
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
            places: super::get(b, "places"),
            places_separator: super::get(b, "places_separator"),
            quota_box: super::get(b, "quota_box"),
            quota: super::get(b, "quota"),
            quota_bar: super::get(b, "quota_bar"),
            place_paths: RefCell::default(),
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

    let weak = Rc::downgrade(ui);
    ui.sidebar.places.connect_row_activated(move |_, row| {
        let Some(ui) = weak.upgrade() else { return };
        let path = ui
            .sidebar
            .place_paths
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
    refresh_places(ui);
    refresh_tree(ui);
}

/// What the server of an account offers, and what is on it besides folders.
#[derive(Default)]
pub struct Server {
    pub features: Features,
    pub tags: Vec<Tag>,
    pub quota: Option<Quota>,
}

/// Ask account `id`'s server what it offers, its tags and its quota (the places and the storage
/// used show once they are known).
pub fn load_server(ui: &Rc<Ui>, id: i64) {
    let weak = Rc::downgrade(ui);
    glib::spawn_future_local(async move {
        let r = gio::spawn_blocking(move || {
            let features = core().features(id).ok()?;
            let tags = if features.tags {
                core().tags(id).unwrap_or_default()
            } else {
                Vec::new()
            };
            Some(Server {
                features,
                tags,
                quota: core().quota(id).ok(),
            })
        })
        .await;
        if let (Some(ui), Ok(Some(server))) = (weak.upgrade(), r) {
            ui.server.borrow_mut().insert(id, server);
            refresh_places(&ui);
        }
    });
}

/// The places of the account shown: (path, title, icon).
fn place_list(ui: &Ui, id: i64) -> Vec<(String, String, &'static str)> {
    let server = ui.server.borrow();
    let known = server.get(&id);
    let mut list = vec![
        (
            places::RECENT.to_string(),
            gettext("Recent"),
            "document-open-recent-symbolic",
        ),
        (
            places::FAVORITES.to_string(),
            gettext("Starred"),
            "starred-symbolic",
        ),
    ];
    let Some(s) = known else { return list };
    if s.features.sharing {
        list.extend([
            (
                places::SHARED_WITH_YOU.to_string(),
                gettext("Shared with You"),
                "folder-publicshare-symbolic",
            ),
            (
                places::SHARED_WITH_OTHERS.to_string(),
                gettext("Shared with Others"),
                "folder-publicshare-symbolic",
            ),
            (
                places::SHARED_BY_LINK.to_string(),
                gettext("Shared by Link"),
                "insert-link-symbolic",
            ),
        ]);
    }
    if s.features.tags {
        list.push((places::TAGS.to_string(), gettext("Tags"), "tag-symbolic"));
    }
    if s.features.trash {
        list.push((
            places::TRASH.to_string(),
            gettext("Deleted Files"),
            "user-trash-symbolic",
        ));
    }
    list
}

/// The title of place `path` of account `id`, as the path bar shows it.
pub fn place_title(ui: &Ui, id: i64, path: &str) -> Option<String> {
    if let Some((_, query)) = places::search_of(path) {
        return Some(gettext("Search: {query}").replace("{query}", query));
    }
    if let Some(tag) = path.strip_prefix(places::TAG_PREFIX) {
        let server = ui.server.borrow();
        return server
            .get(&id)?
            .tags
            .iter()
            .find(|t| t.id == tag)
            .map(|t| t.name.clone());
    }
    place_list(ui, id)
        .into_iter()
        .find(|(p, _, _)| p == path)
        .map(|(_, title, _)| title)
}

/// The rows of the places, and the storage used.
pub fn refresh_places(ui: &Rc<Ui>) {
    let s = &ui.sidebar;
    s.places.remove_all();
    let at = ui.at.borrow().clone();
    let Some((id, path)) = at else {
        s.places.set_visible(false);
        s.places_separator.set_visible(false);
        return;
    };
    let list = place_list(ui, id);
    for (p, title, icon) in &list {
        let r = row(icon, title);
        let target = p.clone();
        super::tabs::middle_click(&Rc::downgrade(ui), &r, move |_| Some((id, target.clone())));
        s.places.append(&r);
        if *p == path {
            s.places.select_row(Some(&r));
        }
    }
    s.places.set_visible(true);
    s.places_separator.set_visible(true);
    *s.place_paths.borrow_mut() = list.into_iter().map(|(p, _, _)| p).collect();
    let quota = ui.server.borrow().get(&id).and_then(|s| s.quota);
    s.quota_box.set_visible(quota.is_some());
    if let Some(q) = quota {
        let used = glib::format_size(q.used);
        s.quota.set_label(&match q.total {
            Some(total) => gettext("Used {used} out of {total}")
                .replace("{used}", &used)
                .replace("{total}", &glib::format_size(total)),
            None => gettext("Used {used}").replace("{used}", &used),
        });
        // A bar only where there is a limit; red when a tenth or less is left.
        let share = q
            .total
            .filter(|&t| t > 0)
            .map(|t| (q.used as f64 / t as f64).min(1.0));
        s.quota_bar.set_visible(share.is_some());
        s.quota_bar.set_fraction(share.unwrap_or(0.0));
        if share.is_some_and(|s| s >= 0.9) {
            s.quota_bar.add_css_class("low");
        } else {
            s.quota_bar.remove_css_class("low");
        }
    }
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
        let r = row(crate::settings::account_icon(a), &crate::account_name(a));
        // Files dropped on an account are copied to its root.
        let id = a.id;
        r.add_controller(super::dnd::target(&Rc::downgrade(ui), true, move |_| {
            Some((id, String::new()))
        }));
        s.accounts.append(&r);
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
        let (id, path) = (at.as_ref().map(|(id, _)| *id), f.path.clone());
        let p = path.clone();
        r.add_controller(super::dnd::target(&Rc::downgrade(ui), true, move |_| {
            Some((id?, p.clone()))
        }));
        super::tabs::middle_click(&Rc::downgrade(ui), &r, move |_| Some((id?, path.clone())));
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
    let row = gtk::ListBoxRow::builder().child(&b).build();
    let p = path.to_string();
    row.add_controller(super::dnd::target(&Rc::downgrade(ui), true, move |_| {
        Some((id, p.clone()))
    }));
    let p = path.to_string();
    super::tabs::middle_click(&Rc::downgrade(ui), &row, move |_| Some((id, p.clone())));
    row
}
