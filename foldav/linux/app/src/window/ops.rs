//! File operations in the window: open, download, new folder, rename, cut, copy and paste,
//! delete, and undo. Each is run in the background, listed in the header while it takes long
//! (`transfers.rs`); the changes to the server are one request each (`foldav_core` `ops.rs`), and
//! the list is loaded again after them. A right click on the files opens the same actions as a
//! menu.

use super::nav::reload;
use super::{Ui, action, nav, note, popup_menu, toast, transfers, ui};
use crate::core;
use crate::i18n::{gettext, ngettext};
use adw::prelude::*;
use foldav_core::places;
use foldav_core::{Client, Error, File, Saved, Taken};
use gtk::{gio, glib};
use std::path::{Path, PathBuf};
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
    action(ui, "open", open_selected);
    action(ui, "open-with", |ui| {
        if let [file] = selected(ui).as_slice() {
            open_in(ui, file.clone(), true);
        }
    });
    action(ui, "download", |ui| download(ui, false));
    action(ui, "download-zip", |ui| download(ui, true));
    action(ui, "upload-files", |ui| pick(ui, false));
    action(ui, "upload-folder", |ui| pick(ui, true));
    action(ui, "new-folder", new_folder);
    action(ui, "rename", rename);
    action(ui, "cut", |ui| copy(ui, true));
    action(ui, "copy", |ui| copy(ui, false));
    action(ui, "paste", paste);
    action(ui, "delete", delete);
    action(ui, "open-browser", |ui| {
        with_file(ui, |ui, id, file| match core().web_url(id, &file) {
            Ok(url) => {
                gtk::UriLauncher::new(&url).launch(
                    Some(&ui.window),
                    gio::Cancellable::NONE,
                    |_| {},
                );
            }
            Err(e) => ui.toasts.add_toast(note(&error_text(&e))),
        });
    });
    action(ui, "share", |ui| with_file(ui, super::share::present));
    action(ui, "versions", |ui| with_file(ui, super::versions::present));
    action(ui, "tags", |ui| with_file(ui, super::tags::present));
    action(ui, "restore", restore);
    action(ui, "delete-forever", delete_forever);
    action(ui, "empty-trash", empty_trash);
    action(ui, "undo", |_| undo(None));
    update_actions(ui);
}

pub fn accels(app: &adw::Application) {
    app.set_accels_for_action("win.open", &["<Control>o"]);
    app.set_accels_for_action("win.upload-files", &["<Control>u"]);
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
    // A place is a list of files, no folder to put files in; in the trash bin they cannot be
    // used, only restored or deleted for good.
    let place = ui
        .at
        .borrow()
        .as_ref()
        .is_some_and(|(_, p)| places::is_place(p));
    let trash = ui
        .at
        .borrow()
        .as_ref()
        .is_some_and(|(_, p)| p == places::TRASH);
    // What the server has: sharing, versions of a file, tags.
    let server = at
        .and_then(|id| ui.server.borrow().get(&id).map(|s| s.features.clone()))
        .unwrap_or_default();
    let one_file = n == 1 && selected(ui).first().is_some_and(|f| !f.folder);
    let any_file = selected(ui).iter().any(|f| !f.folder);
    let paste = ui
        .clipboard
        .borrow()
        .as_ref()
        .is_some_and(|c| Some(c.account) == at);
    for (name, on) in [
        ("open", (n == 1 || any_file) && !trash),
        ("open-with", one_file && !trash),
        ("download", n > 0 && !trash),
        ("download-zip", n > 0 && !place),
        ("upload-files", at.is_some() && !place),
        ("upload-folder", at.is_some() && !place),
        ("new-folder", at.is_some() && !place),
        ("rename", n == 1 && !trash),
        ("cut", n > 0 && !trash),
        ("copy", n > 0 && !trash),
        ("delete", n > 0 && !trash),
        ("paste", paste && !place),
        ("open-browser", n == 1 && !trash),
        ("share", n == 1 && !trash && server.sharing),
        ("versions", one_file && !trash && server.versions),
        ("tags", n == 1 && !trash && server.tags),
        ("restore", n > 0 && trash),
        ("delete-forever", n > 0 && trash),
        ("empty-trash", trash),
    ] {
        ui.window
            .lookup_action(name)
            .and_downcast::<gio::SimpleAction>()
            .inspect(|a| a.set_enabled(on));
    }
    super::favorites::update_actions(ui);
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

/// Run `op` in the background, listed as `busy` if it takes long; the folder is shown again
/// after it, and a step it made is offered to undo under `done`.
fn run(
    ui: &Rc<Ui>,
    busy: String,
    done: String,
    op: impl FnOnce(&Client) -> foldav_core::Result<Option<u64>> + Send + 'static,
) {
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let r = transfers::run(&ui, &busy, false, move |c, _| op(c)).await;
        // After a failure too: what went through is there.
        reload(&ui);
        match r {
            Some(Ok(Some(step))) => ui.toasts.add_toast(undo_toast(&done, step)),
            Some(Ok(None)) => {}
            Some(Err(e)) => ui.toasts.add_toast(note(&error_text(&e))),
            None => ui.toasts.add_toast(note(&gettext("The operation failed"))),
        }
    });
}

