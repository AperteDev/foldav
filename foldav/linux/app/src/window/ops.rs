//! File operations in the window: new folder, rename, cut, copy and paste, delete, and undo.
//! Each is one request to the server (`foldav_core` `ops.rs`), run in the background; the list
//! is loaded again after it. A right click on the files opens the same actions as a menu.

use super::nav::reload;
use super::{Ui, action, note, popup_menu, ui};
use crate::core;
use crate::i18n::gettext;
use adw::prelude::*;
use foldav_core::{Client, Error, File};
use gtk::{gio, glib};
use std::rc::Rc;

/// Files cut or copied, to paste into a folder of the same account.
pub struct Clipboard {
    account: i64,
    paths: Vec<String>,
    cut: bool,
}

/// The selected files, in the order of the view.
fn selected(ui: &Ui) -> Vec<File> {
    ui.views.selected()
}

/// What went wrong, as the user reads it.
fn error_text(e: &Error) -> String {
    match e {
        Error::Exists(name) => gettext("“{name}” exists already").replace("{name}", name),
        Error::Invalid(name) => gettext("“{name}” cannot be used here").replace("{name}", name),
        Error::NotFound(_) => gettext("It is not on the server any more"),
        Error::Locked(_) => gettext("It is locked on the server"),
        e => e.to_string(),
    }
}

/// The actions (keys in `accels`).
pub fn connect(ui: &Rc<Ui>) {
    action(ui, "new-folder", new_folder);
    action(ui, "rename", rename);
    action(ui, "cut", |ui| copy(ui, true));
    action(ui, "copy", |ui| copy(ui, false));
    action(ui, "paste", paste);
    action(ui, "delete", delete);
    action(ui, "undo", |_| undo(None));
    update_actions(ui);
}

pub fn accels(app: &adw::Application) {
    app.set_accels_for_action("win.new-folder", &["<Control><Shift>n"]);
    app.set_accels_for_action("win.rename", &["F2"]);
    app.set_accels_for_action("win.cut", &["<Control>x"]);
    app.set_accels_for_action("win.copy", &["<Control>c"]);
    app.set_accels_for_action("win.paste", &["<Control>v"]);
    app.set_accels_for_action("win.delete", &["Delete"]);
    app.set_accels_for_action("win.undo", &["<Control>z"]);
}

/// Enable what the folder shown and the selection allow.
pub fn update_actions(ui: &Ui) {
    let n = selected(ui).len();
    let at = ui.at.borrow().as_ref().map(|(id, _)| *id);
    let paste = ui
        .clipboard
        .borrow()
        .as_ref()
        .is_some_and(|c| Some(c.account) == at);
    for (name, on) in [
        ("new-folder", at.is_some()),
        ("rename", n == 1),
        ("cut", n > 0),
        ("copy", n > 0),
        ("delete", n > 0),
        ("paste", paste),
    ] {
        ui.window
            .lookup_action(name)
            .and_downcast::<gio::SimpleAction>()
            .inspect(|a| a.set_enabled(on));
    }
}

/// The menu of a right click at `x`, `y` of `widget`; what is clicked is selected already.
pub fn context_menu(ui: &Ui, widget: &gtk::Widget, x: f64, y: f64) {
    update_actions(ui);
    popup_menu(widget, &ui.file_menu, x, y);
}

/// Ask for a name in a dialog; `select` characters of `initial` from its start are selected
/// (a file's name without its extension), -1 for all. None when cancelled.
async fn ask_name(ui: &Ui, heading: &str, ok: &str, initial: &str, select: i32) -> Option<String> {
    let entry = gtk::Entry::builder()
        .text(initial)
        .activates_default(true)
        .build();
    let alert = adw::AlertDialog::new(Some(heading), None);
    alert.add_responses(&[("cancel", &gettext("Cancel")), ("ok", ok)]);
    alert.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    alert.set_default_response(Some("ok"));
    alert.set_close_response("cancel");
    alert.set_extra_child(Some(&entry));
    let e = entry.clone();
    glib::idle_add_local_once(move || {
        e.grab_focus();
        e.select_region(0, select);
    });
    if alert.choose_future(Some(&ui.window)).await != "ok" {
        return None;
    }
    Some(entry.text().trim().to_string())
}

