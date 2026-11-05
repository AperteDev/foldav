//! Dragging files, as Files does. Out of the window to other apps (Files, the desktop): the
//! files are downloaded when the other side asks for them, listed in the sidebar like any
//! transfer. Inside it: onto a folder in the views, the sidebar or the path bar, a move, a copy
//! with Ctrl. In from other apps: an upload into the folder dropped on, or shown.

use super::{Ui, ops, transfers, ui};
use crate::core;
use crate::i18n::gettext;
use adw::prelude::*;
use foldav_core::File;
use gtk::{gdk, gio, glib};
use std::path::PathBuf;
use std::rc::{Rc, Weak};

/// Files dragged inside the app: what the drop targets take without going through the other
/// side's format.
#[derive(Clone, glib::Boxed)]
#[boxed_type(name = "FoldavDraggedFiles")]
pub struct Dragged {
    account: i64,
    files: Vec<File>,
}

/// The files being dragged inside the window, while they are.
struct Drag {
    account: i64,
    paths: Vec<String>,
    /// All of them are folders.
    folder: bool,
}

thread_local! {
    static DRAGGING: std::cell::RefCell<Option<Drag>> = const { std::cell::RefCell::new(None) };
}

/// Dropping on `dest` makes no sense for what is dragged: on one of the files itself, in a
/// folder dragged (it would go into itself), or, for a move (`copy_only` false), where the files
/// are already.
fn refuses(dest: &(i64, String), copy_only: bool) -> bool {
    DRAGGING.with_borrow(|d| {
        d.as_ref().is_some_and(|d| {
            d.account == dest.0
                && (d
                    .paths
                    .iter()
                    .any(|p| *p == dest.1 || dest.1.starts_with(&format!("{p}/")))
                    || (!copy_only && d.paths.iter().all(|p| super::nav::parent(p) == dest.1)))
        })
    })
}

/// What shows by the pointer while files are dragged: a rounded badge with an icon for a file, a
/// folder or several, and their number.
fn drag_badge() -> Option<gtk::Widget> {
    let (n, folder) = DRAGGING.with_borrow(|d| d.as_ref().map(|d| (d.paths.len(), d.folder)))?;
    let icon = gtk::Image::builder()
        .icon_name(match (n, folder) {
            (1, true) => "folder-symbolic",
            (1, false) => "text-x-generic-symbolic",
            _ => "edit-copy-symbolic",
        })
        .pixel_size(32)
        .css_classes(["drag-badge"])
        .build();
    let overlay = gtk::Overlay::builder()
        .child(&icon)
        // Beside the pointer, not under it.
        .margin_start(10)
        .margin_top(10)
        .build();
    if n > 1 {
        overlay.add_overlay(
            &gtk::Label::builder()
                .label(n.to_string())
                .halign(gtk::Align::End)
                .valign(gtk::Align::Start)
                .css_classes(["drag-count"])
                .build(),
        );
    }
    Some(overlay.upcast())
}

/// What was dropped (files of the window, or of another app) goes into folder `dest` of account
/// `id`: moved or, with `cut` false, copied; from another app, uploaded. A place takes none.
pub fn drop_value(ui: &Rc<Ui>, value: &glib::Value, id: i64, dest: String, cut: bool) -> bool {
    if foldav_core::places::is_place(&dest) {
        return false;
    }
    if let Ok(dragged) = value.get::<Dragged>() {
        return ops::drop_files(ui, id, dragged.account, &dragged.files, dest, cut);
    }
    match value.get::<gdk::FileList>() {
        Ok(list) => {
            let items: Vec<PathBuf> = list.files().iter().filter_map(|f| f.path()).collect();
            ops::upload(ui, id, dest, items);
            true
        }
        Err(_) => false,
    }
}

mod provider {
    use super::*;
    use gdk::subclass::prelude::*;
    use std::cell::RefCell;
    use std::future::Future;
    use std::pin::Pin;

    mod imp {
        use super::*;