/// How often the copies of opened files are looked at for changes to send back.
const WATCH: u32 = 5;
/// After a copy could not be sent, how long before it is tried again.
const RETRY: std::time::Duration = std::time::Duration::from_secs(60);

thread_local! {
    /// Not before: a change is being sent, or failed lately.
    static NOT_BEFORE: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
}

/// Send back what other apps changed in the copies they opened (also what was changed while
/// Foldav was closed), and say what became of it.
pub fn watch_copies(ui: &Rc<Ui>) {
    let weak = Rc::downgrade(ui);
    glib::timeout_add_seconds_local(WATCH, move || {
        let Some(ui) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let ready = NOT_BEFORE
            .get()
            .is_none_or(|t| t <= std::time::Instant::now());
        if ready {
            // Taken for the time it takes: no second one starts meanwhile.
            NOT_BEFORE.set(Some(std::time::Instant::now() + 24 * RETRY));
            glib::spawn_future_local(save_changes(ui));
        }
        glib::ControlFlow::Continue
    });
}

async fn save_changes(ui: Rc<Ui>) {
    let changed = gio::spawn_blocking(|| core().has_changes())
        .await
        .unwrap_or(false);
    let mut failed = false;
    if changed {
        let title = gettext("Saving changes");
        let r = transfers::run(&ui, &title, true, |c, p| Ok(c.save_changes(p))).await;
        reload(&ui);
        for (name, saved) in r.and_then(Result::ok).unwrap_or_default() {
            match saved {
                Ok(Saved::Uploaded) => {
                    toast(&gettext("Saved “{name}” to Nextcloud").replace("{name}", &name))
                }
                Ok(Saved::Conflict(kept)) => ui.toasts.add_toast(note(
                    &gettext("“{name}” changed on the server: your version is saved as “{kept}”")
                        .replace("{name}", &name)
                        .replace("{kept}", &kept),
                )),
                Err(Error::Cancelled) => failed = true,
                Err(e) => {
                    failed = true;
                    ui.toasts.add_toast(note(
                        &gettext("Cannot save “{name}”: {error}")
                            .replace("{name}", &name)
                            .replace("{error}", &error_text(&e)),
                    ));
                }
            }
        }
    }
    // A failed one is tried again after a while, not every few seconds.
    NOT_BEFORE.set(failed.then(|| std::time::Instant::now() + RETRY));
}

/// Open a folder in the window, a file in its default app, from a copy kept on this device.
pub fn open(ui: &Rc<Ui>, file: File) {
    open_in(ui, file, false);
}

/// Open the selected files, each in its default app (a folder only when it is the one thing).
fn open_selected(ui: &Rc<Ui>) {
    match selected(ui).as_slice() {
        [one] => open(ui, one.clone()),
        many => many
            .iter()
            .filter(|f| !f.folder)
            .for_each(|f| open(ui, f.clone())),
    }
}

/// As `open`; with `ask`, the user picks the app (Open With…).
fn open_in(ui: &Rc<Ui>, file: File, ask: bool) {
    let Some((id, _)) = ui.at.borrow().clone() else {
        return;
    };
    if file.folder {
        return nav::open(ui, id, file.path);
    }
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let title = gettext("Opening “{name}”").replace("{name}", &file.name);
        let r = transfers::run(&ui, &title, true, move |c, p| c.open_file(id, &file, p)).await;
        match r {
            Some(Ok(copy)) => launch(&ui, &copy, ask),
            Some(Err(Error::Cancelled)) => {}
            Some(Err(e)) => ui.toasts.add_toast(note(&error_text(&e))),
            None => ui.toasts.add_toast(note(&gettext("The operation failed"))),
        }
    });
}

