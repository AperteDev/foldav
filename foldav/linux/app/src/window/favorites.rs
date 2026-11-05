//! Favorites in the window, as Nextcloud Files has them: a star on files and folders, the
//! favorite folders in the sidebar. What is starred is the server's, kept here (`foldav_core`
//! `favorites.rs`): shown at once, sent when it can be, and asked of the server again when the
//! window comes to the front, on a reload and when an account is shown, so what another device
//! starred shows without a restart.

use super::{Ui, action, nav, note, sidebar};
use crate::core;
use crate::i18n::gettext;
use adw::prelude::*;
use foldav_core::Favorite;
use gtk::{gio, glib};
use std::collections::HashSet;
use std::rc::Rc;

/// The actions that star, the keys and a sync when the window comes to the front.
pub fn connect(ui: &Rc<Ui>) {
    action(ui, "favorite-add", |ui| folder(ui, true));
    action(ui, "favorite-remove", |ui| folder(ui, false));
    action(ui, "file-favorite", |ui| files(ui, true));
    action(ui, "file-unfavorite", |ui| files(ui, false));
    let weak = Rc::downgrade(ui);
    ui.window.connect_is_active_notify(move |w| {
        if let (true, Some(ui)) = (w.is_active(), weak.upgrade()) {
            sync(&ui);
        }
    });
}

pub fn accels(app: &adw::Application) {
    app.set_accels_for_action("win.favorite-add", &["<Control>d"]);
}

/// The paths starred in account `id`.
pub fn paths(ui: &Ui, id: i64) -> HashSet<String> {
    ui.favorites
        .borrow()
        .get(&id)
        .map(|f| f.iter().map(|f| f.path.clone()).collect())
        .unwrap_or_default()
}

/// Take account `id`'s favorites from what is kept (no request).
pub fn load(ui: &Ui, id: i64) {
    ui.favorites.borrow_mut().insert(id, core().favorites(id));
}

/// Ask the server for the shown account's favorites, sending what waits to go up first; what
/// shows follows if they changed.
pub fn sync(ui: &Rc<Ui>) {
    let Some((id, _)) = ui
        .at
        .borrow()
        .clone()
        .filter(|(id, _)| core().connected(*id))
    else {
        return;
    };
    let weak = Rc::downgrade(ui);
    glib::spawn_future_local(async move {
        let r = gio::spawn_blocking(move || core().sync_favorites(id)).await;
        let Some(ui) = weak.upgrade() else { return };
        if matches!(r, Ok(Ok(true))) {
            load(&ui, id);
            apply(&ui);
        }
    });
}

/// Star or unstar `items` (path, folder) of account `id`.
pub fn toggle(ui: &Rc<Ui>, id: i64, items: Vec<(String, bool)>, on: bool) {
    let weak = Rc::downgrade(ui);
    glib::spawn_future_local(async move {
        let r = gio::spawn_blocking(move || {
            items
                .iter()
                .try_for_each(|(p, folder)| core().set_favorite(id, p, *folder, on))
        })
        .await;
        let Some(ui) = weak.upgrade() else { return };
        load(&ui, id);
        apply(&ui);
        if !matches!(r, Ok(Ok(()))) {
            ui.toasts
                .add_toast(note(&gettext("Cannot change the favorites")));
        }
    });
}

/// What shows follows the favorites: the sidebar, the stars, the actions.
pub fn apply(ui: &Rc<Ui>) {
    ui.fav_version.set(ui.fav_version.get() + 1);
    sidebar::refresh_favorites(ui);
    nav::show(ui);
    update_actions(ui);
}

/// The folder shown, starred or not.
fn folder(ui: &Rc<Ui>, on: bool) {
    let at = ui.at.borrow().clone();
    if let Some((id, path)) = at.filter(|(_, p)| !p.is_empty()) {
        toggle(ui, id, vec![(path, true)], on);
    }
}

/// What is selected, starred or not.
fn files(ui: &Rc<Ui>, on: bool) {
    let id = ui.at.borrow().as_ref().map(|(id, _)| *id);
    let items: Vec<(String, bool)> = ui
        .views
        .selected()
        .into_iter()
        .map(|f| (f.path, f.folder))
        .collect();
    if let (Some(id), false) = (id, items.is_empty()) {
        toggle(ui, id, items, on);
    }
}

/// Enable what applies: the folder shown can be starred or unstarred (not the root), what is
/// selected is starred unless all of it is.
pub fn update_actions(ui: &Ui) {
    let at = ui.at.borrow().clone();
    let selected = ui.views.selected();
    let (folder, all) = match &at {
        Some((id, path)) => {
            let starred = paths(ui, *id);
            (
                (!path.is_empty() && !foldav_core::places::is_place(path))
                    .then(|| starred.contains(path)),
                !selected.is_empty() && selected.iter().all(|f| starred.contains(&f.path)),
            )
        }
        None => (None, false),
    };
    // What is in the trash bin cannot be starred.
    let in_trash = at
        .as_ref()
        .is_some_and(|(_, p)| p == foldav_core::places::TRASH);
    for (name, on) in [
        ("favorite-add", folder == Some(false)),
        ("favorite-remove", folder == Some(true)),
        ("file-favorite", !selected.is_empty() && !all && !in_trash),
        ("file-unfavorite", all && !in_trash),
    ] {
        ui.window
            .lookup_action(name)
            .and_downcast::<gio::SimpleAction>()
            .inspect(|a| a.set_enabled(on));
    }
}

/// The favorite folders of the account shown, for the sidebar.
pub fn folders(ui: &Ui, id: i64) -> Vec<Favorite> {
    ui.favorites
        .borrow()
        .get(&id)
        .map(|f| f.iter().filter(|f| f.folder).cloned().collect())
        .unwrap_or_default()
}
