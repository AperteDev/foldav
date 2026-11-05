//! Tagging a file: the tags of the server in a list to filter, a check for each the file has,
//! and a way to make a new one.

use super::{Ui, note};
use crate::core;
use crate::i18n::gettext;
use adw::prelude::*;
use foldav_core::File;
use foldav_core::places::Tag;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

/// Open the dialog that sets the tags of `file` of account `account`.
pub fn present(ui: &Rc<Ui>, account: i64, file: File) {
    let search = gtk::SearchEntry::builder()
        .placeholder_text(gettext("Search or create a tag"))
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(6)
        .build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(12)
        .valign(gtk::Align::Start)
        .build();
    let create = gtk::Button::builder()
        .halign(gtk::Align::Center)
        .css_classes(["pill"])
        .margin_bottom(12)
        .visible(false)
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();
    let inside = gtk::Box::new(gtk::Orientation::Vertical, 0);
    inside.append(&list);
    inside.append(&create);
    scroll.set_child(Some(&inside));
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.append(&search);
    body.append(&scroll);
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&body));
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&toasts));
    let dialog = adw::Dialog::builder()
        .title(gettext("Tags of “{name}”").replace("{name}", &file.name))
        .content_width(420)
        .content_height(560)
        .child(&view)
        .build();

    // The tag of each row, by name for the filter.
    let tags: Rc<RefCell<Vec<Tag>>> = Rc::default();
    let have: Rc<RefCell<HashSet<String>>> = Rc::default();

    let fill = {
        let (list, tags, have, toasts, file) = (
            list.clone(),
            tags.clone(),
            have.clone(),
            toasts.clone(),
            file.clone(),
        );
        Rc::new(move || {
            let (list, tags, have, toasts, file) = (
                list.clone(),
                tags.clone(),
                have.clone(),
                toasts.clone(),
                file.clone(),
            );
            glib::spawn_future_local(async move {
                let f = file.clone();
                let r = gio::spawn_blocking(move || {
                    Ok::<_, foldav_core::Error>((
                        core().tags(account)?,
                        core().file_tags(account, &f)?,
                    ))
                })
                .await;
                let (all, mine) = match r {
                    Ok(Ok(r)) => r,
                    Ok(Err(e)) => return toasts.add_toast(note(&e.to_string())),
                    Err(_) => return,
                };
                *have.borrow_mut() = mine.into_iter().map(|t| t.id).collect();
                list.remove_all();
                for tag in &all {
                    let row = adw::ActionRow::builder()
                        .title(glib::markup_escape_text(&tag.name))
                        .activatable(true)
                        .build();
                    let check = gtk::CheckButton::builder()
                        .active(have.borrow().contains(&tag.id))
                        .valign(gtk::Align::Center)
                        .build();
                    row.add_prefix(&check);
                    row.set_activatable_widget(Some(&check));
                    let (t, f, toasts) = (tag.clone(), file.clone(), toasts.clone());
                    check.connect_toggled(move |c| {
                        let (on, t, f, toasts) =
                            (c.is_active(), t.clone(), f.clone(), toasts.clone());
                        glib::spawn_future_local(async move {
                            let r = gio::spawn_blocking(move || {
                                core().set_file_tag(account, &f, &t.id, on)
                            })
                            .await;
                            if let Ok(Err(e)) = r {
                                toasts.add_toast(note(&e.to_string()));
                            }
                        });
                    });
                    list.append(&row);
                }
                *tags.borrow_mut() = all;
                list.invalidate_filter();
            });
        })
    };
    fill();

    // The filter shows the tags with what is typed in their names.
    let (s, t) = (search.clone(), tags.clone());
    list.set_filter_func(move |row| {
        let q = s.text().to_lowercase();
        let tags = t.borrow();
        let i = row.index() as usize;
        q.is_empty()
            || tags
                .get(i)
                .is_some_and(|t| t.name.to_lowercase().contains(&q))
    });
    let (l, c, t) = (list.clone(), create.clone(), tags.clone());
    search.connect_search_changed(move |s| {
        l.invalidate_filter();
        // A new tag can be made of what is typed, when no tag has that name.
        let q = s.text().trim().to_string();
        let exists = t
            .borrow()
            .iter()
            .any(|t| t.name.to_lowercase() == q.to_lowercase());
        c.set_visible(!q.is_empty() && !exists);
        c.set_label(&gettext("Create Tag “{name}”").replace("{name}", &q));
    });
    let (s, file, toasts, fill) = (search.clone(), file.clone(), toasts.clone(), fill.clone());
    create.connect_clicked(move |b| {
        let name = s.text().trim().to_string();
        let (f, toasts, fill, s, b) = (
            file.clone(),
            toasts.clone(),
            fill.clone(),
            s.clone(),
            b.clone(),
        );
        glib::spawn_future_local(async move {
            let r = gio::spawn_blocking(move || {
                let tag = core().create_tag(account, &name)?;
                core().set_file_tag(account, &f, &tag.id, true)
            })
            .await;
            match r {
                Ok(Err(e)) => toasts.add_toast(note(&e.to_string())),
                _ => {
                    s.set_text("");
                    b.set_visible(false);
                    fill();
                }
            }
        });
    });
    dialog.present(Some(&ui.window));
}