pub(super) fn launch(ui: &Ui, path: &Path, ask: bool) {
    let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(path)));
    launcher.set_always_ask(ask);
    launcher.launch(Some(&ui.window), gio::Cancellable::NONE, |r| {
        if let Err(e) = r
            && !e.matches(gtk::DialogError::Dismissed)
        {
            toast(&gettext("Cannot open the file: {error}").replace("{error}", e.message()));
        }
    });
}

/// Save the selected files and folders into a folder the user picks; with `zip`, as one ZIP
/// the server makes of them.
fn download(ui: &Rc<Ui>, zip: bool) {
    let (at, files) = (ui.at.borrow().clone(), selected(ui));
    let (Some((id, parent)), false) = (at, files.is_empty()) else {
        return;
    };
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let dialog = gtk::FileDialog::builder()
            .title(gettext("Download to"))
            .accept_label(gettext("Download"))
            .build();
        if let Some(dir) = glib::user_special_dir(glib::UserDirectory::Downloads) {
            dialog.set_initial_folder(Some(&gio::File::for_path(dir)));
        }
        // Dismissed, or no folder picked.
        let Ok(folder) = dialog.select_folder_future(Some(&ui.window)).await else {
            return;
        };
        let Some(dest) = folder.path() else {
            return toast(&gettext("Cannot save to this folder"));
        };
        let title = match files.as_slice() {
            [f] => gettext("Downloading “{name}”").replace("{name}", &f.name),
            _ => gettext("Downloading the selected files"),
        };
        let r = transfers::run(&ui, &title, true, move |c, p| {
            if zip {
                c.download_zip(id, &parent, &files, &dest, p).map(|_| ())
            } else {
                c.download(id, &files, &dest, p)
            }
        })
        .await;
        match r {
            Some(Ok(())) => toast(&gettext("Downloaded")),
            Some(Err(Error::Cancelled)) => {}
            Some(Err(e)) => ui.toasts.add_toast(note(&error_text(&e))),
            None => ui.toasts.add_toast(note(&gettext("The operation failed"))),
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
        run(&ui, gettext("Creating folder"), String::new(), move |c| {
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
            run(&ui, gettext("Renaming"), gettext("Renamed"), move |c| {
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
    if !cut {
        // Pasted again later: the copy stays on the clipboard.
        *ui.clipboard.borrow_mut() = Some(Clipboard {
            account: id,
            paths: paths.clone(),
            cut: false,
        });
    }
    put(ui, id, paths, dest, cut);
    update_actions(ui);
}

/// Move (`cut`) or copy the files of `paths` into folder `dest`, all of account `id`.
/// What is already there under the same name is asked about first (what is copied into its own
/// folder is simply named "a (copy)").
pub fn put(ui: &Rc<Ui>, id: i64, paths: Vec<String>, dest: String, cut: bool) {
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let names: Vec<String> = paths
            .iter()
            .filter(|p| nav::parent(p) != dest)
            .map(|p| p.rsplit('/').next().unwrap_or(p).to_string())
            .collect();
        let d = dest.clone();
        let taken = if names.is_empty() {
            Vec::new()
        } else {
            match gio::spawn_blocking(move || core().taken(id, &d, &names)).await {
                Ok(Ok(t)) => t,
                Ok(Err(e)) => return ui.toasts.add_toast(note(&error_text(&e))),
                Err(_) => return,
            }
        };
        let policy = if taken.is_empty() {
            Taken::KeepBoth
        } else {
            let Some(policy) = ask_taken(&ui, &taken).await else {
                return;
            };
            policy
        };
        let (busy, done) = if cut {
            (gettext("Moving"), gettext("Moved"))
        } else {
            (gettext("Copying"), String::new())
        };
        run(&ui, busy, done, move |c| {
            c.paste_into(id, &paths, &dest, cut, policy)
        });
    });
}

// --- uploads -----------------------------------------------------------------

/// Pick files (or a folder) on this device and upload them into the folder shown.
fn pick(ui: &Rc<Ui>, folder: bool) {
    let Some((id, dest)) = ui.at.borrow().clone() else {
        return;
    };
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        // For tests of the sandbox copy, which cannot drive the dialog: the files it names
        // (separated by `:`) are picked.
        if crate::sandbox()
            && let Ok(picked) = std::env::var("FOLDAV_SANDBOX_PICK")
        {
            return upload(
                &ui,
                id,
                dest,
                picked.split(':').map(PathBuf::from).collect(),
            );
        }
        let dialog = gtk::FileDialog::builder()
            .title(if folder {
                gettext("Upload Folder")
            } else {
                gettext("Upload Files")
            })
            .accept_label(gettext("Upload"))
            .build();
        let items: Vec<gio::File> = if folder {
            dialog
                .select_folder_future(Some(&ui.window))
                .await
                .into_iter()
                .collect()
        } else {
            match dialog.open_multiple_future(Some(&ui.window)).await {
                Ok(list) => list.iter::<gio::File>().flatten().collect(),
                Err(_) => return,
            }
        };
        upload(
            &ui,
            id,
            dest,
            items.iter().filter_map(|f| f.path()).collect(),
        );
    });
}

/// What to do with the names taken in the folder: None when the upload is called off.
async fn ask_taken(ui: &Ui, taken: &[String]) -> Option<Taken> {
    let heading = match taken {
        [name] => gettext("“{name}” exists already").replace("{name}", name),
        _ => ngettext(
            "{n} item exists already",
            "{n} items exist already",
            taken.len() as u64,
        )
        .replace("{n}", &taken.len().to_string()),
    };
    let alert = adw::AlertDialog::new(
        Some(&heading),
        Some(&gettext("Replace what is there, keep both, or skip it?")),
    );
    alert.add_responses(&[
        ("cancel", &gettext("Cancel")),
        ("skip", &gettext("Skip")),
        ("keep", &gettext("Keep Both")),
        ("replace", &gettext("Replace")),
    ]);
    alert.set_response_appearance("replace", adw::ResponseAppearance::Destructive);
    alert.set_default_response(Some("keep"));
    alert.set_close_response("cancel");
    match alert.choose_future(Some(&ui.window)).await.as_str() {
        "skip" => Some(Taken::Skip),
        "keep" => Some(Taken::KeepBoth),
        "replace" => Some(Taken::Replace),
        _ => None,
    }
}

/// Upload the files and folders `items` of this device into folder `dest` of account `id`,
/// asking first what to do with the names that are taken.
pub fn upload(ui: &Rc<Ui>, id: i64, dest: String, items: Vec<PathBuf>) {
    if items.is_empty() {
        return toast(&gettext("Only files on this device can be uploaded"));
    }
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let names: Vec<String> = items
            .iter()
            .filter_map(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .collect();
        let (d, n) = (dest.clone(), names.clone());
        let taken = match gio::spawn_blocking(move || core().taken(id, &d, &n)).await {
            Ok(Ok(t)) => t,
            Ok(Err(e)) => return ui.toasts.add_toast(note(&error_text(&e))),
            Err(_) => return ui.toasts.add_toast(note(&gettext("The operation failed"))),
        };
        let policy = if taken.is_empty() {
            Taken::KeepBoth
        } else {
            let Some(policy) = ask_taken(&ui, &taken).await else {
                return;
            };
            policy
        };
        let title = match names.as_slice() {
            [name] => gettext("Uploading “{name}”").replace("{name}", name),
            _ => gettext("Uploading files"),
        };
        let r = transfers::run(&ui, &title, true, move |c, p| {
            c.upload(id, &dest, &items, policy, p)
        })
        .await;
        // After a failure too: what went through is there.
        reload(&ui);
        match r {
            Some(Ok(())) => toast(&gettext("Uploaded")),
            Some(Err(Error::Cancelled)) => {}
            Some(Err(e)) => ui.toasts.add_toast(note(&error_text(&e))),
            None => ui.toasts.add_toast(note(&gettext("The operation failed"))),
        }
    });
}

