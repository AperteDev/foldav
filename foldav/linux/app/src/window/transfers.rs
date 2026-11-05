//! The long actions running, as Nautilus shows them: a row each at the foot of the sidebar, a
//! ring (or a spinner while the size is not known) and the title, there once the action has
//! taken a moment; a click opens its progress, with the time left and the speed, and (for a
//! transfer) a way to cancel it. What is done at once never shows.

use super::{Ui, ui};
use crate::core;
use crate::i18n::{gettext, ngettext};
use adw::prelude::*;
use foldav_core::{Client, Progress};
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::f64::consts::{PI, TAU};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long an action runs before its row shows.
const DELAY: Duration = Duration::from_secs(1);
/// How long before the time left and the speed are worth showing.
const ESTIMATE_AFTER: Duration = Duration::from_secs(2);
/// How often the rows are brought up to date.
const TICK: Duration = Duration::from_millis(200);

struct Job {
    progress: Arc<Progress>,
    started: Instant,
    row: gtk::MenuButton,
    /// "ring" or "spinner".
    stack: gtk::Stack,
    ring: gtk::DrawingArea,
    fraction: Rc<Cell<f64>>,
    bar: gtk::ProgressBar,
    status: gtk::Label,
}

pub struct Transfers {
    list: gtk::Box,
    jobs: RefCell<Vec<Rc<Job>>>,
    ticking: Cell<bool>,
}

/// A ring in the text colour, filled `fraction` of the way round from the top.
fn ring(fraction: Rc<Cell<f64>>) -> gtk::DrawingArea {
    let ring = gtk::DrawingArea::builder()
        .content_width(16)
        .content_height(16)
        .build();
    ring.set_draw_func(move |area, cr, w, h| {
        let (w, h) = (f64::from(w), f64::from(h));
        let (x, y, r) = (w / 2.0, h / 2.0, w.min(h) / 2.0 - 1.5);
        let c = area.color();
        let (red, green, blue) = (c.red().into(), c.green().into(), c.blue().into());
        cr.set_line_width(3.0);
        cr.set_source_rgba(red, green, blue, 0.25);
        cr.arc(x, y, r, 0.0, TAU);
        let _ = cr.stroke();
        cr.set_source_rgba(red, green, blue, 1.0);
        cr.arc(x, y, r, -PI / 2.0, -PI / 2.0 + TAU * fraction.get());
        let _ = cr.stroke();
    });
    ring
}

/// "12 minutes left".
fn time_left(secs: u64) -> String {
    let (minutes, hours) = (secs.div_ceil(60), secs.div_ceil(3600));
    let (text, n) = match secs {
        0..60 => (ngettext("{n} second left", "{n} seconds left", secs), secs),
        60..3600 => (
            ngettext("{n} minute left", "{n} minutes left", minutes),
            minutes,
        ),
        _ => (ngettext("{n} hour left", "{n} hours left", hours), hours),
    };
    text.replace("{n}", &n.to_string())
}

impl Transfers {
    pub fn new(b: &gtk::Builder) -> Transfers {
        Transfers {
            list: super::get(b, "transfers"),
            jobs: RefCell::default(),
            ticking: Cell::new(false),
        }
    }

    fn add(&self, title: &str, cancellable: bool, progress: Arc<Progress>) -> Rc<Job> {
        let fraction = Rc::new(Cell::new(0.0));
        let ring = ring(fraction.clone());
        let stack = gtk::Stack::new();
        stack.add_named(&ring, Some("ring"));
        stack.add_named(
            &adw::Spinner::builder()
                .width_request(16)
                .height_request(16)
                .build(),
            Some("spinner"),
        );
        stack.set_visible_child_name("spinner");
        let label = gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let content = gtk::Box::builder().spacing(12).build();
        content.append(&stack);
        content.append(&label);

        // What a click on the row shows.
        let bar = gtk::ProgressBar::builder().build();
        let status = gtk::Label::builder()
            .xalign(0.0)
            .css_classes(["caption", "dim-label"])
            .build();
        let details = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        details.append(
            &gtk::Label::builder()
                .label(title)
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::Middle)
                .max_width_chars(36)
                .build(),
        );
        details.append(&bar);
        details.append(&status);
        let popover_box = gtk::Box::builder()
            .spacing(12)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(6)
            .margin_end(6)
            .width_request(320)
            .build();
        popover_box.append(&details);
        if cancellable {
            let cancel = gtk::Button::builder()
                .icon_name("window-close-symbolic")
                .tooltip_text(gettext("Cancel"))
                .valign(gtk::Align::Center)
                .css_classes(["circular"])
                .build();
            let p = progress.clone();
            cancel.connect_clicked(move |b| {
                p.cancel();
                b.set_sensitive(false);
            });
            popover_box.append(&cancel);
        }
        let popover = gtk::Popover::builder()
            .position(gtk::PositionType::Right)
            .child(&popover_box)
            .build();

