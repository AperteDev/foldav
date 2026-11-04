//! The folder's files in one of three views, as Files has them: icons, a list (with sortable
//! columns), and columns, one per folder from the root down, as the Files extension "My
//! Computer" has. Icons and list share one model and selection, so switching keeps what is
//! selected; the columns have a model each.

use super::{Ui, nav, ops};
use crate::core;
use crate::i18n::gettext;
use adw::prelude::*;
use foldav_core::File;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::rc::{Rc, Weak};
use std::time::Duration;

/// How wide a column of the columns view is.
const COLUMN_WIDTH: i32 = 280;

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

/// One column of the columns view: folder `path`, as it was filled.
struct Column {
    path: String,
    /// What it was filled from, and whether hidden files were in it.
    filled: Option<(Rc<Vec<File>>, bool)>,
    store: gio::ListStore,
    selection: gtk::MultiSelection,
    scroll: gtk::ScrolledWindow,
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
    columns_box: gtk::Box,
    columns_scroll: gtk::ScrolledWindow,
    columns: RefCell<Vec<Column>>,
    /// Set while the code selects, which is no choice of the user's.
    updating: Cell<bool>,
    mode: Cell<Mode>,
}

fn file_of(item: &glib::Object) -> File {
    item.downcast_ref::<glib::BoxedAnyObject>()
        .expect("files are boxed")
        .borrow::<File>()
        .clone()
}

/// The files selected in `selection`, in its order.
fn selected_in(selection: &gtk::MultiSelection) -> Vec<File> {
    (0..selection.n_items())
        .filter(|&i| selection.is_selected(i))
        .filter_map(|i| selection.item(i))
        .map(|o| file_of(&o))
        .collect()
}

fn icon(f: &File) -> gio::Icon {
    gio::content_type_get_icon(if f.folder {
        "inode/directory"
    } else if f.mime.is_empty() {
        "application/octet-stream"
    } else {
        &f.mime
    })
}

fn sorter(by: impl Fn(&File, &File) -> Ordering + 'static) -> gtk::CustomSorter {
    gtk::CustomSorter::new(move |a, b| by(&file_of(a), &file_of(b)).into())
}

impl Views {
    pub fn new(b: &gtk::Builder) -> Views {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let list = gtk::ColumnView::builder().build();
        // The list's header sorts the model icons and list share.
        let sorted = gtk::SortListModel::new(Some(store.clone()), list.sorter());
        let selection = gtk::MultiSelection::new(Some(sorted));
        let grid = gtk::GridView::builder().max_columns(100).build();
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
            columns_box: super::get(b, "columns"),
            columns_scroll: super::get(b, "columns_scroll"),
            columns: RefCell::default(),
            updating: Cell::new(false),
            mode: Cell::new(Mode::Icons),
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode.get()
    }

    /// The files the operations are for. Columns: those selected in the last column, else the
    /// folder shown, selected in the one before.
    pub fn selected(&self) -> Vec<File> {
        if self.mode.get() != Mode::Columns {
            return selected_in(&self.selection);
        }
        let columns = self.columns.borrow();
        let last = columns.last().map(|c| selected_in(&c.selection));
        match last {
            Some(files) if !files.is_empty() => files,
            _ => columns
                .len()
                .checked_sub(2)
                .and_then(|i| columns.get(i))
                .map(|c| selected_in(&c.selection))
                .unwrap_or_default(),
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
            .margin_top(10)
            .margin_bottom(10)
            .margin_start(8)
            .margin_end(8)
            .halign(gtk::Align::Center)
            .width_request(104)
            .build();
        b.append(&gtk::Image::builder().pixel_size(64).build());
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
        if let (Some(image), Some(label)) = (
            b.first_child().and_downcast::<gtk::Image>(),
            b.last_child().and_downcast::<gtk::Label>(),
        ) {
            image.set_from_gicon(&icon(&f));
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
            item.set_child(Some(&b));
            attach_menu(&w, item, b.upcast_ref());
        });
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
            let (Some(f), Some(b)) = (item.item().map(|o| file_of(&o)), item.child()) else {
                return;
            };
            let Some(label) = b.last_child().and_downcast::<gtk::Label>() else {
                return;
            };
            match i {
                0 => {
                    if let Some(image) = b.first_child().and_downcast::<gtk::Image>() {
                        image.set_from_gicon(&icon(&f));
                    }
                    label.set_label(&f.name);
                }
                1 => label.set_label(&f.size.map(glib::format_size).unwrap_or_default()),
                _ => label.set_label(&date(f.modified)),
            }
        });
        column.set_factory(Some(&factory));
    }

    // A double click or Enter opens a folder.
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

    // A right click on the room between the files.
    for scroll in [&v.icons_scroll, &v.list_scroll, &v.columns_scroll] {
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
}

