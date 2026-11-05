//! The folder's files in one of three views, as Files has them: icons, a list (with sortable
//! columns), and columns (`columns.rs`). Icons and list share one model and selection, so
//! switching keeps what is selected; the columns have a model each.

use super::columns::Columns;
use super::{Ui, columns, nav, ops, options};
use crate::core;
use crate::i18n::{gettext, ngettext};
use adw::prelude::*;
use foldav_core::File;
use gtk::{gdk, gio, glib};
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
    pub(super) list: gtk::ColumnView,
    stack: gtk::Stack,
    icons_scroll: gtk::ScrolledWindow,
    list_scroll: gtk::ScrolledWindow,
    /// What is selected, at the foot of the files.
    status: gtk::Label,
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
    thread_local! {
        /// The icons looked up, by type, scale and size: scrolling binds the same few over and
        /// over.
        static ICONS: std::cell::RefCell<std::collections::HashMap<(String, i32, i32), gtk::IconPaintable>> =
            std::cell::RefCell::default();
    }
    // Looked up larger for the big icons (and their thumbnails).
    let size = if image.pixel_size() > 128 { 256 } else { 128 };
    match gtk::gdk::Display::default() {
        Some(display) => {
            let scale = image.scale_factor();
            let key = (
                if f.folder {
                    "inode/directory".to_string()
                } else {
                    f.mime.clone()
                },
                scale,
                size,
            );
            let paintable = ICONS.with(|c| {
                c.borrow_mut()
                    .entry(key)
                    .or_insert_with(|| {
                        gtk::IconTheme::for_display(&display).lookup_by_gicon(
                            &icon(f),
                            size,
                            scale,
                            gtk::TextDirection::None,
                            gtk::IconLookupFlags::FORCE_REGULAR,
                        )
                    })
                    .clone()
            });
            image.set_paintable(Some(&paintable));
        }
        None => image.set_from_gicon(&icon(f)),
    }
    image.set_opacity(if f.hidden() { 0.5 } else { 1.0 });
    super::thumbs::apply(image, f, size as u32);
}