        let row = gtk::MenuButton::builder()
            .child(&content)
            .popover(&popover)
            // The progress opens beside the sidebar, over the files, as in Files.
            .direction(gtk::ArrowType::Right)
            .css_classes(["flat"])
            .visible(false)
            .build();
        self.list.append(&row);
        let job = Rc::new(Job {
            progress,
            started: Instant::now(),
            row,
            stack,
            ring,
            fraction,
            bar,
            status,
        });
        self.jobs.borrow_mut().push(job.clone());
        if !self.ticking.replace(true) {
            glib::timeout_add_local(TICK, || match ui() {
                Some(ui) if ui.transfers.update() => glib::ControlFlow::Continue,
                ui => {
                    if let Some(ui) = ui {
                        ui.transfers.ticking.set(false);
                    }
                    glib::ControlFlow::Break
                }
            });
        }
        job
    }

    fn remove(&self, job: &Rc<Job>) {
        self.list.remove(&job.row);
        self.jobs.borrow_mut().retain(|j| !Rc::ptr_eq(j, job));
        self.update();
    }

    /// Show where the actions are; false when there are none.
    fn update(&self) -> bool {
        let jobs = self.jobs.borrow();
        let mut any_shown = false;
        for j in jobs.iter() {
            let (done, total) = (j.progress.done(), j.progress.total());
            let elapsed = j.started.elapsed();
            j.status.set_label(&if j.progress.is_cancelled() {
                gettext("Cancelling…")
            } else if total == 0 {
                String::new()
            } else {
                let progress = gettext("{done} / {total}")
                    .replace("{done}", &glib::format_size(done))
                    .replace("{total}", &glib::format_size(total));
                // The average since the start: no jumping about.
                let speed = done as f64 / elapsed.as_secs_f64();
                if elapsed >= ESTIMATE_AFTER && done > 0 && done < total {
                    let left = ((total - done) as f64 / speed).ceil() as u64;
                    gettext("{progress} — {left} ({speed}/s)")
                        .replace("{progress}", &progress)
                        .replace("{left}", &time_left(left))
                        .replace("{speed}", &glib::format_size(speed as u64))
                } else {
                    progress
                }
            });
            if total > 0 {
                let f = (done as f64 / total as f64).min(1.0);
                j.fraction.set(f);
                j.bar.set_fraction(f);
                j.stack.set_visible_child_name("ring");
                j.ring.queue_draw();
            } else {
                j.bar.pulse();
                j.stack.set_visible_child_name("spinner");
            }
            let shown = elapsed >= DELAY;
            j.row.set_visible(shown);
            any_shown |= shown;
        }
        self.list.set_margin_bottom(if any_shown { 6 } else { 0 });
        !jobs.is_empty()
    }
}

/// Run `work` in the background, listed as `title` while it goes; what it returns, None if it
/// panicked. `cancellable`: the list offers to stop it, which `work` sees in its `Progress`.
pub async fn run<T: Send + 'static>(
    ui: &Rc<Ui>,
    title: &str,
    cancellable: bool,
    work: impl FnOnce(&Client, &Progress) -> foldav_core::Result<T> + Send + 'static,
) -> Option<foldav_core::Result<T>> {
    let progress = Arc::new(Progress::default());
    let job = ui.transfers.add(title, cancellable, progress.clone());
    let r = gio::spawn_blocking(move || work(core(), &progress)).await;
    ui.transfers.remove(&job);
    r.ok()
}
