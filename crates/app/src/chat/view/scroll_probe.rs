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
use std::time::{Duration, Instant};

use gpui::{canvas, prelude::*, Bounds, ListState, Pixels, SharedString};

use crate::chat::scroll_trace::{Draw, Frame};

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
    /// Every window draw the frame driver timed.
    draws: Vec<Draw>,
    /// Time spent building row elements in the draw going on.
    build: Duration,
    /// When the draw going on began, and the thread's CPU time then.
    draw: Option<(Instant, f64)>,
    /// The thread's CPU time when the conversation's prepaint ended in the draw going on.
    prepaint_cpu: Option<f64>,
    /// The thread's CPU time when the last timed draw ended.
    last_end_cpu: f64,
    /// The row and list-change counters at the last draw, for what each draw added.
    last_rows: u64,
    last_changes: u64,
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
            last_end_cpu: thread_cpu_ms(),
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

/// What a recording holds.
pub struct Recording {
    pub frames: Vec<Frame>,
    /// Each wheel step: when (ms) and the running total after it.
    pub steps: Vec<(f64, f64)>,
    pub viewport: f32,
    pub draws: Vec<Draw>,
}

/// Stops recording and hands over what was recorded.
pub fn finish() -> Recording {
    ENABLED.store(false, Ordering::Relaxed);
    TRACE.with(|t| {
        let t = std::mem::take(&mut *t.borrow_mut());
        Recording {
            frames: t.frames,
            steps: t.steps,
            viewport: t.viewport,
            draws: t.draws,
        }
    })
}

/// A row's element took `took` to build (the list's `render_item`).
pub(super) fn built(took: Duration) {
    TRACE.with(|t| t.borrow_mut().build += took);
}

/// The calling thread's CPU time so far, in milliseconds: what a draw cost the main thread itself,
/// whatever else the Mac was running.
fn thread_cpu_ms() -> f64 {
    #[repr(C)]
    struct Timespec {
        sec: i64,
        nsec: i64,
    }
    extern "C" {
        fn clock_gettime(clock: u32, out: *mut Timespec) -> i32;
    }
    // CLOCK_THREAD_CPUTIME_ID on macOS.
    const THREAD_CPUTIME: u32 = 16;
    let mut now = Timespec { sec: 0, nsec: 0 };
    if unsafe { clock_gettime(THREAD_CPUTIME, &mut now) } != 0 {
        return 0.0;
    }
    now.sec as f64 * 1000.0 + now.nsec as f64 / 1_000_000.0
}

/// A window draw begins (the root view's render, the first thing in it): the clock for the draw
/// the conversation's frame canvas closes.
pub fn draw_started() {
    if !enabled() {
        return;
    }
    let cpu = thread_cpu_ms();
    TRACE.with(|t| {
        let t = &mut *t.borrow_mut();
        t.draw = Some((Instant::now(), cpu));
        t.build = Duration::ZERO;
    });
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
/// `counters` are the running counts of rows built and list changes (`pane`).
pub(super) fn frame_canvas(state: ListState, counters: fn() -> (u64, u64)) -> impl IntoElement {
    canvas(
        |bounds, _, _| {
            // The list before it has laid out its rows by now: what the draw cost so far.
            let cpu = thread_cpu_ms();
            TRACE.with(|t| t.borrow_mut().prepaint_cpu = Some(cpu));
            bounds
        },
        move |bounds: Bounds<Pixels>, _, _, _| {
            let top = state.logical_scroll_top();
            let count = state.item_count();
            let scrolled = -f32::from(state.scroll_px_offset_for_scrollbar().y);
            let cpu = thread_cpu_ms();
            let (rows_built, changes) = counters();
            TRACE.with(|t| {
                let t = &mut *t.borrow_mut();
                t.viewport = f32::from(bounds.size.height);
                // The draw this frame closes: from the root's render to here.
                if let (Some((began, began_cpu)), Some(origin)) = (t.draw.take(), t.started) {
                    t.draws.push(Draw {
                        at_ms: began.saturating_duration_since(origin).as_secs_f64() * 1000.0,
                        wall_ms: began.elapsed().as_secs_f64() * 1000.0,
                        cpu_ms: cpu - began_cpu,
                        between_cpu_ms: (began_cpu - t.last_end_cpu).max(0.0),
                        rows: rows_built.saturating_sub(t.last_rows),
                        changes: changes.saturating_sub(t.last_changes),
                        build_ms: t.build.as_secs_f64() * 1000.0,
                        prepaint_ms: t.prepaint_cpu.take().map_or(0.0, |at| at - began_cpu),
                    });
                    t.last_end_cpu = cpu;
                    t.last_rows = rows_built;
                    t.last_changes = changes;
                }
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
                    at_top: top.item_ix == 0 && f32::from(top.offset_in_item) < 0.5,
                    rows_built,
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