/// Run `op` in the background; the folder is shown again after it, and a step it made is
/// offered to undo under `done`.
fn run(
    ui: &Rc<Ui>,
    done: String,
    op: impl FnOnce(&Client) -> foldav_core::Result<Option<u64>> + Send + 'static,
) {
    let weak = Rc::downgrade(ui);
    glib::spawn_future_local(async move {
        let r = gio::spawn_blocking(move || op(core())).await;
        let Some(ui) = weak.upgrade() else { return };
        // After a failure too: what went through is there.
        reload(&ui);
        match r {
            Ok(Ok(Some(step))) => ui.toasts.add_toast(undo_toast(&done, step)),
            Ok(Ok(None)) => {}
            Ok(Err(e)) => ui.toasts.add_toast(note(&error_text(&e))),
            Err(_) => ui.toasts.add_toast(note(&gettext("The operation failed"))),
        }
    });
}

fn undo_toast(title: &str, step: u64) -> adw::Toast {
    let toast = note(title);
    toast.set_button_label(Some(&gettext("Undo")));
    toast.connect_button_clicked(move |_| undo(Some(step)));
    toast
}

/// Undo step `step`, the latest when None.
fn undo(step: Option<u64>) {
    glib::spawn_future_local(async move {
        let r = gio::spawn_blocking(move || core().undo(step)).await;
        let Some(ui) = ui() else { return };
        reload(&ui);
        match r {
            Ok(Ok(())) => {}
            Ok(Err(Error::NotFound(_))) => ui.toasts.add_toast(note(&gettext("Nothing to undo"))),
            Ok(Err(e)) => ui.toasts.add_toast(note(&error_text(&e))),
            Err(_) => ui.toasts.add_toast(note(&gettext("The operation failed"))),
        }
    });
}

fn new_folder(ui: &Rc<Ui>) {
    let Some((id, dir)) = ui.at.borrow().clone() else {
        return;
    };
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let initial = gettext("Untitled Folder");
        let Some(name) = ask_name(
            &ui,
            &gettext("New Folder"),
            &gettext("Create"),
            &initial,
            -1,
        )
        .await
        else {
            return;
        };
        run(&ui, String::new(), move |c| {
            c.new_folder(id, &dir, &name).map(|()| None)
        });
    });
}

fn rename(ui: &Rc<Ui>) {
    let (at, mut files) = (ui.at.borrow().clone(), selected(ui));
    let (Some((id, _)), Some(file), 1) = (at, files.pop(), files.len() + 1) else {
        return;
    };
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        // A file's name without its extension is selected, as in Files.
        let select = match file.name.rfind('.') {
            Some(i) if !file.folder && i > 0 => file.name[..i].chars().count() as i32,
            _ => -1,
        };
        let Some(name) = ask_name(
            &ui,
            &gettext("Rename"),
            &gettext("Rename"),
            &file.name,
            select,
        )
        .await
        else {
            return;
        };
        if name != file.name {
            run(&ui, gettext("Renamed"), move |c| {
                c.rename(id, &file.path, &name)
            });
        }
    });
}

fn copy(ui: &Rc<Ui>, cut: bool) {
    let Some((account, _)) = ui.at.borrow().clone() else {
        return;
    };
    let paths: Vec<String> = selected(ui).into_iter().map(|f| f.path).collect();
    if !paths.is_empty() {
        *ui.clipboard.borrow_mut() = Some(Clipboard {
            account,
            paths,
            cut,
        });
        update_actions(ui);
    }
}

fn paste(ui: &Rc<Ui>) {
    let Some((id, dest)) = ui.at.borrow().clone() else {
        return;
    };
    let clip = ui.clipboard.borrow_mut().take_if(|c| c.account == id);
    let Some(Clipboard { paths, cut, .. }) = clip else {
        return;
    };
    if cut {
        run(ui, gettext("Moved"), move |c| c.move_to(id, &paths, &dest));
    } else {
        // Pasted again later: the copy stays on the clipboard.
        *ui.clipboard.borrow_mut() = Some(Clipboard {
            account: id,
            paths: paths.clone(),
            cut: false,
        });
        run(ui, String::new(), move |c| {
            c.copy_to(id, &paths, &dest).map(|()| None)
        });
    }
    update_actions(ui);
}

fn delete(ui: &Rc<Ui>) {
    let Some((id, _)) = ui.at.borrow().clone() else {
        return;
    };
    let paths: Vec<String> = selected(ui).into_iter().map(|f| f.path).collect();
    if !paths.is_empty() {
        run(ui, gettext("Moved to Trash"), move |c| c.delete(id, &paths));
    }
}
