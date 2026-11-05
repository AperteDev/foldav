//! What Files' view menu sets: the sort (one for the three views, and the list's header sets it
//! too), the icon size (the icons' and the list's own, with the plus and minus of the menu, and
//! Ctrl+plus, Ctrl+minus and Ctrl+0), and the captions beneath the icons' names. All of it is
//! kept on this device.

use super::views::{Mode, date};
use super::{Ui, action, get, nav, view};
use crate::core;
use crate::i18n::gettext;
use adw::prelude::*;
use foldav_core::File;
use gtk::{gio, glib};
use std::cell::Cell;
use std::cmp::Ordering;
use std::rc::Rc;

/// How many captions an icon can have.
pub const CAPTIONS: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Name,
    Modified,
    Size,
    Type,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sort {
    pub key: Key,
    pub desc: bool,
}

/// The sorts by their names (the menu's action targets): the ones of the menu, as Files has them
/// (the newest first, the biggest first), and the ones a click on the list's header makes.
const SORTS: [(&str, Key, bool); 8] = [
    ("name", Key::Name, false),
    ("name-desc", Key::Name, true),
    ("modified", Key::Modified, true),
    ("modified-asc", Key::Modified, false),
    ("size", Key::Size, true),
    ("size-asc", Key::Size, false),
    ("type", Key::Type, false),
    ("type-desc", Key::Type, true),
];

impl Sort {
    fn name(self) -> &'static str {
        SORTS
            .iter()
            .find(|s| (s.1, s.2) == (self.key, self.desc))
            .map_or("name", |s| s.0)
    }

    fn from_name(name: &str) -> Option<Sort> {
        SORTS.iter().find(|s| s.0 == name).map(|s| Sort {
            key: s.1,
            desc: s.2,
        })
    }

    /// Folders first, whichever way; then the key, and the name where that is the same.
    fn compare(self, a: &File, b: &File) -> Ordering {
        let by = match self.key {
            Key::Name => by_name(a, b),
            Key::Modified => a.modified.cmp(&b.modified),
            Key::Size => a.size.cmp(&b.size),
            // The type's name, not its description: the same files are together all the same.
            Key::Type => a.mime.cmp(&b.mime),
        };
        (!a.folder)
            .cmp(&!b.folder)
            .then(if self.desc { by.reverse() } else { by })
            .then_with(|| by_name(a, b))
    }
}

fn by_name(a: &File, b: &File) -> Ordering {
    a.name.to_lowercase().cmp(&b.name.to_lowercase())
}

/// What a caption says of a file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Caption {
    None,
    Size,
    Type,
    Modified,
}

const CAPTION_NAMES: [(&str, Caption); 4] = [
    ("none", Caption::None),
    ("size", Caption::Size),
    ("type", Caption::Type),
    ("modified", Caption::Modified),
];

impl Caption {
    fn name(self) -> &'static str {
        CAPTION_NAMES
            .iter()
            .find(|c| c.1 == self)
            .map_or("none", |c| c.0)
    }

    fn from_name(name: &str) -> Caption {
        CAPTION_NAMES
            .iter()
            .find(|c| c.0 == name)
            .map_or(Caption::None, |c| c.1)
    }

    fn label(self) -> String {
        match self {
            Caption::None => gettext("None"),
            Caption::Size => gettext("Size"),
            Caption::Type => gettext("Type"),
            Caption::Modified => gettext("Modified"),
        }
    }

    pub fn text(self, f: &File) -> String {
        match self {
            Caption::None => String::new(),
            Caption::Size => f
                .size
                .map(glib::format_size)
                .unwrap_or_default()
                .to_string(),
            Caption::Type if f.folder => gettext("Folder"),
            Caption::Type => gio::content_type_get_description(&f.mime).to_string(),
            Caption::Modified => date(f.modified),
        }
    }
}

/// The icons' sizes (pixels), as Files has them, and the one they start at.
const GRID_PX: [i32; 5] = [48, 64, 96, 168, 256];
const GRID_DEFAULT: usize = 2;
/// How many captions show at each of them: more as the icons grow.
const GRID_CAPTIONS: [usize; 5] = [0, 1, 2, 3, 3];
/// The list's.
const LIST_PX: [i32; 3] = [16, 32, 64];
const LIST_DEFAULT: usize = 0;

pub struct Options {
    sort: Cell<Option<Sort>>,
    grid: Cell<usize>,
    list: Cell<usize>,
    captions: Cell<[Caption; CAPTIONS]>,
    /// Changes with the sort, for what was made from it (the columns) to be made again.
    pub version: Cell<u64>,
    /// Set while the code sets the list's header, which is no click of the user's.
    syncing: Cell<bool>,
}