        #[derive(Default)]
        pub struct RemoteFiles {
            pub files: RefCell<(i64, Vec<File>)>,
        }

        #[glib::object_subclass]
        impl ObjectSubclass for RemoteFiles {
            const NAME: &'static str = "FoldavRemoteFiles";
            type Type = super::RemoteFiles;
            type ParentType = gdk::ContentProvider;
        }

        impl ObjectImpl for RemoteFiles {}

        impl ContentProviderImpl for RemoteFiles {
            fn formats(&self) -> gdk::ContentFormats {
                gdk::ContentFormats::new(&["text/uri-list"])
            }

            fn write_mime_type_future(
                &self,
                _mime_type: &str,
                stream: &gio::OutputStream,
                io_priority: glib::Priority,
            ) -> Pin<Box<dyn Future<Output = Result<(), glib::Error>> + 'static>> {
                let (account, files) = self.files.borrow().clone();
                let stream = stream.clone();
                Box::pin(async move {
                    let uris = super::super::fetch(account, files)
                        .await
                        .map_err(|e| glib::Error::new(gio::IOErrorEnum::Failed, &e))?;
                    stream
                        .write_all_future(uris.into_bytes(), io_priority)
                        .await
                        .map(|_| ())
                        .map_err(|(_, e)| e)
                })
            }
        }
    }

    glib::wrapper! {
        /// The files of the server as a list of addresses of copies on this device, made when
        /// another app asks for it.
        pub struct RemoteFiles(ObjectSubclass<imp::RemoteFiles>) @extends gdk::ContentProvider;
    }

    impl RemoteFiles {
        pub fn new(account: i64, files: Vec<File>) -> RemoteFiles {
            let p: RemoteFiles = glib::Object::new();
            *p.imp().files.borrow_mut() = (account, files);
            p
        }
    }
}

/// Download `files` into a folder of this device and say where they are, as the lines of a
/// `text/uri-list`.
async fn fetch(account: i64, files: Vec<File>) -> Result<String, String> {
    let ui = ui().ok_or_else(|| gettext("The window is closed"))?;
    let dir = gio::spawn_blocking(|| core().scratch())
        .await
        .map_err(|_| gettext("The operation failed"))?
        .map_err(|e| e.to_string())?;
    let title = match files.as_slice() {
        [f] => gettext("Downloading “{name}”").replace("{name}", &f.name),
        _ => gettext("Downloading the selected files"),
    };
    let d = dir.clone();
    match transfers::run(&ui, &title, true, move |c, p| {
        c.download(account, &files, &d, p)
    })
    .await
    {
        Some(Ok(())) => {}
        Some(Err(e)) => return Err(e.to_string()),
        None => return Err(gettext("The operation failed")),
    }
    let uris: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| gio::File::for_path(e.path()).uri().to_string())
        .collect();
    Ok(uris.join("\r\n") + "\r\n")
}

