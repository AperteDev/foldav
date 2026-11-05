//! Moving between folders: the history (back and forward), the folders loaded so far, and what
//! the views and the sidebar show of them. A folder seen before shows at once and is asked for
//! again in the background; what the tree and the columns need above and below it is loaded
//! after it.

use super::{Ui, columns, header, ops, sidebar, views};
use crate::core;
use crate::i18n::gettext;
use foldav_core::{Error, File};
use gtk::{gio, glib};
use std::rc::Rc;

/// The folders visited, `pos` the one shown.
#[derive(Default)]
pub struct History {
    items: Vec<(i64, String)>,
    pos: usize,
}

/// How a folder is gone to.
#[derive(Clone, Copy, PartialEq)]
pub enum Go {
    /// A new place in the history.
    Push,
    /// Not in the history: back, forward, or the same folder again.
    Keep,
    /// Another account's, the same folder, or the nearest one above it it has: in the history.
    Switch,
    /// The same folder again, to see what it is now: not in the history, and not from what was
    /// kept on disk (a file just deleted would show for a moment).
    Reload,
}

/// The folders above `path`, the root ("") first: "a/b" has ["", "a"], the root has none.
pub fn ancestors(path: &str) -> Vec<String> {
    if path.is_empty() {
        return Vec::new();
    }
    let parts: Vec<&str> = path.split('/').collect();
    let mut out = vec![String::new()];
    for i in 1..parts.len() {
        out.push(parts[..i].join("/"));
    }
    out
}

/// The folder above `path` ("" above a top-level one).
pub fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(p, _)| p)
}

/// What shows of `files`: hidden ones only when the user wants them, and the favorites starred
/// (as the favorites say, not as the listing did: starring changes no folder's ETag).
pub fn visible(ui: &Ui, files: &[File]) -> Vec<File> {
    let starred = ui
        .at
        .borrow()
        .as_ref()
        .map(|(id, _)| super::favorites::paths(ui, *id))
        .unwrap_or_default();
    files
        .iter()
        .filter(|f| ui.hidden.get() || !f.hidden())
        .map(|f| File {
            favorite: starred.contains(&f.path),
            ..f.clone()
        })
        .collect()
}

pub fn open(ui: &Rc<Ui>, id: i64, path: String) {
    go(ui, id, path, Go::Push);
}

/// Show folder `path` of account `id`.
pub fn go(ui: &Rc<Ui>, id: i64, path: String, how: Go) {
    if core().account(id).is_none() {
        return;
    }
    *ui.at.borrow_mut() = Some((id, path.clone()));
    if matches!(how, Go::Push | Go::Switch) {
        push(ui, id, &path);
    }
    // The tree opens down to it; and at the root, the first time (it can be closed after).
    if !ui.dirs.borrow().keys().any(|(a, _)| *a == id) {
        ui.expanded.borrow_mut().insert((id, String::new()));
    }
    ui.expanded
        .borrow_mut()
        .extend(ancestors(&path).into_iter().map(|p| (id, p)));
    // The favorites of the account: what is kept at once, the server's after.
    let first = !ui.favorites.borrow().contains_key(&id);
    if first {
        super::favorites::load(ui, id);
    }
    chrome(ui);
    if first || how == Go::Reload {
        super::favorites::sync(ui);
    }
    if first || how == Go::Reload || !ui.server.borrow().contains_key(&id) {
        sidebar::load_server(ui, id);
    }
    if !core().connected(id) {
        return show_error(
            ui,
            &gettext("Not connected. Sign in again in Preferences → Accounts."),
        );
    }
    let mut cached = ui.dirs.borrow().get(&(id, path.clone())).cloned();
    // A folder listed before shows at once, as it was; the server is asked after.
    if cached.is_none()
        && how != Go::Reload
        && let Some(files) = core().cached(id, &path)
    {
        let files = Rc::new(files);
        ui.dirs
            .borrow_mut()
            .insert((id, path.clone()), files.clone());
        cached = Some(files);
    }
    if cached.is_some() {
        show(ui);
    } else if ui.views.mode() == views::Mode::Columns {
        // The columns stay as they are, the new one filling when it is loaded.
        columns::refresh(ui);
    } else {
        ui.files_stack.set_visible_child_name("loading");
    }
    let load = ui.load.get() + 1;
    ui.load.set(load);
    let weak = Rc::downgrade(ui);
    glib::spawn_future_local(async move {
        let p = path.clone();
        let r = gio::spawn_blocking(move || fetch(id, p, how == Go::Switch)).await;
        let Some(ui) = weak.upgrade().filter(|u| u.load.get() == load) else {
            return;
        };
        match r {
            Ok(Ok((found, files))) => {
                // What was loaded before stays when it is the same: what is made from it
                // (the columns) is not made again.
                let old = ui.dirs.borrow().get(&(id, found.clone())).cloned();
                let changed = old.as_deref() != Some(&files);
                if changed {
                    ui.dirs
                        .borrow_mut()
                        .insert((id, found.clone()), Rc::new(files));
                }
                if found != path {
                    // A folder above it: that is where it is now.
                    replace_top(&ui, id, &found);
                    return go(&ui, id, found, Go::Keep);
                }
                if changed {
                    show(&ui);
                }
                load_around(&ui);
            }
            Ok(Err(e)) if cached.is_some() => ui.toasts.add_toast(super::note(&e.to_string())),
            Ok(Err(e)) => show_error(&ui, &e.to_string()),
            Err(_) => show_error(&ui, &gettext("Loading failed")),
        }
    });
}

