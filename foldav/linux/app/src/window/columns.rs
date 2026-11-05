//! The columns view, after the Column View of the Files extension "My Computer" (MIT licence,
//! Yann Masoch): one column for each folder from the root down, each with the folder leading to
//! the next selected. A click on a folder opens it as the next column, on a file shows it in a
//! preview column at the end; the selection of the column the user works in is accent
//! coloured, the ones that lead to it grey, as Finder's; columns are resized by dragging their
//! edge; Left and Right move between them; a click on a column's empty room closes the ones to
//! its right.

use super::views::{activate, attach_menu, file_of, select_paths, selected_in, set_icon, star};
use super::{Ui, nav, ops};
use crate::core;
use crate::i18n::gettext;
use adw::prelude::*;
use foldav_core::File;
use gtk::{gdk, gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// A column is this wide to begin with, and as wide as the user drags it, between the limits.
const COLUMN_WIDTH: i32 = 300;
const COLUMN_MIN_WIDTH: i32 = 180;
const COLUMN_MAX_WIDTH: i32 = 580;
/// The preview column at least; it takes the room that is left.
const PREVIEW_WIDTH: i32 = 400;
const PREVIEW_ICON_SIZE: i32 = 256;

/// One column: folder `path`, as it was filled.
struct Column {
    path: String,
    /// What it was filled from, whether hidden files were in it, and the favorites' number.
    filled: Option<(Rc<Vec<File>>, bool, u64)>,
    store: gio::ListStore,
    selection: gtk::MultiSelection,
    view: gtk::ListView,
    scroll: gtk::ScrolledWindow,
    /// What is dragged to resize it, after it.
    handle: gtk::Widget,
}

/// The file selected in the last column: its icon (the thumbnail, once there are any), name,
/// and details.
struct Preview {
    root: gtk::Box,
    icon: gtk::Image,
    name: gtk::Label,
    detail: gtk::Label,
    modified: gtk::Label,
}

pub struct Columns {
    outer: gtk::Box,
    pub scroll: gtk::ScrolledWindow,
    columns: RefCell<Vec<Column>>,
    /// Set while the code selects, which is no choice of the user's.
    updating: Cell<bool>,
    /// The width of a new column: the last one the user dragged.
    width: Cell<i32>,
    preview: Preview,
}

impl Columns {
    pub fn new(b: &gtk::Builder) -> Columns {
        let width = core()
            .setting("column_width".into())
            .and_then(|w| w.parse().ok())
            .unwrap_or(COLUMN_WIDTH)
            .clamp(COLUMN_MIN_WIDTH, COLUMN_MAX_WIDTH);
        Columns {
            outer: super::get(b, "columns"),
            scroll: super::get(b, "columns_scroll"),
            columns: RefCell::default(),
            updating: Cell::new(false),
            width: Cell::new(width),
            preview: Preview::new(),
        }
    }

    /// The files the operations are for: those selected in the last column, else the folder
    /// shown, selected in the one before.
    pub fn selected(&self) -> Vec<File> {
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

    /// The column the user works in: the deepest with something selected (an empty one just
    /// opened is the folder's, the one before it is where the user is).
    fn active(&self) -> Option<usize> {
        self.columns
            .borrow()
            .iter()
            .rposition(|c| has_selection(&c.selection))
    }

    fn preview_shown(&self) -> bool {
        self.preview.root.parent().is_some()
    }
}

fn has_selection(s: &gtk::MultiSelection) -> bool {
    (0..s.n_items()).any(|i| s.is_selected(i))
}

impl Preview {
    fn new() -> Preview {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .hexpand(true)
            .width_request(PREVIEW_WIDTH)
            .css_classes(["miller-preview"])
            .build();
        let icon = gtk::Image::builder()
            .pixel_size(PREVIEW_ICON_SIZE)
            .vexpand(true)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        let name = gtk::Label::builder()
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .max_width_chars(20)
            .justify(gtk::Justification::Center)
            .css_classes(["heading"])
            .build();
        let detail = gtk::Label::builder()
            .css_classes(["dim-label", "caption"])
            .build();
        let modified = gtk::Label::builder()
            .xalign(1.0)
            .hexpand(true)
            .css_classes(["caption"])
            .build();
        let row = gtk::Box::builder().margin_top(6).build();
        row.append(
            &gtk::Label::builder()
                .label(gettext("Modified"))
                .css_classes(["dim-label", "caption"])
                .build(),
        );
        row.append(&modified);
        for w in [
            icon.upcast_ref::<gtk::Widget>(),
            name.upcast_ref(),
            detail.upcast_ref(),
            row.upcast_ref(),
        ] {
            root.append(w);
        }
        Preview {
            root,
            icon,
            name,
            detail,
            modified,
        }
    }

    fn show(&self, f: &File) {
        set_icon(&self.icon, f);
        super::thumbs::apply(&self.icon, f, PREVIEW_ICON_SIZE as u32);
        self.name.set_label(&f.name);
        let kind = gio::content_type_get_description(&f.mime);
        let size = f
            .size
            .map(|s| glib::format_size(s).to_string())
            .unwrap_or_default();
        self.detail.set_label(&if kind.is_empty() {
            size
        } else {
            format!("{kind} · {size}")
        });
        self.modified.set_label(&super::views::date(f.modified));
    }
}

/// What the columns do with keys, and a right click on the room around them.
pub fn connect(ui: &Rc<Ui>) {
    let c = &ui.views.columns;
    let weak = Rc::downgrade(ui);
    let key = gtk::EventControllerKey::new();
    key.set_propagation_phase(gtk::PropagationPhase::Capture);
    key.connect_key_pressed(move |_, keyval, _, state| {
        let plain = !state.intersects(
            gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SHIFT_MASK,
        );
        let Some(ui) = weak.upgrade().filter(|_| plain) else {
            return glib::Propagation::Proceed;
        };
        let handled = match keyval {
            gdk::Key::Right => right(&ui),
            gdk::Key::Left => left(&ui),
            _ => false,
        };
        if handled {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    c.scroll.add_controller(key);

    let click = gtk::GestureClick::builder().button(3).build();
    let weak = Rc::downgrade(ui);
    click.connect_pressed(move |g, _, x, y| {
        if let (Some(ui), Some(widget)) = (weak.upgrade(), g.widget()) {
            ops::context_menu(&ui, &widget, x, y);
        }
    });
    c.scroll.add_controller(click);
}

/// Right: into the column the selected folder opened, selecting its first file.
fn right(ui: &Rc<Ui>) -> bool {
    let c = &ui.views.columns;
    let Some(a) = c.active() else { return false };
    let next = c
        .columns
        .borrow()
        .get(a + 1)
        .map(|n| (n.view.clone(), n.selection.clone()));
    let Some((view, selection)) = next else {
        return false;
    };
    view.grab_focus();
    if !has_selection(&selection) && selection.n_items() > 0 {
        selection.select_item(0, true);
    }
    true
}

/// Left: back to the column before. What was selected in the one left closes (a file) or is
/// left selected only as the folder shown (a folder, whose columns after it close).
fn left(ui: &Rc<Ui>) -> bool {
    let c = &ui.views.columns;
    let Some(a) = c.active().filter(|a| *a > 0) else {
        return false;
    };
    let (path, selection, before, last) = {
        let columns = c.columns.borrow();
        (
            columns[a].path.clone(),
            columns[a].selection.clone(),
            columns[a - 1].view.clone(),
            columns.len() - 1,
        )
    };
    c.updating.set(true);
    selection.unselect_all();
    c.updating.set(false);
    if a < last {
        let id = ui.at.borrow().as_ref().map(|(id, _)| *id);
        if let Some(id) = id {
            nav::open(ui, id, path);
        }
    } else {
        paint(c);
        update_preview(ui);
    }
    before.grab_focus();
    true
}

/// The columns: one for each folder from the root to the folder shown, each with the one that
/// leads to the next selected. Columns kept from before stay as they are.
pub fn refresh(ui: &Rc<Ui>) {
    let c = &ui.views.columns;
    if ui.views.mode() != super::views::Mode::Columns {
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
    // Taken out for now, to be put back after the last column.
    if c.preview_shown() {
        c.outer.remove(&c.preview.root);
    }
    {
        let mut columns = c.columns.borrow_mut();
        let keep = columns
            .iter()
            .zip(&levels)
            .take_while(|(col, l)| col.path == **l)
            .count();
        while columns.len() > keep {
            if let Some(col) = columns.pop() {
                c.outer.remove(&col.scroll);
                c.outer.remove(&col.handle);
            }
        }
        for path in &levels[keep..] {
            columns.push(new_column(ui, path));
        }
        c.updating.set(true);
        for (i, column) in columns.iter_mut().enumerate() {
            let data = ui.dirs.borrow().get(&(id, levels[i].clone())).cloned();
            let hidden = ui.hidden.get();
            let stars = ui.fav_version.get();
            let same = match (&data, &column.filled) {
                (Some(d), Some((f, h, s))) => {
                    (Rc::ptr_eq(d, f) || **d == **f) && *h == hidden && *s == stars
                }
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
                // What was selected, and is still there, stays.
                let kept: Vec<String> = selected_in(&column.selection)
                    .into_iter()
                    .map(|f| f.path)
                    .collect();
                column.store.splice(0, column.store.n_items(), &items);
                select_paths(&column.selection, &kept);
                column.filled = data.map(|d| (d, hidden, stars));
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
        c.updating.set(false);
    }
    paint(c);
    update_preview(ui);
    scroll_to_end(c);
}

/// The accent colour on the active column's selection, grey on the others'.
fn paint(c: &Columns) {
    let active = c.active();
    for (i, col) in c.columns.borrow().iter().enumerate() {
        if Some(i) == active {
            col.view.add_css_class("miller-active");
        } else {
            col.view.remove_css_class("miller-active");
        }
    }
}

/// The preview column: after the last, while a file is selected there.
fn update_preview(ui: &Rc<Ui>) {
    let c = &ui.views.columns;
    let file = {
        let columns = c.columns.borrow();
        columns
            .last()
            .map(|col| selected_in(&col.selection))
            .filter(|f| f.len() == 1 && !f[0].folder)
            .map(|mut f| f.remove(0))
    };
    match file {
        Some(f) => {
            c.preview.show(&f);
            if !c.preview_shown() {
                c.outer.append(&c.preview.root);
                scroll_to_end(c);
            }
        }
        None if c.preview_shown() => c.outer.remove(&c.preview.root),
        None => {}
    }
}

/// The last column (or the preview) is the one to see.
fn scroll_to_end(c: &Columns) {
    let scroll = c.scroll.clone();
    // After the new column has its place.
    glib::timeout_add_local_once(Duration::from_millis(80), move || {
        let a = scroll.hadjustment();
        let target = (a.upper() - a.page_size()).max(0.0);
        if (target - a.value()).abs() < 1.0 {
            return;
        }
        let a2 = a.clone();
        let animation = adw::TimedAnimation::new(
            &scroll,
            a.value(),
            target,
            200,
            adw::CallbackAnimationTarget::new(move |v| a2.set_value(v)),
        );
        animation.set_easing(adw::Easing::EaseOutCubic);
        animation.play();
    });
}

fn new_column(ui: &Rc<Ui>, path: &str) -> Column {
    let c = &ui.views.columns;
    let store = gio::ListStore::new::<glib::BoxedAnyObject>();
    let selection = gtk::MultiSelection::new(Some(store.clone()));
    let weak = Rc::downgrade(ui);

    // A row: the icon in a place of its own, the name, a chevron on a folder's.
    let factory = gtk::SignalListItemFactory::new();
    let w = weak.clone();
    factory.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
        let b = gtk::Box::builder()
            .spacing(8)
            .css_classes(["miller-row"])
            .build();
        b.append(
            &gtk::Image::builder()
                .pixel_size(24)
                .valign(gtk::Align::Center)
                .build(),
        );
        // A long name must not make the column wider than it is.
        b.append(
            &gtk::Label::builder()
                .xalign(0.0)
                .hexpand(true)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .max_width_chars(1)
                .build(),
        );
        b.append(&star(12));
        b.append(
            &gtk::Image::builder()
                .icon_name("go-next-symbolic")
                .pixel_size(12)
                .css_classes(["dim-label"])
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
        let label = b.first_child().and_then(|c| c.next_sibling());
        if let (Some(image), Some(label), Some(star), Some(chevron)) = (
            b.first_child().and_downcast::<gtk::Image>(),
            label.clone().and_downcast::<gtk::Label>(),
            label.and_then(|l| l.next_sibling()),
            b.last_child().and_downcast::<gtk::Image>(),
        ) {
            set_icon(&image, &f);
            label.set_label(&f.name);
            star.set_visible(f.favorite);
            chevron.set_visible(f.folder);
        }
    });
    let view = gtk::ListView::new(Some(selection.clone()), Some(factory));
    view.add_css_class("miller-list");
    view.add_css_class("files-view");
    let scroll = gtk::ScrolledWindow::builder()
        .child(&view)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .width_request(c.width.get())
        .hexpand(false)
        .build();
    let handle = new_handle(ui, &scroll);
    c.outer.append(&scroll);
    c.outer.append(&handle);

    let w = weak.clone();
    view.connect_activate(move |view, pos| activate(&w, view.model(), pos));

    // What the user selects: one folder opens as the next column, one file ends the columns
    // here and is previewed; none or several change nothing but the colours.
    let (w, column_path, sel) = (weak.clone(), path.to_string(), selection.clone());
    selection.connect_selection_changed(move |_, _, _| {
        let Some(ui) = w.upgrade() else { return };
        let c = &ui.views.columns;
        if c.updating.get() {
            return;
        }
        ops::update_actions(&ui);
        let (files, at) = (selected_in(&sel), ui.at.borrow().clone());
        match (files.as_slice(), at) {
            ([f], Some((id, _))) if f.folder => nav::open(&ui, id, f.path.clone()),
            ([_], Some((id, at_path))) if at_path != column_path => {
                nav::open(&ui, id, column_path.clone())
            }
            _ => {
                paint(c);
                update_preview(&ui);
            }
        }
    });

    // A click on the room below the rows: nothing is selected here, the columns after end.
    let click = gtk::GestureClick::builder()
        .button(gdk::BUTTON_PRIMARY)
        .build();
    let (w, column_path, sel) = (weak.clone(), path.to_string(), selection.clone());
    click.connect_released(move |g, _, x, y| {
        let (Some(ui), Some(view)) = (w.upgrade(), g.widget()) else {
            return;
        };
        if view.pick(x, y, gtk::PickFlags::DEFAULT).as_ref() != Some(&view) {
            return;
        }
        let c = &ui.views.columns;
        c.updating.set(true);
        sel.unselect_all();
        c.updating.set(false);
        let at = ui.at.borrow().clone();
        match at {
            Some((id, at_path)) if at_path != column_path => {
                nav::open(&ui, id, column_path.clone())
            }
            _ => {
                paint(c);
                update_preview(&ui);
            }
        }
        ops::update_actions(&ui);
    });
    view.add_controller(click);

    let click = gtk::GestureClick::builder().button(3).build();
    let (w, sel) = (weak, selection.clone());
    click.connect_pressed(move |g, _, x, y| {
        if let (Some(ui), Some(widget)) = (w.upgrade(), g.widget()) {
            sel.unselect_all();
            ops::context_menu(&ui, &widget, x, y);
        }
    });
    view.add_controller(click);

    Column {
        path: path.to_string(),
        filled: None,
        store,
        selection,
        view,
        scroll,
        handle: handle.upcast(),
    }
}

/// The edge of a column, dragged to make it narrower or wider; the width is the next
/// columns' too.
fn new_handle(ui: &Rc<Ui>, scroll: &gtk::ScrolledWindow) -> gtk::Box {
    let handle = gtk::Box::builder()
        .width_request(8)
        .css_classes(["miller-handle"])
        .build();
    handle.set_cursor_from_name(Some("col-resize"));
    let drag = gtk::GestureDrag::new();
    let start = Rc::new(Cell::new(0));
    let (s, sc) = (start.clone(), scroll.clone());
    drag.connect_drag_begin(move |_, _, _| s.set(sc.width()));
    let (s, sc, weak) = (start, scroll.clone(), Rc::downgrade(ui));
    drag.connect_drag_update(move |_, dx, _| {
        let width = (s.get() + dx as i32).clamp(COLUMN_MIN_WIDTH, COLUMN_MAX_WIDTH);
        sc.set_size_request(width, -1);
        if let Some(ui) = weak.upgrade() {
            ui.views.columns.width.set(width);
        }
    });
    let weak = Rc::downgrade(ui);
    drag.connect_drag_end(move |_, _, _| {
        if let Some(ui) = weak.upgrade() {
            let _ = core().set_setting(
                "column_width".into(),
                ui.views.columns.width.get().to_string(),
            );
        }
    });
    handle.add_controller(drag);
    handle
}
