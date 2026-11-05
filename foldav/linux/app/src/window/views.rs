//! The folder's files in one of three views, as Files has them: icons, a list (with sortable
//! columns), and columns (`columns.rs`). Icons and list share one model and selection, so
//! switching keeps what is selected; the columns have a model each.

use super::columns::Columns;
use super::{Ui, columns, nav, ops};
use crate::core;
use crate::i18n::gettext;
use adw::prelude::*;
use foldav_core::File;
use gtk::{gio, glib};
use std::cell::Cell;
use std::cmp::Ordering;
use std::rc::{Rc, Weak};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Icons,
    List,
    Columns,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Icons => "icons",
            Mode::List => "list",
            Mode::Columns => "columns",
        }
    }

    pub fn from_name(name: &str) -> Mode {
        match name {
            "list" => Mode::List,
            "columns" => Mode::Columns,
            _ => Mode::Icons,
        }
    }
}

pub struct Views {
    /// The files of the folder shown (`glib::BoxedAnyObject`s of `File`), for icons and list.
    store: gio::ListStore,
    selection: gtk::MultiSelection,
    grid: gtk::GridView,
    list: gtk::ColumnView,
    stack: gtk::Stack,
    icons_scroll: gtk::ScrolledWindow,
    list_scroll: gtk::ScrolledWindow,
    pub columns: Columns,
    mode: Cell<Mode>,
}

pub fn file_of(item: &glib::Object) -> File {
    item.downcast_ref::<glib::BoxedAnyObject>()
        .expect("files are boxed")
        .borrow::<File>()
        .clone()
}

/// The files selected in `selection`, in its order.
pub fn selected_in(selection: &gtk::MultiSelection) -> Vec<File> {
    (0..selection.n_items())
        .filter(|&i| selection.is_selected(i))
        .filter_map(|i| selection.item(i))
        .map(|o| file_of(&o))
        .collect()
}

pub fn icon(f: &File) -> gio::Icon {
    gio::content_type_get_icon(if f.folder {
        "inode/directory"
    } else if f.mime.is_empty() {
        "application/octet-stream"
    } else {
        &f.mime
    })
}

/// A file's icon as every view draws it: the full-colour one, dimmed for a hidden file (as
/// Files does). Looked up large and scaled down, as the extension "My Computer" does: at a
/// small size the theme gives a monochrome one for some icons (a folder's).
pub fn set_icon(image: &gtk::Image, f: &File) {
    match gtk::gdk::Display::default() {
        Some(display) => {
            let paintable = gtk::IconTheme::for_display(&display).lookup_by_gicon(
                &icon(f),
                128,
                image.scale_factor(),
                gtk::TextDirection::None,
                gtk::IconLookupFlags::FORCE_REGULAR,
            );
            image.set_paintable(Some(&paintable));
        }
        None => image.set_from_gicon(&icon(f)),
    }
    image.set_opacity(if f.hidden() { 0.5 } else { 1.0 });
}

fn sorter(by: impl Fn(&File, &File) -> Ordering + 'static) -> gtk::CustomSorter {
    gtk::CustomSorter::new(move |a, b| by(&file_of(a), &file_of(b)).into())
}