/// Files dropped (or pasted) from another place of this window into folder `dest` of account
/// `id`: moved, or copied. False when they cannot go there.
pub fn drop_files(
    ui: &Rc<Ui>,
    id: i64,
    from: i64,
    files: &[File],
    dest: String,
    cut: bool,
) -> bool {
    if from != id {
        // To another account: copied, by way of this device.
        let (ui, files) = (ui.clone(), files.to_vec());
        glib::spawn_future_local(async move {
            let title = gettext("Copying to another account");
            let r = transfers::run(&ui, &title, true, move |c, p| {
                c.copy_across(from, &files, id, &dest, Taken::KeepBoth, p)
            })
            .await;
            match r {
                Some(Ok(())) => toast(&gettext("Copied to the other account")),
                Some(Err(Error::Cancelled)) => {}
                Some(Err(e)) => ui.toasts.add_toast(note(&error_text(&e))),
                None => ui.toasts.add_toast(note(&gettext("The operation failed"))),
            }
        });
        return true;
    }
    // What is there already, or would go into itself, is left as it is.
    let paths: Vec<String> = files
        .iter()
        .map(|f| f.path.clone())
        .filter(|p| nav::parent(p) != dest || !cut)
        .filter(|p| *p != dest)
        .collect();
    if !paths.is_empty() {
        put(ui, id, paths, dest, cut);
    }
    true
}