fn date(t: i64) -> String {
    glib::DateTime::from_unix_local(t)
        .and_then(|d| d.format("%x"))
        .map(|d| d.to_string())
        .unwrap_or_default()
}

/// The selection model of the list, grid or column view `w` is in.
fn model_of(w: &gtk::Widget) -> Option<gtk::SelectionModel> {
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
fn attach_menu(ui: &Weak<Ui>, item: &gtk::ListItem, w: &gtk::Widget) {
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

/// Open the folder at `pos` of `model`.
fn activate(ui: &Weak<Ui>, model: Option<gtk::SelectionModel>, pos: u32) {
    let (Some(ui), Some(file)) = (
        ui.upgrade(),
        model.and_then(|m| m.item(pos)).map(|o| file_of(&o)),
    ) else {
        return;
    };
    let id = ui.at.borrow().as_ref().map(|(id, _)| *id);
    if let (true, Some(id)) = (file.folder, id) {
        nav::open(&ui, id, file.path);
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

/// Show `files` in icons and list.
pub fn show(ui: &Ui, files: &[File]) {
    let items: Vec<glib::BoxedAnyObject> = files
        .iter()
        .cloned()
        .map(glib::BoxedAnyObject::new)
        .collect();
    ui.views.store.splice(0, ui.views.store.n_items(), &items);
}

/// The columns: one for each folder from the root to the folder shown, each with the one that
/// leads to the next selected. Columns kept from before stay as they are.
pub fn refresh_columns(ui: &Rc<Ui>) {
    let v = &ui.views;
    if v.mode.get() != Mode::Columns {
        return;
    }
    let at = ui.at.borrow().clone();
    let levels: Vec<String> = match &at {
        Some((_, path)) => {
            let mut l = nav::ancestors(path);
            l.push(path.clone());
            l
        }
        None => Vec::new(),
    };
    let id = at.map_or(0, |(id, _)| id);
    let mut columns = v.columns.borrow_mut();
    let keep = columns
        .iter()
        .zip(&levels)
        .take_while(|(c, l)| c.path == **l)
        .count();
    while columns.len() > keep {
        if let Some(c) = columns.pop() {
            v.columns_box.remove(&c.scroll);
        }
    }
    for path in &levels[keep..] {
        columns.push(new_column(ui, path));
    }
    v.updating.set(true);
    for (i, column) in columns.iter_mut().enumerate() {
        let data = ui.dirs.borrow().get(&(id, levels[i].clone())).cloned();
        let hidden = ui.hidden.get();
        let same = match (&data, &column.filled) {
            (Some(d), Some((f, h))) => Rc::ptr_eq(d, f) && *h == hidden,
            (None, None) => true,
            _ => false,
        };
        if !same {
            let files = data
                .as_deref()
                .map(|f| nav::visible(ui, f))
                .unwrap_or_default();
            let items: Vec<glib::BoxedAnyObject> =
                files.into_iter().map(glib::BoxedAnyObject::new).collect();
            column.store.splice(0, column.store.n_items(), &items);
            column.filled = data.map(|d| (d, hidden));
        }
        // The folder the next column shows is the one selected here.
        if let Some(next) = levels.get(i + 1) {
            let at = (0..column.store.n_items()).find(|&n| {
                column
                    .store
                    .item(n)
                    .is_some_and(|o| file_of(&o).path == *next)
            });
            match at {
                Some(n) => {
                    column.selection.select_item(n, true);
                }
                None => {
                    column.selection.unselect_all();
                }
            }
        }
    }
    v.updating.set(false);
    // The deepest column is the one to see.
    let scroll = v.columns_scroll.clone();
    glib::timeout_add_local_once(Duration::from_millis(30), move || {
        let a = scroll.hadjustment();
        a.set_value(a.upper() - a.page_size());
    });
}

fn new_column(ui: &Rc<Ui>, path: &str) -> Column {
    let store = gio::ListStore::new::<glib::BoxedAnyObject>();
    let selection = gtk::MultiSelection::new(Some(store.clone()));
    let weak = Rc::downgrade(ui);

    let factory = gtk::SignalListItemFactory::new();
    let w = weak.clone();
    factory.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
        let b = gtk::Box::builder().spacing(10).build();
        b.append(&gtk::Image::builder().pixel_size(24).build());
        b.append(
            &gtk::Label::builder()
                .xalign(0.0)
                .hexpand(true)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build(),
        );
        b.append(&gtk::Image::from_icon_name("go-next-symbolic"));
        item.set_child(Some(&b));
        attach_menu(&w, item, b.upcast_ref());
    });
    factory.connect_bind(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
        let (Some(f), Some(b)) = (item.item().map(|o| file_of(&o)), item.child()) else {
            return;
        };
        if let (Some(image), Some(label), Some(chevron)) = (
            b.first_child().and_downcast::<gtk::Image>(),
            b.first_child()
                .and_then(|c| c.next_sibling())
                .and_downcast::<gtk::Label>(),
            b.last_child().and_downcast::<gtk::Image>(),
        ) {
            image.set_from_gicon(&icon(&f));
            label.set_label(&f.name);
            chevron.set_opacity(if f.folder { 0.6 } else { 0.0 });
        }
    });
    let view = gtk::ListView::new(Some(selection.clone()), Some(factory));
    view.add_css_class("navigation-sidebar");
    // The sidebar style draws icons symbolic; these are the files' own.
    view.add_css_class("miller-list");
    let scroll = gtk::ScrolledWindow::builder()
        .child(&view)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .width_request(COLUMN_WIDTH)
        .css_classes(["miller"])
        .build();
    ui.views.columns_box.append(&scroll);

    let w = weak.clone();
    view.connect_activate(move |view, pos| activate(&w, view.model(), pos));
    let (w, column_path, sel) = (weak, path.to_string(), selection.clone());
    selection.connect_selection_changed(move |_, _, _| {
        let Some(ui) = w.upgrade() else { return };
        if ui.views.updating.get() {
            return;
        }
        ops::update_actions(&ui);
        // One selected opens it: a folder as the next column, a file as the end of the columns.
        let (files, at) = (selected_in(&sel), ui.at.borrow().clone());
        if let ([f], Some((id, at_path))) = (files.as_slice(), at) {
            if f.folder {
                nav::open(&ui, id, f.path.clone());
            } else if at_path != column_path {
                nav::open(&ui, id, column_path.clone());
            }
        }
    });
    let click = gtk::GestureClick::builder().button(3).build();
    let (w, s) = (Rc::downgrade(ui), selection.clone());
    click.connect_pressed(move |g, _, x, y| {
        if let (Some(ui), Some(widget)) = (w.upgrade(), g.widget()) {
            s.unselect_all();
            ops::context_menu(&ui, &widget, x, y);
        }
    });
    view.add_controller(click);
    Column {
        path: path.to_string(),
        filled: None,
        store,
        selection,
        scroll,
    }
}