impl Views {
    pub fn new(b: &gtk::Builder) -> Views {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let list = gtk::ColumnView::builder()
            .css_classes(["files-view"])
            .build();
        // The list's header sorts the model icons and list share.
        let sorted = gtk::SortListModel::new(Some(store.clone()), list.sorter());
        let selection = gtk::MultiSelection::new(Some(sorted));
        let grid = gtk::GridView::builder()
            .max_columns(100)
            .css_classes(["files-view"])
            .build();
        grid.set_model(Some(&selection));
        list.set_model(Some(&selection));

        // Folders before files, whichever way; then the column's own order.
        type By = Box<dyn Fn(&File, &File) -> Ordering>;
        let columns: [(String, By); 3] = [
            (
                gettext("Name"),
                Box::new(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            ),
            (gettext("Size"), Box::new(|a, b| a.size.cmp(&b.size))),
            (
                gettext("Modified"),
                Box::new(|a, b| a.modified.cmp(&b.modified)),
            ),
        ];
        for (i, (title, by)) in columns.into_iter().enumerate() {
            let column = gtk::ColumnViewColumn::new(Some(&title), None::<gtk::ListItemFactory>);
            column.set_expand(i == 0);
            column.set_resizable(true);
            column.set_sorter(Some(&sorter(move |a, b| {
                (!a.folder).cmp(&!b.folder).then_with(|| by(a, b))
            })));
            list.append_column(&column);
        }

        let icons_scroll: gtk::ScrolledWindow = super::get(b, "icons_scroll");
        icons_scroll.set_child(Some(&grid));
        let list_scroll: gtk::ScrolledWindow = super::get(b, "list_scroll");
        list_scroll.set_child(Some(&list));
        Views {
            store,
            selection,
            grid,
            list,
            stack: super::get(b, "view_stack"),
            icons_scroll,
            list_scroll,
            columns: Columns::new(b),
            mode: Cell::new(Mode::Icons),
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode.get()
    }

    /// The files the operations are for.
    pub fn selected(&self) -> Vec<File> {
        if self.mode.get() == Mode::Columns {
            self.columns.selected()
        } else {
            selected_in(&self.selection)
        }
    }
}

/// The widgets of the three views, their factories and what a click and a key do.
pub fn connect(ui: &Rc<Ui>) {
    let v = &ui.views;
    let weak = Rc::downgrade(ui);

    // Icons.
    let factory = gtk::SignalListItemFactory::new();
    let w = weak.clone();
    factory.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
        let b = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(6)
            .margin_end(6)
            .halign(gtk::Align::Center)
            .width_request(108)
            .build();
        // The icon, and the star of a favorite at its corner.
        let overlay = gtk::Overlay::builder()
            .child(&gtk::Image::builder().pixel_size(64).build())
            .halign(gtk::Align::Center)
            .build();
        overlay.add_overlay(&star(16));
        b.append(&overlay);
        b.append(
            &gtk::Label::builder()
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .lines(2)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .max_width_chars(12)
                .justify(gtk::Justification::Center)
                .build(),
        );
        item.set_child(Some(&b));
        attach_menu(&w, item, b.upcast_ref());
    });
    factory.connect_bind(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
        let (Some(f), Some(b)) = (item.item().map(|o| file_of(&o)), item.child()) else {
            return;
        };
        let overlay = b.first_child().and_downcast::<gtk::Overlay>();
        if let (Some(image), Some(star), Some(label)) = (
            overlay
                .as_ref()
                .and_then(|o| o.child())
                .and_downcast::<gtk::Image>(),
            overlay.as_ref().and_then(|o| o.last_child()),
            b.last_child().and_downcast::<gtk::Label>(),
        ) {
            set_icon(&image, &f);
            star.set_visible(f.favorite);
            label.set_label(&f.name);
        }
        b.set_tooltip_text(Some(&f.name));
    });
    v.grid.set_factory(Some(&factory));

    // List: the name with its icon, the size, the date.
    let columns = v.list.columns();
    for i in 0..columns.n_items() {
        let Some(column) = columns.item(i).and_downcast::<gtk::ColumnViewColumn>() else {
            continue;
        };
        let factory = gtk::SignalListItemFactory::new();
        let w = weak.clone();
        factory.connect_setup(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
            let b = gtk::Box::builder().spacing(8).build();
            if i == 0 {
                b.append(&gtk::Image::builder().pixel_size(16).build());
            }
            let label = gtk::Label::builder()
                .xalign(if i == 1 { 1.0 } else { 0.0 })
                .hexpand(true)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build();
            if i > 0 {
                label.add_css_class("dim-label");
            }
            b.append(&label);
            if i == 0 {
                b.append(&star(12));
            }
            item.set_child(Some(&b));
            attach_menu(&w, item, b.upcast_ref());
        });
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
            let (Some(f), Some(b)) = (item.item().map(|o| file_of(&o)), item.child()) else {
                return;
            };
            // The name's cell is the icon, the name, the star; the others' just the text.
            let label = if i == 0 {
                b.first_child().and_then(|c| c.next_sibling())
            } else {
                b.last_child()
            }
            .and_downcast::<gtk::Label>();
            let Some(label) = label else { return };
            match i {
                0 => {
                    if let Some(image) = b.first_child().and_downcast::<gtk::Image>() {
                        set_icon(&image, &f);
                    }
                    if let Some(star) = b.last_child() {
                        star.set_visible(f.favorite);
                    }
                    label.set_label(&f.name);
                }
                1 => label.set_label(&f.size.map(glib::format_size).unwrap_or_default()),
                _ => label.set_label(&date(f.modified)),
            }
        });
        column.set_factory(Some(&factory));
    }

    // A double click or Enter opens a folder or a file.
    let w = weak.clone();
    v.grid.connect_activate(move |view, pos| {
        activate(&w, view.model(), pos);
    });
    let w = weak.clone();
    v.list.connect_activate(move |view, pos| {
        activate(&w, view.model(), pos);
    });
    let w = weak.clone();
    v.selection.connect_selection_changed(move |_, _, _| {
        if let Some(ui) = w.upgrade() {
            ops::update_actions(&ui);
        }
    });

    // Files dropped on the room between the files go into the folder shown.
    for scroll in [&v.icons_scroll, &v.list_scroll, &v.columns.scroll] {
        scroll.add_controller(super::dnd::target(&weak, false, |ui| {
            ui.at.borrow().clone()
        }));
    }

    // A right click on the room between the files.
    for scroll in [&v.icons_scroll, &v.list_scroll] {
        let click = gtk::GestureClick::builder().button(3).build();
        let w = weak.clone();
        click.connect_pressed(move |g, _, x, y| {
            let (Some(ui), Some(widget)) = (w.upgrade(), g.widget()) else {
                return;
            };
            ui.views.selection.unselect_all();
            ops::context_menu(&ui, &widget, x, y);
        });
        scroll.add_controller(click);
    }
    columns::connect(ui);
}