/// Run `f` for the one file selected, with its account.
fn with_file(ui: &Rc<Ui>, f: impl FnOnce(&Rc<Ui>, i64, File)) {
    let (at, mut files) = (ui.at.borrow().clone(), selected(ui));
    if let (Some((id, _)), Some(file), 1) = (at, files.pop(), files.len() + 1) {
        f(ui, id, file);
    }
}

/// Ask before something that cannot be taken back; true when the user agrees.
async fn confirm(ui: &Ui, heading: &str, body: &str, ok: &str) -> bool {
    let alert = adw::AlertDialog::new(Some(heading), Some(body));
    alert.add_responses(&[("cancel", &gettext("Cancel")), ("ok", ok)]);
    alert.set_response_appearance("ok", adw::ResponseAppearance::Destructive);
    alert.set_close_response("cancel");
    alert.choose_future(Some(&ui.window)).await == "ok"
}

/// The files selected in the trash bin: their account and paths.
fn in_trash(ui: &Ui) -> Option<(i64, Vec<String>)> {
    let (id, path) = ui.at.borrow().clone()?;
    let paths: Vec<String> = selected(ui).into_iter().map(|f| f.path).collect();
    (path == places::TRASH && !paths.is_empty()).then_some((id, paths))
}

fn restore(ui: &Rc<Ui>) {
    if let Some((id, paths)) = in_trash(ui) {
        run(ui, gettext("Restoring"), String::new(), move |c| {
            c.trash_restore(id, &paths).map(|()| None)
        });
    }
}

fn delete_forever(ui: &Rc<Ui>) {
    let Some((id, paths)) = in_trash(ui) else {
        return;
    };
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let heading = ngettext(
            "Delete {n} item permanently?",
            "Delete {n} items permanently?",
            paths.len() as u64,
        )
        .replace("{n}", &paths.len().to_string());
        if confirm(
            &ui,
            &heading,
            &gettext("It cannot be restored."),
            &gettext("Delete"),
        )
        .await
        {
            run(&ui, gettext("Deleting"), String::new(), move |c| {
                c.trash_delete(id, &paths).map(|()| None)
            });
        }
    });
}

fn empty_trash(ui: &Rc<Ui>) {
    let Some((id, _)) = ui.at.borrow().clone() else {
        return;
    };
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let body = gettext("All the items in the trash will be deleted permanently.");
        if confirm(
            &ui,
            &gettext("Empty the trash?"),
            &body,
            &gettext("Empty Trash"),
        )
        .await
        {
            run(
                &ui,
                gettext("Emptying the trash"),
                String::new(),
                move |c| c.trash_empty(id).map(|()| None),
            );
        }
    });
}

fn delete(ui: &Rc<Ui>) {
    let Some((id, _)) = ui.at.borrow().clone() else {
        return;
    };
    let paths: Vec<String> = selected(ui).into_iter().map(|f| f.path).collect();
    if !paths.is_empty() {
        run(
            ui,
            gettext("Moving to Trash"),
            gettext("Moved to Trash"),
            move |c| c.delete(id, &paths),
        );
    }
}
