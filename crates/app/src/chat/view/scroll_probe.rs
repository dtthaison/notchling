//! The bench's trajectory recorder for the conversation's list: while it is on (`enable`, called
//! by the bench's scroll modes, never by the app) every painted row notes where it is and a canvas
//! after the list closes the frame, so `chat::scroll_trace` can say what each draw showed against
//! what the wheel asked for. With it off the rows draw exactly as they do in the app: one
//! `enabled()` check per row, no canvas.
//!
//! What it reads is what painted (the rows' bounds), never the list's own bookkeeping, which
//! doesn't know where a row is while the list follows its end (`bounds_for_item` is `None` then).

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use gpui::{canvas, prelude::*, Bounds, ListState, Pixels, SharedString};

use crate::chat::scroll_trace::Frame;

static ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct Trace {
    started: Option<Instant>,
    /// The window's frame counter now.
    window_frames: Option<fn() -> u64>,
    /// How far the bench has scrolled so far.
    injected: f64,
    /// Each wheel step: when (ms) and the running total after it.
    steps: Vec<(f64, f64)>,
    /// The rows painted since the last frame closed.
    rows: Vec<(String, f32)>,
    frames: Vec<Frame>,
    /// The viewport of the latest frame.
    viewport: f32,
}

thread_local! {
    // Painting and the bench both run on the main thread.
    static TRACE: RefCell<Trace> = RefCell::new(Trace::default());
}

/// Whether the recorder is on.
pub(in crate::chat) fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Starts a recording: `window_frames` reads the window's frame counter.
pub fn enable(window_frames: fn() -> u64) {
    TRACE.with(|t| {
        *t.borrow_mut() = Trace {
            started: Some(Instant::now()),
            window_frames: Some(window_frames),
            ..Trace::default()
        }
    });
    ENABLED.store(true, Ordering::Relaxed);
}

/// The bench injected a wheel step of `dy` points (positive moves the content down).
pub fn injected(dy: f32) {
    if !enabled() {
        return;
    }
    TRACE.with(|t| {
        let t = &mut *t.borrow_mut();
        t.injected += f64::from(dy);
        let at = t
            .started
            .map_or(0.0, |s| s.elapsed().as_secs_f64() * 1000.0);
        let total = t.injected;
        t.steps.push((at, total));
    });
}

/// Stops recording; what was recorded: the frames, the wheel steps and the viewport's height.
pub fn finish() -> (Vec<Frame>, Vec<(f64, f64)>, f32) {
    ENABLED.store(false, Ordering::Relaxed);
    TRACE.with(|t| {
        let t = std::mem::take(&mut *t.borrow_mut());
        (t.frames, t.steps, t.viewport)
    })
}

/// A canvas filling its row that notes where the row painted (a row's key).
pub(super) fn row_canvas(key: SharedString) -> impl IntoElement {
    canvas(
        |bounds, _, _| bounds,
        move |bounds: Bounds<Pixels>, _, _, _| {
            TRACE.with(|t| {
                t.borrow_mut()
                    .rows
                    .push((key.to_string(), f32::from(bounds.top())));
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// A canvas filling the list's column, after the list: it closes the frame the rows noted.
/// `rows_built` is the running count of rows built (`pane::ROWS`).
pub(super) fn frame_canvas(state: ListState, rows_built: fn() -> u64) -> impl IntoElement {
    canvas(
        |bounds, _, _| bounds,
        move |bounds: Bounds<Pixels>, _, _, _| {
            let top = state.logical_scroll_top();
            let count = state.item_count();
            let scrolled = -f32::from(state.scroll_px_offset_for_scrollbar().y);
            TRACE.with(|t| {
                let t = &mut *t.borrow_mut();
                t.viewport = f32::from(bounds.size.height);
                let frame = Frame {
                    at_ms: t
                        .started
                        .map_or(0.0, |s| s.elapsed().as_secs_f64() * 1000.0),
                    injected: t.injected,
                    window_frame: t.window_frames.map_or(0, |f| f()),
                    rows: std::mem::take(&mut t.rows),
                    item_ix: top.item_ix,
                    offset_in_item: f32::from(top.offset_in_item),
                    scrolled,
                    at_end: top.item_ix >= count,
                    rows_built: rows_built(),
                };
                t.frames.push(frame);
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
