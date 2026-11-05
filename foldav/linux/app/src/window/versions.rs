//! The versions of a file the server keeps: each can be opened (from a copy kept for it) or put
//! back as the file.

use super::{Ui, nav::reload, note, ops, transfers};
use crate::core;
use crate::i18n::gettext;
use adw::prelude::*;
use foldav_core::{File, Version};
use gtk::{gio, glib};
use std::rc::Rc;

/// "5 Nov 2026, 14:03".
fn when(t: i64) -> String {
    glib::DateTime::from_unix_local(t)
        .and_then(|d| d.format("%x %H:%M:%S"))
        .map(|d| d.to_string())
        .unwrap_or_default()
}

/// Open the dialog with the versions of `file` of account `account`.
pub fn present(ui: &Rc<Ui>, account: i64, file: File) {
    let group = adw::PreferencesGroup::new();
    let status = adw::StatusPage::builder()
        .icon_name("document-open-recent-symbolic")
        .title(gettext("No Earlier Versions"))
        .description(gettext(
            "The server keeps a version each time the file is changed.",
        ))
        .visible(false)
        .build();
    let page = adw::PreferencesPage::new();
    page.add(&group);
    let toasts = adw::ToastOverlay::new();
    let stack = gtk::Box::new(gtk::Orientation::Vertical, 0);
    stack.append(&page);
    stack.append(&status);
    toasts.set_child(Some(&stack));
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&toasts));
    let dialog = adw::Dialog::builder()
        .title(gettext("Versions of “{name}”").replace("{name}", &file.name))
        .content_width(460)
        .content_height(520)
        .child(&view)
        .build();

    let (window, ui, f) = (ui.window.clone(), ui.clone(), file.clone());
    glib::spawn_future_local({
        let dialog = dialog.clone();
        async move {
            let fl = f.clone();
            let versions = gio::spawn_blocking(move || core().versions(account, &fl)).await;
            let versions = match versions {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => return toasts.add_toast(note(&e.to_string())),
                Err(_) => return,
            };
            page.set_visible(!versions.is_empty());
            status.set_visible(versions.is_empty());
            for v in versions {
                let row = adw::ActionRow::builder()
                    .title(when(v.modified))
                    .subtitle(glib::format_size(v.size))
                    .build();
                let open = gtk::Button::builder()
                    .label(gettext("Open"))
                    .valign(gtk::Align::Center)
                    .build();
                let restore = gtk::Button::builder()
                    .label(gettext("Restore"))
                    .valign(gtk::Align::Center)
                    .build();
                let (u, ff, vv) = (ui.clone(), f.clone(), v.clone());
                open.connect_clicked(move |_| open_version(&u, account, ff.clone(), vv.clone()));
                let (u, ff, d, t) = (ui.clone(), f.clone(), dialog.clone(), toasts.clone());
                restore.connect_clicked(move |b| {
                    b.set_sensitive(false);
                    let (u, ff, vv, d, t, b) = (
                        u.clone(),
                        ff.clone(),
                        v.clone(),
                        d.clone(),
                        t.clone(),
                        b.clone(),
                    );
                    glib::spawn_future_local(async move {
                        let r =
                            gio::spawn_blocking(move || core().restore_version(account, &ff, &vv))
                                .await;
                        match r {
                            Ok(Ok(())) => {
                                reload(&u);
                                d.close();
                            }
                            Ok(Err(e)) => {
                                b.set_sensitive(true);
                                t.add_toast(note(&e.to_string()));
                            }
                            Err(_) => b.set_sensitive(true),
                        }
                    });
                });
                row.add_suffix(&open);
                row.add_suffix(&restore);
                group.add(&row);
            }
        }
    });
    dialog.present(Some(&window));
}

/// Fetch a version into a folder of its own and open it in its app.
fn open_version(ui: &Rc<Ui>, account: i64, file: File, version: Version) {
    let ui = ui.clone();
    glib::spawn_future_local(async move {
        let dir = match gio::spawn_blocking(|| core().scratch()).await {
            Ok(Ok(d)) => d,
            _ => return,
        };
        let title = gettext("Opening “{name}”").replace("{name}", &file.name);
        let r = transfers::run(&ui, &title, true, move |c, p| {
            c.save_version(account, &file, &version, &dir, p)
        })
        .await;
        match r {
            Some(Ok(path)) => ops::launch(&ui, &path, false),
            Some(Err(foldav_core::Error::Cancelled)) => {}
            Some(Err(e)) => ui.toasts.add_toast(note(&e.to_string())),
            None => {}
        }
    });
}
