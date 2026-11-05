//! Sharing a file or folder, as the web app's sidebar does: its public links (with a password
//! and an expiry date, to copy or take down) and the users and groups it is shared with, and a
//! way to add more.

use super::{Ui, note};
use crate::core;
use crate::i18n::gettext;
use adw::prelude::*;
use foldav_core::{File, ShareInfo, ShareKind, Sharee, perm};
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

struct Dialog {
    account: i64,
    file: File,
    dialog: adw::Dialog,
    toasts: adw::ToastOverlay,
    links: adw::PreferencesGroup,
    people: adw::PreferencesGroup,
    found: adw::PreferencesGroup,
    /// What the groups hold now, to take out again.
    rows: RefCell<Vec<(adw::PreferencesGroup, gtk::Widget)>>,
    password: adw::PasswordEntryRow,
    expires: adw::EntryRow,
    editable: adw::SwitchRow,
}

/// Run `f` away from the window; what it returns, None if it panicked.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    gio::spawn_blocking(f).await.ok()
}

fn describe(s: &ShareInfo) -> String {
    let mut parts = Vec::new();
    parts.push(if s.permissions & perm::UPDATE != 0 {
        gettext("Can edit")
    } else {
        gettext("Can view")
    });
    if s.has_password {
        parts.push(gettext("Password protected"));
    }
    if let Some(d) = &s.expires {
        parts.push(gettext("Expires {date}").replace("{date}", d));
    }
    parts.join(" · ")
}

impl Dialog {
    fn error(&self, e: &foldav_core::Error) {
        self.toasts.add_toast(note(&e.to_string()));
    }

    fn clear(&self) {
        for (group, row) in self.rows.take() {
            group.remove(&row);
        }
    }

    fn add(&self, group: &adw::PreferencesGroup, row: &impl IsA<gtk::Widget>) {
        group.add(row);
        self.rows
            .borrow_mut()
            .push((group.clone(), row.clone().upcast()));
    }
}

/// Fill the dialog with the shares there are now.
fn refresh(d: &Rc<Dialog>) {
    let d = d.clone();
    glib::spawn_future_local(async move {
        let (id, path) = (d.account, d.file.path.clone());
        let Some(r) = blocking(move || core().file_shares(id, &path)).await else {
            return;
        };
        let shares = match r {
            Ok(s) => s,
            Err(e) => return d.error(&e),
        };
        d.clear();
        // A heading with nothing under it is left out.
        let links = shares.iter().filter(|s| s.kind == ShareKind::Link).count();
        d.links.set_visible(links > 0);
        d.people.set_visible(shares.len() > links);
        for s in &shares {
            if s.kind == ShareKind::Link {
                link_row(&d, s);
            } else {
                person_row(&d, s);
            }
        }
    });
}

