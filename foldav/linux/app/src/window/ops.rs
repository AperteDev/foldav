//! File operations in the window: new folder, rename, cut, copy and paste, delete, and undo.
//! Each is one request to the server (`foldav_core` `ops.rs`), run in the background; the list
//! is loaded again after it. A right click on the files opens the same actions as a menu.

use super::{Ui, note, reload, ui};
use crate::core;
use crate::i18n::gettext;
use adw::prelude::*;
use foldav_core::{Client, Error, File};
use gtk::{gdk, gio, glib};
use std::rc::Rc;

/// Files cut or copied, to paste into a folder of the same account.
pub struct Clipboard {
    account: i64,
    paths: Vec<String>,
    cut: bool,
}

/// The selected files, in the order of the list.
fn selected(ui: &Ui) -> Vec<File> {
    let listing = ui.listing.borrow();
    let mut rows: Vec<i32> = ui.files.selected_rows().iter().map(|r| r.index()).collect();
    rows.sort_unstable();
    rows.iter()
        .filter_map(|&i| listing.get(i as usize).cloned())
        .collect()
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

type Action = fn(&Rc<Ui>);

/// The actions (keys in `accels`) and the list's selection and right click.
pub fn connect(ui: &Rc<Ui>) {
    let actions: [(&str, Action); 7] = [
        ("new-folder", new_folder),
        ("rename", rename),
        ("cut", |ui| copy(ui, true)),
        ("copy", |ui| copy(ui, false)),
        ("paste", paste),
        ("delete", delete),
        ("undo", |_| undo(None)),
    ];
    for (name, f) in actions {
        let a = gio::SimpleAction::new(name, None);
        let weak = Rc::downgrade(ui);
        a.connect_activate(move |_, _| {
            // The keys are the text field's while one has the focus (a name being typed).
            if let Some(ui) = weak.upgrade()
                && !GtkWindowExt::focus(&ui.window).is_some_and(|w| w.is::<gtk::Editable>())
            {
                f(&ui);
            }
        });
        ui.window.add_action(&a);
    }
    update_actions(ui);

    let weak = Rc::downgrade(ui);
    ui.files.connect_selected_rows_changed(move |_| {
        if let Some(ui) = weak.upgrade() {
            update_actions(&ui);
        }
    });
    let weak = Rc::downgrade(ui);
    let click = gtk::GestureClick::builder().button(3).build();
    click.connect_pressed(move |_, _, x, y| {
        if let Some(ui) = weak.upgrade() {
            context_menu(&ui, x, y);
        }
    });
    ui.files.add_controller(click);
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

/// The menu at the pointer; the row under it is selected unless it is already.
fn context_menu(ui: &Rc<Ui>, x: f64, y: f64) {
    match ui.files.row_at_y(y as i32) {
        Some(row) if !row.is_selected() => {
            ui.files.unselect_all();
            ui.files.select_row(Some(&row));
        }
        Some(_) => {}
        None => ui.files.unselect_all(),
    }
    let popover = gtk::PopoverMenu::from_model(Some(&ui.file_menu));
    popover.set_parent(&ui.files);
    popover.set_has_arrow(false);
    popover.set_halign(gtk::Align::Start);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    popover.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    popover.popup();
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