/// The folder `path`; with `fallback`, the nearest one above it that is there.
fn fetch(id: i64, mut path: String, fallback: bool) -> foldav_core::Result<(String, Vec<File>)> {
    loop {
        match core().list(id, &path) {
            Ok(files) => return Ok((path, files)),
            Err(Error::NotFound(_)) if fallback && !path.is_empty() => {
                path = parent(&path).to_string();
            }
            Err(e) => return Err(e),
        }
    }
}

/// What around the folder changes with it: the header, the sidebar, the actions.
fn chrome(ui: &Rc<Ui>) {
    header::update(ui);
    sidebar::refresh(ui);
    ops::update_actions(ui);
}

/// Show the folder as it is loaded: in the views, or the page for none.
pub fn show(ui: &Rc<Ui>) {
    let Some((id, path)) = ui.at.borrow().clone() else {
        return;
    };
    let Some(files) = ui.dirs.borrow().get(&(id, path)).cloned() else {
        return;
    };
    let shown = visible(ui, &files);
    views::show(ui, &shown);
    // The columns show an empty folder as an empty column.
    let empty = shown.is_empty() && ui.views.mode() != views::Mode::Columns;
    ui.files_stack
        .set_visible_child_name(if empty { "empty" } else { "files" });
    columns::refresh(ui);
}

fn show_error(ui: &Ui, text: &str) {
    ui.error.set_description(Some(text));
    ui.files_stack.set_visible_child_name("error");
}