fn delete_button(d: &Rc<Dialog>, share: &ShareInfo) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .tooltip_text(gettext("Stop Sharing"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    let (d, id) = (d.clone(), share.id.clone());
    button.connect_clicked(move |b| {
        b.set_sensitive(false);
        let (d, id) = (d.clone(), id.clone());
        glib::spawn_future_local(async move {
            let account = d.account;
            match blocking(move || core().delete_share(account, &id)).await {
                Some(Err(e)) => d.error(&e),
                _ => refresh(&d),
            }
        });
    });
    button
}

fn link_row(d: &Rc<Dialog>, s: &ShareInfo) {
    let title = if s.label.is_empty() {
        gettext("Public Link")
    } else {
        s.label.clone()
    };
    let row = adw::ActionRow::builder()
        .title(glib::markup_escape_text(&title))
        .subtitle(glib::markup_escape_text(&describe(s)))
        .build();
    let url = s.url.clone().unwrap_or_default();
    let copy = gtk::Button::builder()
        .icon_name("edit-copy-symbolic")
        .tooltip_text(gettext("Copy Link"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    let toasts = d.toasts.clone();
    copy.connect_clicked(move |b| {
        b.display().clipboard().set_text(&url);
        toasts.add_toast(note(&gettext("Link copied")));
    });
    row.add_suffix(&copy);
    row.add_suffix(&delete_button(d, s));
    d.add(&d.links, &row);
}

fn person_row(d: &Rc<Dialog>, s: &ShareInfo) {
    let kind = if s.kind == ShareKind::Group {
        gettext("Group")
    } else {
        gettext("User")
    };
    let row = adw::ActionRow::builder()
        .title(glib::markup_escape_text(&s.label))
        .subtitle(glib::markup_escape_text(&format!(
            "{kind} · {}",
            describe(s)
        )))
        .build();
    let edit = gtk::Switch::builder()
        .active(s.permissions & perm::UPDATE != 0)
        .valign(gtk::Align::Center)
        .tooltip_text(gettext("Can edit"))
        .build();
    let (dialog, id, permissions) = (d.clone(), s.id.clone(), s.permissions);
    edit.connect_state_set(move |_, on| {
        let (d, id) = (dialog.clone(), id.clone());
        let p = if on {
            permissions | perm::UPDATE
        } else {
            permissions & !perm::UPDATE
        };
        glib::spawn_future_local(async move {
            let account = d.account;
            match blocking(move || core().update_share(account, &id, Some(p), None, None)).await {
                Some(Err(e)) => d.error(&e),
                _ => refresh(&d),
            }
        });
        glib::Propagation::Proceed
    });
    row.add_suffix(&edit);
    row.add_suffix(&delete_button(d, s));
    d.add(&d.people, &row);
}

/// The users and groups matching what was typed, as rows to pick one from.
fn search(d: &Rc<Dialog>, query: String) {
    let d = d.clone();
    glib::spawn_future_local(async move {
        let account = d.account;
        let q = query.clone();
        let found = match blocking(move || core().find_sharees(account, &q)).await {
            Some(Ok(f)) => f,
            Some(Err(e)) => return d.error(&e),
            None => return,
        };
        d.clear_found();
        d.found.set_visible(true);
        if found.is_empty() {
            let none = adw::ActionRow::builder()
                .title(gettext("Nobody found"))
                .css_classes(["dim-label"])
                .build();
            d.add(&d.found, &none);
        }
        for s in found {
            sharee_row(&d, s);
        }
    });
}

impl Dialog {
    fn clear_found(&self) {
        let mut rows = self.rows.borrow_mut();
        rows.retain(|(g, r)| {
            if *g == self.found {
                g.remove(r);
                false
            } else {
                true
            }
        });
    }
}

fn sharee_row(d: &Rc<Dialog>, s: Sharee) {
    let kind = if s.kind == ShareKind::Group {
        gettext("Group")
    } else {
        gettext("User")
    };
    let row = adw::ActionRow::builder()
        .title(glib::markup_escape_text(&s.name))
        .subtitle(kind)
        .activatable(true)
        .build();
    row.add_suffix(&gtk::Image::from_icon_name("list-add-symbolic"));
    let dialog = d.clone();
    row.connect_activated(move |_| {
        let (d, s) = (dialog.clone(), s.clone());
        glib::spawn_future_local(async move {
            let (account, path) = (d.account, d.file.path.clone());
            let r = blocking(move || {
                core().share_with(account, &path, s.kind, &s.id, perm::READ | perm::SHARE)
            })
            .await;
            d.clear_found();
            d.found.set_visible(false);
            match r {
                Some(Err(e)) => d.error(&e),
                _ => refresh(&d),
            }
        });
    });
    d.add(&d.found, &row);
}

/// Open the dialog that shares `file` of account `account`.
pub fn present(ui: &Rc<Ui>, account: i64, file: File) {
    let links = adw::PreferencesGroup::builder()
        .title(gettext("Public Links"))
        .visible(false)
        .build();
    let people = adw::PreferencesGroup::builder()
        .title(gettext("People"))
        .visible(false)
        .build();
    let found = adw::PreferencesGroup::builder().visible(false).build();
    let password = adw::PasswordEntryRow::builder()
        .title(gettext("Password (optional)"))
        .build();
    let expires = adw::EntryRow::builder()
        .title(gettext("Expires on, as 2026-12-31 (optional)"))
        .build();
    let editable = adw::SwitchRow::builder()
        .title(gettext("Allow editing"))
        .build();
    let create = gtk::Button::builder()
        .label(gettext("Create Public Link"))
        .halign(gtk::Align::Center)
        .css_classes(["pill", "suggested-action"])
        .build();
    let new_link = adw::PreferencesGroup::builder()
        .title(gettext("New Public Link"))
        .build();
    for row in [
        password.upcast_ref::<gtk::Widget>(),
        expires.upcast_ref(),
        editable.upcast_ref(),
    ] {
        new_link.add(row);
    }
    let with = adw::EntryRow::builder()
        .title(gettext("Share with a user or group"))
        .show_apply_button(true)
        .build();
    let share_with = adw::PreferencesGroup::builder()
        .title(gettext("Add People"))
        .build();
    share_with.add(&with);

    let page = adw::PreferencesPage::new();
    for g in [&links, &people, &share_with, &found, &new_link] {
        page.add(g);
    }
    let buttons = adw::PreferencesGroup::new();
    buttons.add(&create);
    page.add(&buttons);

    let toasts = adw::ToastOverlay::new();
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    toasts.set_child(Some(&page));
    view.set_content(Some(&toasts));
    let dialog = adw::Dialog::builder()
        .title(gettext("Share “{name}”").replace("{name}", &file.name))
        .content_width(480)
        .content_height(640)
        .child(&view)
        .build();
    let d = Rc::new(Dialog {
        account,
        file,
        dialog: dialog.clone(),
        toasts,
        links,
        people,
        found,
        rows: RefCell::default(),
        password,
        expires,
        editable,
    });

    let dd = d.clone();
    with.connect_apply(move |entry| {
        let q = entry.text().trim().to_string();
        if !q.is_empty() {
            search(&dd, q);
        }
    });
    let dd = d.clone();
    create.connect_clicked(move |b| {
        b.set_sensitive(false);
        let (d, b) = (dd.clone(), b.clone());
        glib::spawn_future_local(async move {
            let (account, path) = (d.account, d.file.path.clone());
            let (edit, pw, until) = (
                d.editable.is_active(),
                d.password.text().to_string(),
                d.expires.text().trim().to_string(),
            );
            let r =
                blocking(move || core().share_link(account, &path, edit, Some(&pw), Some(&until)))
                    .await;
            b.set_sensitive(true);
            match r {
                Some(Ok(link)) => {
                    d.password.set_text("");
                    d.expires.set_text("");
                    // The link is what is wanted: on the clipboard at once, as the web app does.
                    if let Some(url) = &link.url {
                        d.dialog.display().clipboard().set_text(url);
                        d.toasts.add_toast(note(&gettext("Link copied")));
                    }
                    refresh(&d);
                }
                Some(Err(e)) => d.error(&e),
                None => {}
            }
        });
    });
    refresh(&d);
    dialog.present(Some(&ui.window));
}