pub fn date(t: i64) -> String {
    glib::DateTime::from_unix_local(t)
        .and_then(|d| d.format("%x"))
        .map(|d| d.to_string())
        .unwrap_or_default()
}

/// The selection model of the list, grid or column view `w` is in.
pub fn model_of(w: &gtk::Widget) -> Option<gtk::SelectionModel> {
    w.ancestor(gtk::GridView::static_type())
        .and_downcast::<gtk::GridView>()
        .and_then(|v| v.model())
        .or_else(|| {
            w.ancestor(gtk::ListView::static_type())
                .and_downcast::<gtk::ListView>()
                .and_then(|v| v.model())
        })
        .or_else(|| {
            w.ancestor(gtk::ColumnView::static_type())
                .and_downcast::<gtk::ColumnView>()
                .and_then(|v| v.model())
        })
}

/// A right click on `item`'s widget `w` selects it, unless it is selected, and opens the menu.
pub fn attach_menu(ui: &Weak<Ui>, item: &gtk::ListItem, w: &gtk::Widget) {
    super::dnd::attach(ui, item, w);
    let click = gtk::GestureClick::builder().button(3).build();
    let (ui, item) = (ui.clone(), item.downgrade());
    click.connect_pressed(move |g, _, x, y| {
        let (Some(ui), Some(item), Some(widget)) = (ui.upgrade(), item.upgrade(), g.widget())
        else {
            return;
        };
        // Not the room behind it too.
        g.set_state(gtk::EventSequenceState::Claimed);
        if let Some(model) = model_of(&widget) {
            let pos = item.position();
            if !model.is_selected(pos) {
                model.select_item(pos, true);
            }
        }
        ops::context_menu(&ui, &widget, x, y);
    });
    w.add_controller(click);
}

/// Open the folder or file at `pos` of `model`.
pub fn activate(ui: &Weak<Ui>, model: Option<gtk::SelectionModel>, pos: u32) {
    if let (Some(ui), Some(file)) = (
        ui.upgrade(),
        model.and_then(|m| m.item(pos)).map(|o| file_of(&o)),
    ) {
        ops::open(&ui, file);
    }
}

/// The view for `mode`.
pub fn set_mode(ui: &Rc<Ui>, mode: Mode) {
    let v = &ui.views;
    v.mode.set(mode);
    v.stack.set_visible_child_name(mode.name());
    let _ = core().set_setting("view_mode".into(), mode.name().into());
    nav::show(ui);
    ops::update_actions(ui);
}

/// The star of a favorite, hidden until a file is one.
pub fn star(size: i32) -> gtk::Image {
    gtk::Image::builder()
        .icon_name("starred-symbolic")
        .pixel_size(size)
        .halign(gtk::Align::End)
        .valign(gtk::Align::Start)
        .visible(false)
        .css_classes(["favorite-star"])
        .build()
}

/// Show `files` in icons and list; what was selected, and is still there, stays selected.
pub fn show(ui: &Ui, files: &[File]) {
    let v = &ui.views;
    let kept: Vec<String> = selected_in(&v.selection)
        .into_iter()
        .map(|f| f.path)
        .collect();
    let items: Vec<glib::BoxedAnyObject> = files
        .iter()
        .cloned()
        .map(glib::BoxedAnyObject::new)
        .collect();
    v.store.splice(0, v.store.n_items(), &items);
    select_paths(&v.selection, &kept);
}

/// Select the files of `paths` in `selection`.
pub fn select_paths(selection: &gtk::MultiSelection, paths: &[String]) {
    if paths.is_empty() {
        return;
    }
    for i in 0..selection.n_items() {
        if selection
            .item(i)
            .is_some_and(|o| paths.contains(&file_of(&o).path))
        {
            selection.select_item(i, false);
        }
    }
}