/// Load what the tree and the columns need besides the folder shown: the folders above it and
/// the ones open in the tree.
fn load_around(ui: &Rc<Ui>) {
    let Some((id, path)) = ui.at.borrow().clone() else {
        return;
    };
    let mut need = ancestors(&path);
    need.extend(
        ui.expanded
            .borrow()
            .iter()
            .filter(|(a, _)| *a == id)
            .map(|(_, p)| p.clone()),
    );
    need.sort();
    need.dedup();
    need.retain(|p| !ui.dirs.borrow().contains_key(&(id, p.clone())));
    if need.is_empty() {
        return refresh_around(ui);
    }
    // What was kept of them shows at once; the server is asked after, by the folders' ETags.
    let mut from_disk = false;
    for p in &need {
        if let Some(files) = core().cached(id, p) {
            ui.dirs.borrow_mut().insert((id, p.clone()), Rc::new(files));
            from_disk = true;
        }
    }
    if from_disk {
        refresh_around(ui);
    }
    let weak = Rc::downgrade(ui);
    glib::spawn_future_local(async move {
        let found = gio::spawn_blocking(move || {
            need.into_iter()
                .filter_map(|p| core().list(id, &p).ok().map(|f| (p, f)))
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        let Some(ui) = weak.upgrade() else { return };
        let mut changed = !from_disk;
        for (p, files) in found {
            let old = ui.dirs.borrow().get(&(id, p.clone())).cloned();
            if old.as_deref() != Some(&files) {
                ui.dirs.borrow_mut().insert((id, p), Rc::new(files));
                changed = true;
            }
        }
        if changed {
            refresh_around(&ui);
        }
    });
}

fn refresh_around(ui: &Rc<Ui>) {
    sidebar::refresh_tree(ui);
    columns::refresh(ui);
}

/// Open or close folder `path` of account `id` in the sidebar's tree.
pub fn toggle_expanded(ui: &Rc<Ui>, id: i64, path: &str) {
    let key = (id, path.to_string());
    let opened = !ui.expanded.borrow_mut().remove(&key);
    if opened {
        ui.expanded.borrow_mut().insert(key);
        load_around(ui);
    }
    sidebar::refresh_tree(ui);
}

/// Look whether the folder shown changed on the server (one small request, its ETag), and show
/// it as it is now if it did. For a folder, not a place; what is on screen stays when nothing
/// changed.
pub fn poll(ui: &Rc<Ui>) {
    let Some((id, path)) = ui.at.borrow().clone() else {
        return;
    };
    if foldav_core::places::is_place(&path) || !core().connected(id) {
        return;
    }
    let load = ui.load.get();
    let weak = Rc::downgrade(ui);
    glib::spawn_future_local(async move {
        let p = path.clone();
        let r = gio::spawn_blocking(move || core().changed(id, &p)).await;
        // Another folder was gone to meanwhile: not this one's news.
        let Some(ui) = weak
            .upgrade()
            .filter(|u| u.load.get() == load && *u.at.borrow() == Some((id, path.clone())))
        else {
            return;
        };
        if let Ok(Ok(Some(files))) = r {
            let old = ui.dirs.borrow().get(&(id, path.clone())).cloned();
            if old.as_deref() != Some(&files) {
                ui.dirs.borrow_mut().insert((id, path), Rc::new(files));
                show(&ui);
                refresh_around(&ui);
            }
        }
    });
}

/// Load the shown folder again, and what was loaded of the account with it.
pub fn reload(ui: &Rc<Ui>) {
    let at = ui.at.borrow().clone();
    if let Some((id, path)) = at {
        ui.dirs.borrow_mut().retain(|(a, _), _| *a != id);
        go(ui, id, path, Go::Reload);
    }
}

pub fn up(ui: &Rc<Ui>) {
    let at = ui.at.borrow().clone();
    if let Some((id, path)) = at.filter(|(_, p)| !p.is_empty()) {
        open(ui, id, parent(&path).into());
    }
}

pub fn back(ui: &Rc<Ui>) {
    step(ui, -1);
}

pub fn forward(ui: &Rc<Ui>) {
    step(ui, 1);
}

fn step(ui: &Rc<Ui>, by: isize) {
    let target = {
        let mut h = ui.history.borrow_mut();
        match h.pos.checked_add_signed(by).filter(|p| *p < h.items.len()) {
            Some(p) => {
                h.pos = p;
                h.items.get(p).cloned()
            }
            None => None,
        }
    };
    if let Some((id, path)) = target {
        go(ui, id, path, Go::Keep);
    }
}

pub fn can_back(ui: &Ui) -> bool {
    ui.history.borrow().pos > 0
}

pub fn can_forward(ui: &Ui) -> bool {
    let h = ui.history.borrow();
    h.pos + 1 < h.items.len()
}

fn push(ui: &Ui, id: i64, path: &str) {
    let mut h = ui.history.borrow_mut();
    let here = (id, path.to_string());
    if h.items.get(h.pos) == Some(&here) {
        return;
    }
    let keep = h.pos + 1;
    h.items.truncate(keep);
    h.items.push(here);
    h.pos = h.items.len() - 1;
}

fn replace_top(ui: &Ui, id: i64, path: &str) {
    let mut h = ui.history.borrow_mut();
    let pos = h.pos;
    if let Some(top) = h.items.get_mut(pos) {
        *top = (id, path.to_string());
    }
}

/// Another account, the sidebar's: the same folder if it has it, else the nearest one above.
pub fn switch_account(ui: &Rc<Ui>, id: i64) {
    let path = ui
        .at
        .borrow()
        .as_ref()
        .map(|(_, p)| p.clone())
        .unwrap_or_default();
    go(ui, id, path, Go::Switch);
}

/// Accounts came, went, were switched on or off or renamed: the sidebar shows them as they are
/// now, and the folder of one gone is replaced by the root of the first left.
pub fn accounts_changed(ui: &Rc<Ui>) {
    let accounts: Vec<_> = core()
        .accounts()
        .into_iter()
        .filter(|a| a.enabled)
        .collect();
    let current = ui.at.borrow().as_ref().map(|(id, _)| *id);
    match (current, accounts.first()) {
        (Some(id), _) if accounts.iter().any(|a| a.id == id) => chrome(ui),
        (_, Some(a)) => open(ui, a.id, String::new()),
        (_, None) => {
            ui.at.take();
            ui.files_stack.set_visible_child_name("empty");
            chrome(ui);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folders_above_a_path() {
        assert!(ancestors("").is_empty());
        assert_eq!(ancestors("a"), [""]);
        assert_eq!(ancestors("a/b/c"), ["", "a", "a/b"]);
        assert_eq!((parent("a/b"), parent("a"), parent("")), ("a", "", ""));
    }
}