impl Views {
    pub fn new(b: &gtk::Builder) -> Views {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        // Dragging a rectangle on the room between the files selects the ones it touches.
        let list = gtk::ColumnView::builder()
            .css_classes(["files-view"])
            .enable_rubberband(true)
            .build();
        // The files come sorted (`options`): a click on the list's header only tells the sort,
        // which is the menu's too, so the sorter of its columns sorts nothing (it is for
        // their arrows) and the three views are in one order.
        let selection = gtk::MultiSelection::new(Some(store.clone()));
        let grid = gtk::GridView::builder()
            .max_columns(100)
            .css_classes(["files-view"])
            .enable_rubberband(true)
            .build();
        grid.set_model(Some(&selection));
        list.set_model(Some(&selection));

        for (i, (id, title)) in [
            ("name", gettext("Name")),
            ("size", gettext("Size")),
            ("modified", gettext("Modified")),
        ]
        .into_iter()
        .enumerate()
        {
            let column = gtk::ColumnViewColumn::new(Some(&title), None::<gtk::ListItemFactory>);
            column.set_id(Some(id));
            column.set_expand(i == 0);
            column.set_resizable(true);
            column.set_sorter(Some(&gtk::CustomSorter::new(|_, _| Ordering::Equal.into())));
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
            status: super::get(b, "selection_status"),
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
            .spacing(2)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(6)
            .margin_end(6)
            .halign(gtk::Align::Center)
            .build();
        // The icon, and the star of a favorite at its corner.
        let overlay = icon_with_star(48);
        overlay.set_margin_bottom(4);
        b.append(&overlay);
        b.append(
            &gtk::Label::builder()
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .lines(2)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .justify(gtk::Justification::Center)
                .build(),
        );
        // The captions beneath the name (`options`), as many as the icons' size has room for.
        for _ in 0..options::CAPTIONS {
            b.append(
                &gtk::Label::builder()
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .justify(gtk::Justification::Center)
                    .css_classes(["dim-label", "caption"])
                    .visible(false)
                    .build(),
            );
        }
        item.set_child(Some(&b));
        attach_menu(&w, item, b.upcast_ref());
    });
    let w = weak.clone();
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
        let (Some(f), Some(b), Some(ui)) =
            (item.item().map(|o| file_of(&o)), item.child(), w.upgrade())
        else {
            return;
        };
        let (px, captions) = (ui.options.grid_px(), ui.options.captions());
        let width = (px + 40).max(100);
        b.set_width_request(width);
        let icon = b.first_child();
        let name = icon.as_ref().and_then(|o| o.next_sibling());
        if let (Some((image, star)), Some(label)) = (
            icon.as_ref().and_then(parts),
            name.clone().and_downcast::<gtk::Label>(),
        ) {
            image.set_pixel_size(px);
            set_icon(&image, &f);
            set_star(&star, f.favorite, px);
            label.set_max_width_chars(width / 9);
            label.set_label(&f.name);
        }
        let mut next = name.and_then(|n| n.next_sibling());
        for slot in 0..options::CAPTIONS {
            let Some(label) = next.and_downcast_ref::<gtk::Label>() else {
                break;
            };
            let text = captions.get(slot).map(|c| c.text(&f)).unwrap_or_default();
            label.set_max_width_chars(width / 9);
            label.set_label(&text);
            label.set_visible(!text.is_empty());
            next = label.next_sibling();
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
        let w = weak.clone();
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
            let (Some(f), Some(b), Some(ui)) =
                (item.item().map(|o| file_of(&o)), item.child(), w.upgrade())
            else {
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
                        image.set_pixel_size(ui.options.list_px());
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

    // A click on the room between the files selects none (Ctrl and Shift click: no change; a
    // drag of a rectangle claims the click, which then ends without a release).
    for scroll in [&v.icons_scroll, &v.list_scroll] {
        let click = gtk::GestureClick::builder().button(1).build();
        let w = weak.clone();
        click.connect_released(move |g, _, x, y| {
            let (Some(ui), Some(widget)) = (w.upgrade(), g.widget()) else {
                return;
            };
            let plain = !g
                .current_event_state()
                .intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK);
            if plain && on_room(&widget, x, y) {
                ui.views.selection.unselect_all();
            }
        });
        scroll.add_controller(click);
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

/// What `scroll` has at `x`, `y` is the room between the files: not a file's tile or row, the
/// list's header or a scroll bar.
fn on_room(scroll: &gtk::Widget, x: f64, y: f64) -> bool {
    let mut w = scroll.pick(x, y, gtk::PickFlags::DEFAULT);
    while let Some(widget) = w {
        if widget == *scroll {
            return true;
        }
        if matches!(
            widget.css_name().as_str(),
            "child" | "row" | "header" | "scrollbar" | "button"
        ) {
            return false;
        }
        w = widget.parent();
    }
    true
}

/// What is selected, said at the foot of the files as Files says it: one file by its name and
/// size, several by their number and together size, folders apart.
pub fn update_status(ui: &Ui) {
    // Not in the columns: a click there opens a folder or shows a file in the preview.
    let files = if ui.views.mode() == Mode::Columns {
        Vec::new()
    } else {
        ui.views.selected()
    };
    let text = status_text(&files);
    ui.views.status.set_visible(text.is_some());
    if let Some(text) = text {
        ui.views.status.set_label(&text);
    }
}

fn status_text(files: &[File]) -> Option<String> {
    let first = files.first()?;
    let size = files
        .iter()
        .any(|f| f.size.is_some())
        .then(|| glib::format_size(files.iter().filter_map(|f| f.size).sum()).to_string());
    let with_size = |what: String| match &size {
        Some(size) => gettext("{what} ({size})")
            .replace("{what}", &what)
            .replace("{size}", size),
        None => what,
    };
    if files.len() == 1 {
        return Some(with_size(
            gettext("“{name}” selected").replace("{name}", &first.name),
        ));
    }
    let folders = files.iter().filter(|f| f.folder).count() as u64;
    let others = files.len() as u64 - folders;
    let folders_text = ngettext("{n} folder selected", "{n} folders selected", folders)
        .replace("{n}", &folders.to_string());
    let others_text = |plural_of_all: bool| {
        if plural_of_all {
            ngettext("{n} item selected", "{n} items selected", others)
        } else {
            ngettext(
                "{n} other item selected",
                "{n} other items selected",
                others,
            )
        }
        .replace("{n}", &others.to_string())
    };
    Some(with_size(match (folders, others) {
        (0, _) => others_text(true),
        (_, 0) => folders_text,
        _ => gettext("{folders}, {others}")
            .replace("{folders}", &folders_text)
            .replace("{others}", &others_text(false)),
    }))
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
    // A middle click on a folder opens it in a new tab.
    let row = item.downgrade();
    super::tabs::middle_click(ui, w, move |ui| {
        let (id, _) = ui.at.borrow().clone()?;
        let f = row.upgrade()?.item().map(|o| file_of(&o))?;
        f.folder.then_some((id, f.path))
    });
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
    options::update(ui);
}

/// A file's icon with the star of a favorite at its corner, as the icons have it (the web app's
/// too). `parts` gives the two back; `set_star` shows or hides the star.
pub fn icon_with_star(icon_px: i32) -> gtk::Overlay {
    let overlay = gtk::Overlay::builder()
        .child(
            &gtk::Image::builder()
                .pixel_size(icon_px)
                .valign(gtk::Align::Center)
                .build(),
        )
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    overlay.add_overlay(&star(star_px(icon_px)));
    overlay
}

/// The icon and the star of what `icon_with_star` made.
pub fn parts(widget: &gtk::Widget) -> Option<(gtk::Image, gtk::Image)> {
    let overlay = widget.downcast_ref::<gtk::Overlay>()?;
    Some((
        overlay.child().and_downcast::<gtk::Image>()?,
        overlay.last_child().and_downcast::<gtk::Image>()?,
    ))
}

/// The star, as big as its icon's size says.
fn star_px(icon_px: i32) -> i32 {
    (icon_px / 4).clamp(12, 32)
}

/// Show the star of `overlay` (from `icon_with_star`) if `favorite`, sized for the icon.
pub fn set_star(star: &gtk::Image, favorite: bool, icon_px: i32) {
    star.set_pixel_size(star_px(icon_px));
    star.set_visible(favorite);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, folder: bool, size: Option<u64>) -> File {
        File {
            name: name.into(),
            path: name.into(),
            folder,
            size,
            modified: 0,
            mime: String::new(),
            favorite: false,
            has_preview: false,
            etag: String::new(),
            fileid: String::new(),
        }
    }

    #[test]
    fn the_status_says_what_is_selected() {
        assert_eq!(status_text(&[]), None);
        let (a, b, d) = (
            file("a", false, Some(1000)),
            file("b", false, Some(2000)),
            file("d", true, Some(500)),
        );
        let said = |files: &[File]| status_text(files).unwrap();
        assert!(said(std::slice::from_ref(&a)).starts_with("“a” selected"));
        assert!(said(&[a.clone(), b.clone()]).starts_with("2 items selected (3"));
        assert!(said(&[d.clone(), d.clone()]).starts_with("2 folders selected"));
        assert!(said(&[d, a, b]).starts_with("1 folder selected, 2 other items selected"));
        // No size known (a place's files): none said.
        assert_eq!(
            said(&[file("x", false, None), file("y", false, None)]),
            "2 items selected"
        );
    }
}