/// Make the item at `w` (of `item`'s row) a source of the selected files, and a target of what
/// is dropped on it: into the folder it is, else into the folder shown.
pub fn attach(ui: &Weak<Ui>, item: &gtk::ListItem, w: &gtk::Widget) {
    let source = gtk::DragSource::builder()
        .actions(gdk::DragAction::COPY | gdk::DragAction::MOVE)
        .build();
    let (weak, row) = (ui.clone(), item.downgrade());
    source.connect_prepare(move |source, _, _| {
        let (ui, item) = (weak.upgrade()?, row.upgrade()?);
        // What is dragged is selected: the row, unless it is part of the selection.
        let widget = source.widget()?;
        let model = super::views::model_of(&widget)?;
        if !model.is_selected(item.position()) {
            model.select_item(item.position(), true);
        }
        let (files, account) = (
            ui.views.selected(),
            ui.at.borrow().as_ref().map(|(id, _)| *id)?,
        );
        if files.is_empty() {
            return None;
        }
        DRAGGING.set(Some(Drag {
            account,
            paths: files.iter().map(|f| f.path.clone()).collect(),
            folder: files.iter().all(|f| f.folder),
        }));
        Some(gdk::ContentProvider::new_union(&[
            gdk::ContentProvider::for_value(
                &Dragged {
                    account,
                    files: files.clone(),
                }
                .to_value(),
            ),
            provider::RemoteFiles::new(account, files).upcast(),
        ]))
    });
    // What is dragged is a small badge by the pointer, as Files', not the row.
    source.connect_drag_begin(|_, drag| {
        if let Some(badge) = drag_badge()
            && let Ok(icon) = gtk::DragIcon::for_drag(drag).downcast::<gtk::DragIcon>()
        {
            icon.set_child(Some(&badge));
        }
    });
    source.connect_drag_end(|_, _, _| {
        DRAGGING.take();
    });
    w.add_controller(source);

    let row = item.downgrade();
    w.add_controller(target(ui, true, move |ui| {
        let at = ui.at.borrow().clone()?;
        // A folder takes what is dropped on it; a file's room is the folder it is in.
        match row.upgrade()?.item().map(|o| super::views::file_of(&o)) {
            Some(f) if f.folder => Some((at.0, f.path)),
            _ => Some(at),
        }
    }));
}

/// A target for files dropped, going to the folder `dest` says (its account and path). With
/// `highlight` the widget shows it is the one the files would go into.
pub fn target(
    ui: &Weak<Ui>,
    highlight: bool,
    dest: impl Fn(&Ui) -> Option<(i64, String)> + 'static,
) -> gtk::DropTarget {
    let target = gtk::DropTarget::new(
        glib::Type::INVALID,
        gdk::DragAction::COPY | gdk::DragAction::MOVE,
    );
    target.set_types(&[Dragged::static_type(), gdk::FileList::static_type()]);
    // Files of the window move (Ctrl: copy; the compositor turns that into the action it
    // gives); what comes from another app is copied, to the server.
    let (weak, dest) = (ui.clone(), std::rc::Rc::new(dest));
    // Ctrl held: the drop offers copying only.
    let copy_only = |t: &gtk::DropTarget| {
        t.current_drop()
            .is_some_and(|d| !d.actions().contains(gdk::DragAction::MOVE))
    };
    // What the window drags is no drop here when it makes no sense (on itself).
    let (w, d) = (weak.clone(), dest.clone());
    target.connect_motion(move |t, _, _| {
        let ours = t
            .current_drop()
            .is_some_and(|d| d.formats().contains_type(Dragged::static_type()));
        if !ours {
            return gdk::DragAction::COPY;
        }
        let refused = w
            .upgrade()
            .and_then(|ui| d(&ui))
            .is_some_and(|dest| refuses(&dest, copy_only(t)));
        if refused {
            gdk::DragAction::empty()
        } else {
            gdk::DragAction::MOVE
        }
    });
    let d = dest.clone();
    target.connect_drop(move |t, value, _, _| {
        let Some(ui) = weak.upgrade() else {
            return false;
        };
        let Some((id, dest)) = d(&ui) else {
            return false;
        };
        let cut = !copy_only(t);
        if refuses(&(id, dest.clone()), !cut) {
            return false;
        }
        drop_value(&ui, value, id, dest, cut)
    });
    if highlight {
        // Only a folder: the room of the folder shown needs no mark.
        let (weak, d) = (ui.clone(), dest);
        target.connect_current_drop_notify(move |t| {
            let Some(w) = t.widget() else { return };
            let over = t.current_drop().is_some()
                && weak.upgrade().is_some_and(|ui| {
                    d(&ui).is_some_and(|dest| {
                        Some(&dest) != ui.at.borrow().as_ref() && !refuses(&dest, copy_only(t))
                    })
                });
            if over {
                w.add_css_class("drop-target");
            } else {
                w.remove_css_class("drop-target");
            }
        });
    }
    target
}
