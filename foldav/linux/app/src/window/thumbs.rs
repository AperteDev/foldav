//! The previews the server made of images, documents and videos, shown instead of the type's
//! icon. What is kept on this device shows at once; the rest is fetched a few at a time, the
//! ones asked for last first (those are on screen), and put in the image when it arrives, if the
//! image still shows that file.

use super::ui;
use crate::core;
use adw::prelude::*;
use foldav_core::File;
use gtk::{gdk, glib};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

/// How many previews are asked of the server at once.
const AT_ONCE: usize = 4;
/// How many are kept decoded, before the lot is dropped.
const KEPT: usize = 600;

struct Job {
    account: i64,
    file: File,
    size: u32,
    image: glib::WeakRef<gtk::Image>,
    /// What the image is told to show: the preview's place, which says the file and its ETag.
    key: String,
}

thread_local! {
    static QUEUE: RefCell<VecDeque<Job>> = const { RefCell::new(VecDeque::new()) };
    static RUNNING: Cell<usize> = const { Cell::new(0) };
    static TEXTURES: RefCell<HashMap<PathBuf, gdk::Texture>> = RefCell::default();
}

fn texture(path: &PathBuf) -> Option<gdk::Texture> {
    if let Some(t) = TEXTURES.with(|t| t.borrow().get(path).cloned()) {
        return Some(t);
    }
    let t = gdk::Texture::from_filename(path).ok()?;
    TEXTURES.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= KEPT {
            c.clear();
        }
        c.insert(path.clone(), t.clone());
    });
    Some(t)
}

/// Show the preview of `f`, up to `size` pixels, in `image`, as soon as there is one; the icon
/// that is there stays until then, and when there is none.
pub fn apply(image: &gtk::Image, f: &File, size: u32) {
    let at = ui().and_then(|ui| ui.at.borrow().as_ref().map(|(id, _)| *id));
    let Some(account) = at else { return };
    let Some(path) = core().thumbnail_path(account, f, size) else {
        image.set_widget_name("");
        return;
    };
    let key = path.to_string_lossy().into_owned();
    image.set_widget_name(&key);
    if let Some(t) = texture(&path).filter(|_| path.exists()) {
        return image.set_paintable(Some(&t));
    }
    QUEUE.with(|q| {
        q.borrow_mut().push_front(Job {
            account,
            file: f.clone(),
            size,
            image: image.downgrade(),
            key,
        })
    });
    pump();
}

/// Start fetching while there are previews wanted and room for them.
fn pump() {
    while RUNNING.get() < AT_ONCE {
        let Some(job) = QUEUE.with(|q| q.borrow_mut().pop_front()) else {
            return;
        };
        // Scrolled away, or the image shows another file now: not wanted any more.
        let wanted = job
            .image
            .upgrade()
            .is_some_and(|i| i.widget_name() == job.key);
        if !wanted {
            continue;
        }
        RUNNING.set(RUNNING.get() + 1);
        glib::spawn_future_local(async move {
            let (account, file, size) = (job.account, job.file.clone(), job.size);
            let r = gtk::gio::spawn_blocking(move || core().thumbnail(account, &file, size)).await;
            RUNNING.set(RUNNING.get() - 1);
            if let (Ok(Ok(Some(path))), Some(image)) = (r, job.image.upgrade())
                && image.widget_name() == job.key
                && let Some(t) = texture(&path)
            {
                image.set_paintable(Some(&t));
            }
            pump();
        });
    }
}