impl Options {
    /// As they were left. No sort was chosen yet: the files stay as they come (the folders'
    /// as folders then by name, and the places' in their own order).
    pub fn load() -> Options {
        let get = |key: &str| core().setting(key.into());
        let index = |key: &str, max: usize, default: usize| {
            get(key)
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|&i| i < max)
                .unwrap_or(default)
        };
        let mut captions = [Caption::None; CAPTIONS];
        for (slot, name) in get("captions").unwrap_or_default().split(',').enumerate() {
            if let Some(c) = captions.get_mut(slot) {
                *c = Caption::from_name(name);
            }
        }
        Options {
            sort: Cell::new(get("sort").and_then(|s| Sort::from_name(&s))),
            grid: Cell::new(index("grid_zoom", GRID_PX.len(), GRID_DEFAULT)),
            list: Cell::new(index("list_zoom", LIST_PX.len(), LIST_DEFAULT)),
            captions: Cell::new(captions),
            version: Cell::new(0),
            syncing: Cell::new(false),
        }
    }

    /// `files` in the order chosen, if one was.
    pub fn sort_files(&self, files: &mut [File]) {
        if let Some(sort) = self.sort.get() {
            files.sort_by(|a, b| sort.compare(a, b));
        }
    }

    pub fn grid_px(&self) -> i32 {
        GRID_PX[self.grid.get()]
    }

    pub fn list_px(&self) -> i32 {
        LIST_PX[self.list.get()]
    }

    /// The captions the icons show now: the first ones, as many as the size has room for.
    pub fn captions(&self) -> Vec<Caption> {
        self.captions
            .get()
            .into_iter()
            .take(GRID_CAPTIONS[self.grid.get()])
            .filter(|&c| c != Caption::None)
            .collect()
    }
}

fn save(key: &str, value: String) {
    let _ = core().set_setting(key.into(), value);
}

/// Sort as `sort` says; `from_header` when the list's header said it, which then shows it
/// already.
fn set_sort(ui: &Rc<Ui>, sort: Sort, from_header: bool) {
    let o = &ui.options;
    if o.sort.get() == Some(sort) {
        return;
    }
    o.sort.set(Some(sort));
    o.version.set(o.version.get() + 1);
    save("sort", sort.name().into());
    if let Some(a) = ui.window.lookup_action("sort") {
        a.change_state(&sort.name().to_variant());
    }
    if !from_header {
        // The arrow of the list's header.
        let id = match sort.key {
            Key::Name => Some("name"),
            Key::Size => Some("size"),
            Key::Modified => Some("modified"),
            Key::Type => None,
        };
        let column = ui
            .views
            .list
            .columns()
            .iter::<gtk::ColumnViewColumn>()
            .flatten()
            .find(|c| c.id().as_deref() == id);
        o.syncing.set(true);
        ui.views.list.sort_by_column(
            column.as_ref(),
            if sort.desc {
                gtk::SortType::Descending
            } else {
                gtk::SortType::Ascending
            },
        );
        o.syncing.set(false);
    }
    nav::show(ui);
}

/// Zoom the icons or the list (the columns have one size) a step, or back to where they began
/// (`step` 0).
fn zoom(ui: &Rc<Ui>, step: isize) {
    let o = &ui.options;
    let (cell, max, default) = match ui.views.mode() {
        Mode::Icons => (&o.grid, GRID_PX.len(), GRID_DEFAULT),
        Mode::List => (&o.list, LIST_PX.len(), LIST_DEFAULT),
        Mode::Columns => return,
    };
    let now = if step == 0 {
        default
    } else {
        cell.get().saturating_add_signed(step).min(max - 1)
    };
    if now == cell.get() {
        return;
    }
    cell.set(now);
    save(
        if ui.views.mode() == Mode::Icons {
            "grid_zoom"
        } else {
            "list_zoom"
        },
        now.to_string(),
    );
    update(ui);
    nav::show(ui);
}

/// The zoom buttons work where there is a zoom to change, and as far as it goes.
pub fn update(ui: &Rc<Ui>) {
    let o = &ui.options;
    let (now, max, default) = match ui.views.mode() {
        Mode::Icons => (o.grid.get(), GRID_PX.len(), GRID_DEFAULT),
        Mode::List => (o.list.get(), LIST_PX.len(), LIST_DEFAULT),
        Mode::Columns => (0, 0, 0),
    };
    let set = |name: &str, on: bool| {
        ui.window
            .lookup_action(name)
            .and_downcast::<gio::SimpleAction>()
            .inspect(|a| a.set_enabled(on));
    };
    set("zoom-in", max > 0 && now + 1 < max);
    set("zoom-out", max > 0 && now > 0);
    set("zoom-reset", max > 0 && now != default);
    set("captions", ui.views.mode() == Mode::Icons);
}

/// The dialog choosing the three captions.
fn captions_dialog(ui: &Rc<Ui>) {
    let b = view!("captions");
    let dialog: adw::Dialog = get(&b, "dialog");
    let labels: Vec<String> = CAPTION_NAMES.iter().map(|c| c.1.label()).collect();
    let model = gtk::StringList::new(&labels.iter().map(String::as_str).collect::<Vec<_>>());
    for (slot, id) in ["first", "second", "third"].into_iter().enumerate() {
        let row: adw::ComboRow = get(&b, id);
        row.set_model(Some(&model));
        let now = ui.options.captions.get()[slot];
        row.set_selected(CAPTION_NAMES.iter().position(|c| c.1 == now).unwrap_or(0) as u32);
        let weak = Rc::downgrade(ui);
        row.connect_selected_notify(move |row| {
            let (Some(ui), Some(&(_, caption))) =
                (weak.upgrade(), CAPTION_NAMES.get(row.selected() as usize))
            else {
                return;
            };
            let mut all = ui.options.captions.get();
            all[slot] = caption;
            ui.options.captions.set(all);
            save(
                "captions",
                all.iter().map(|c| c.name()).collect::<Vec<_>>().join(","),
            );
            nav::show(&ui);
        });
    }
    dialog.present(Some(&ui.window));
}

/// The menu's actions, its icon size row, and a click on the list's header.
pub fn connect(ui: &Rc<Ui>) {
    let current = ui.options.sort.get().map_or("name", Sort::name);
    let sort = gio::SimpleAction::new_stateful(
        "sort",
        Some(glib::VariantTy::STRING),
        &current.to_variant(),
    );
    let weak = Rc::downgrade(ui);
    sort.connect_activate(move |_, target| {
        if let (Some(ui), Some(sort)) = (
            weak.upgrade(),
            target.and_then(|t| t.str()).and_then(Sort::from_name),
        ) {
            set_sort(&ui, sort, false);
        }
    });
    // The state is what the radio items show: set by `set_sort`, not by the activation.
    sort.connect_change_state(|a, state| {
        if let Some(state) = state {
            a.set_state(state);
        }
    });
    ui.window.add_action(&sort);
    if let Some(saved) = ui.options.sort.get() {
        // The arrow of the list's header, from the start.
        ui.options.sort.set(None);
        set_sort(ui, saved, false);
    }

    action(ui, "zoom-in", |ui| zoom(ui, 1));
    action(ui, "zoom-out", |ui| zoom(ui, -1));
    action(ui, "zoom-reset", |ui| zoom(ui, 0));
    action(ui, "captions", captions_dialog);
    update(ui);

    // The icon size row of the menu: its buttons are the zoom actions'.
    let row = gtk::Box::builder()
        .spacing(6)
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(6)
        .build();
    row.append(
        &gtk::Label::builder()
            .label(gettext("Icon Size"))
            .hexpand(true)
            .xalign(0.0)
            .build(),
    );
    for (icon, action, tip) in [
        ("zoom-out-symbolic", "win.zoom-out", gettext("Zoom Out")),
        ("zoom-in-symbolic", "win.zoom-in", gettext("Zoom In")),
    ] {
        row.append(
            &gtk::Button::builder()
                .icon_name(icon)
                .action_name(action)
                .tooltip_text(tip)
                .css_classes(["flat", "circular"])
                .build(),
        );
    }
    if let Some(menu) = ui
        .header
        .options
        .popover()
        .and_downcast::<gtk::PopoverMenu>()
    {
        menu.add_child(&row, "zoom");
    }

    // A click on a header of the list sorts, and the menu follows.
    let weak = Rc::downgrade(ui);
    if let Some(sorter) = ui
        .views
        .list
        .sorter()
        .and_downcast::<gtk::ColumnViewSorter>()
    {
        sorter.connect_changed(move |s, _| {
            let Some(ui) = weak.upgrade().filter(|ui| !ui.options.syncing.get()) else {
                return;
            };
            let Some(column) = s.primary_sort_column() else {
                return;
            };
            let key = match column.id().as_deref() {
                Some("size") => Key::Size,
                Some("modified") => Key::Modified,
                _ => Key::Name,
            };
            let desc = s.primary_sort_order() == gtk::SortType::Descending;
            set_sort(&ui, Sort { key, desc }, true);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, folder: bool, size: u64, modified: i64) -> File {
        File {
            name: name.into(),
            path: name.into(),
            folder,
            size: Some(size),
            modified,
            mime: String::new(),
            favorite: false,
            has_preview: false,
            etag: String::new(),
            fileid: String::new(),
        }
    }

    #[test]
    fn folders_stay_first_whichever_way_the_sort_goes() {
        let mut files = vec![
            file("b.txt", false, 5, 20),
            file("A", true, 1, 10),
            file("a.txt", false, 9, 30),
            file("Z", true, 2, 40),
        ];
        let order = |sort: &str, files: &mut Vec<File>| {
            files.sort_by(|a, b| Sort::from_name(sort).unwrap().compare(a, b));
            files.iter().map(|f| f.name.clone()).collect::<Vec<_>>()
        };
        assert_eq!(order("name", &mut files), ["A", "Z", "a.txt", "b.txt"]);
        assert_eq!(order("name-desc", &mut files), ["Z", "A", "b.txt", "a.txt"]);
        // Last Modified: the newest first; Size: the biggest first.
        assert_eq!(order("modified", &mut files), ["Z", "A", "a.txt", "b.txt"]);
        assert_eq!(
            order("modified-asc", &mut files),
            ["A", "Z", "b.txt", "a.txt"]
        );
        assert_eq!(order("size", &mut files), ["Z", "A", "a.txt", "b.txt"]);
    }
}
