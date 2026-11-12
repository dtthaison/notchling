//! `--render chat`'s bench (`NOTCHLING_RENDER_BENCH=<seconds>`, `NOTCHLING_RENDER_BENCH_MODE`):
//! keeps the window up, drives one kind of activity into it (a wheel, the mouse, typing, a
//! terminal's output, a reload) and prints what the window spent on it: the `bench:` line for a
//! person, then `bench-json:` with the same numbers and the counters for a test.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use gpui::{point, px, AsyncApp, Entity, Size};
use notchling_engine::transcript::tail::PARSED_BYTES;

use super::bench_events::Injector;
use super::render::{cpu_ms, FRAMES};
use crate::chat::view::pane::{DETAILS_RENDERS, RENDERS, ROOT_RENDERS, ROWS, SIDEBAR_RENDERS};
use crate::chat::view::{bench_point, ChatView};

/// The environment's name for what the driver does each step.
pub const MODE_ENV: &str = "NOTCHLING_RENDER_BENCH_MODE";

/// `1` forces frames by hand always, `0` never; unset: only when no display-link frame arrives
/// (a locked screen), which the bench finds out first.
pub const FORCE_ENV: &str = "NOTCHLING_BENCH_FORCE_FRAMES";

/// How long a mode runs before its numbers start.
const WARM_UP: Duration = Duration::from_millis(1500);

/// How often a forced frame is tried.
const FORCED_EVERY: Duration = Duration::from_millis(16);

/// What the driver does while the bench runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Nothing: the working session's spinners still turn.
    Idle,
    /// A wheel over the conversation: 2000 pt up once, then 60 pt every step, up for 30 steps
    /// and down for 30.
    Scroll,
    /// A second of the `Scroll` wheel before the numbers start, then nothing: what is measured is
    /// the scroll bar holding and fading (`HOLD_SECONDS`, `FADE_SECONDS`).
    ScrollFade,
    /// The mouse over the conversation, 7 pt down a step, wrapping.
    Hover,
    /// A press on a message, dragged 7 pt a step down and back, released at the end.
    Select,
    /// An `a` typed into the reply box.
    Typing,
    /// The render's terminal panel, one line fed to it.
    Terminal,
    /// A reload that finds nothing changed.
    Reload,
    /// The account menu opened or closed.
    Menu,
}

impl Mode {
    /// The mode a name says, or `None`.
    pub fn parse(name: &str) -> Option<Mode> {
        Some(match name {
            "idle" => Mode::Idle,
            "scroll" => Mode::Scroll,
            "scroll-fade" => Mode::ScrollFade,
            "hover" => Mode::Hover,
            "select" => Mode::Select,
            "typing" => Mode::Typing,
            "terminal" => Mode::Terminal,
            "reload" => Mode::Reload,
            "menu" => Mode::Menu,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Idle => "idle",
            Mode::Scroll => "scroll",
            Mode::ScrollFade => "scroll-fade",
            Mode::Hover => "hover",
            Mode::Select => "select",
            Mode::Typing => "typing",
            Mode::Terminal => "terminal",
            Mode::Reload => "reload",
            Mode::Menu => "menu",
        }
    }

    /// Steps a second; `None` for a mode that does nothing.
    fn rate(self) -> Option<u32> {
        match self {
            Mode::Idle | Mode::ScrollFade => None,
            Mode::Scroll => Some(60),
            Mode::Hover | Mode::Select | Mode::Terminal => Some(30),
            Mode::Typing => Some(10),
            Mode::Reload => Some(2),
            Mode::Menu => Some(5),
        }
    }
}

/// How far a mouse step moves, in points.
const MOUSE_STEP: f32 = 7.0;
/// How far a wheel step moves, and how many steps go one way.
const WHEEL_STEP: f32 = 60.0;
const WHEEL_RUN: u64 = 30;
/// The first wheel movement, up from the end.
const WHEEL_START: f32 = 2000.0;
/// The stretch of the window the mouse covers: from just under the header to just above the reply
/// box, so a hover crosses message after message instead of staying inside one.
const LANE_TOP: f32 = 90.0;
const LANE_BOTTOM: f32 = 190.0;

/// What the bench drives: the window's view to send events to, the chat in it and the window's
/// size.
pub struct Target {
    pub injector: Option<Injector>,
    pub chat: Option<Entity<ChatView>>,
    pub size: Size<gpui::Pixels>,
}

impl Target {
    fn chat(&self, cx: &mut AsyncApp, f: impl FnOnce(&mut ChatView, &mut gpui::Context<ChatView>)) {
        if let Some(chat) = &self.chat {
            cx.update(|cx| chat.update(cx, f)).ok();
        }
    }
}

/// How far down the lane a step's point is: `step` mouse steps from its top. A hover wraps to the
/// top at the end; a drag turns round, so a long run stays inside the conversation.
fn wave(step: u64, wrap: bool, span: f32) -> f32 {
    let steps = ((span / MOUSE_STEP) as u64).max(1);
    let n = if wrap {
        step % steps
    } else {
        let cycle = step % (steps * 2);
        if cycle < steps {
            cycle
        } else {
            steps * 2 - cycle
        }
    };
    n as f32 * MOUSE_STEP
}

/// The lane's top point (in the conversation's column) and its length.
pub(super) fn lane(size: Size<gpui::Pixels>) -> (gpui::Point<gpui::Pixels>, f32) {
    let middle = bench_point(size);
    let span = (f32::from(size.height) - LANE_TOP - LANE_BOTTOM).max(MOUSE_STEP);
    (point(middle.x, px(LANE_TOP)), span)
}

/// One step of `mode`.
fn step(mode: Mode, n: u64, target: &mut Target, cx: &mut AsyncApp) {
    let middle = bench_point(target.size);
    let (top, span) = lane(target.size);
    match mode {
        Mode::Idle | Mode::ScrollFade => {}
        Mode::Scroll => {
            let dy = if (n / WHEEL_RUN).is_multiple_of(2) {
                WHEEL_STEP
            } else {
                -WHEEL_STEP
            };
            if let Some(injector) = target.injector.as_mut() {
                injector.scroll(middle.x, middle.y, dy);
            }
        }
        Mode::Hover => {
            if let Some(injector) = &target.injector {
                injector.moved(top.x, top.y + px(wave(n, true, span)));
            }
        }
        Mode::Select => {
            if let Some(injector) = &target.injector {
                injector.drag(top.x, top.y + px(wave(n + 1, false, span)));
            }
        }
        Mode::Typing => target.chat(cx, |chat, cx| chat.bench_type("a", cx)),
        Mode::Terminal => target.chat(cx, |chat, _| chat.bench_terminal_feed()),
        Mode::Reload => target.chat(cx, |chat, cx| chat.bench_reload(cx)),
        Mode::Menu => target.chat(cx, |chat, cx| chat.bench_menu(cx)),
    }
}

/// What a mode needs before the numbers start.
async fn prepare(mode: Mode, target: &mut Target, cx: &mut AsyncApp) {
    let middle = bench_point(target.size);
    match mode {
        Mode::Scroll => {
            if let Some(injector) = target.injector.as_mut() {
                injector.scroll(middle.x, middle.y, WHEEL_START);
            }
        }
        Mode::ScrollFade => {
            if let Some(injector) = target.injector.as_mut() {
                injector.scroll(middle.x, middle.y, WHEEL_START);
            }
            cx.background_executor().timer(WARM_UP).await;
            // A second of wheel, then straight into the measured time: the bar holds, then fades.
            for n in 0..60 {
                step(Mode::Scroll, n, target, cx);
                cx.background_executor()
                    .timer(Duration::from_secs_f64(1.0 / 60.0))
                    .await;
            }
            return;
        }
        Mode::Select => {
            // The conversation loads in the background: a press before its first draw lands on
            // nothing, so wait for rows (a second or so, a locked screen included).
            let until = Instant::now() + Duration::from_secs(5);
            while ROWS.load(Ordering::Relaxed) == 0 && Instant::now() < until {
                cx.background_executor().timer(FORCED_EVERY).await;
            }
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            if let Some(injector) = &target.injector {
                injector.press(lane(target.size).0.x, lane(target.size).0.y);
            }
        }
        Mode::Terminal => target.chat(cx, |chat, cx| chat.bench_terminal_start(cx)),
        _ => {}
    }
    // The first frames after a change draw it for the first time, and the window's own first
    // reloads land in its first seconds: not what the bench measures.
    cx.background_executor().timer(WARM_UP).await;
}

/// Where a mode ends: a drag lets go.
fn finish(mode: Mode, target: &Target) {
    if let (Mode::Select, Some(injector)) = (mode, &target.injector) {
        let (top, span) = lane(target.size);
        injector.release(top.x, top.y + px(wave(0, false, span)));
    }
}

/// Whether the bench has to draw the window itself: the window drew nothing in a moment after it
/// was told to (a locked screen sends the display link no frames).
async fn needs_forcing(target: &Target, cx: &mut AsyncApp) -> bool {
    match std::env::var(FORCE_ENV).as_deref() {
        Ok("1") => return true,
        Ok("0") => return false,
        _ => {}
    }
    let before = FRAMES.load(Ordering::Relaxed);
    target.chat(cx, |chat, cx| chat.bench_chrome(cx));
    cx.background_executor()
        .timer(Duration::from_millis(300))
        .await;
    FRAMES.load(Ordering::Relaxed) == before
}

/// For a `--render` that must see the window draw after its first frame (a locked screen sends no
/// display-link frames, so the load that fills the conversation would never show): the same
/// forcing the bench does, when the window needs it. Set the returned flag to stop.
pub async fn keep_drawing(target: &Target, cx: &mut AsyncApp) -> Option<Rc<Cell<bool>>> {
    let injector = target.injector.as_ref()?;
    needs_forcing(target, cx)
        .await
        .then(|| force_frames(injector.frame_driver(), cx))
}

/// Draws the window every 16 ms (it draws only when dirty) until the returned flag is set.
fn force_frames(driver: super::bench_events::FrameDriver, cx: &mut AsyncApp) -> Rc<Cell<bool>> {
    let stop = Rc::new(Cell::new(false));
    let done = stop.clone();
    cx.spawn(async move |cx| {
        while !done.get() {
            driver.frame();
            cx.background_executor().timer(FORCED_EVERY).await;
        }
    })
    .detach();
    stop
}

/// Keeps the window up for `seconds` driving `mode`, then prints what it drew and spent. Prints
/// `bench: ready` first, for a test that has to start something of its own.
pub async fn bench(seconds: f64, mode: Mode, mut target: Target, cx: &mut AsyncApp) {
    println!("bench: ready");
    let forcing = match target.injector.as_ref() {
        Some(injector) if needs_forcing(&target, cx).await => {
            Some(force_frames(injector.frame_driver(), cx))
        }
        _ => None,
    };
    prepare(mode, &mut target, cx).await;
    let drawn = || {
        [
            &RENDERS,
            &SIDEBAR_RENDERS,
            &DETAILS_RENDERS,
            &ROOT_RENDERS,
            &ROWS,
            &PARSED_BYTES,
        ]
        .map(|n| n.load(Ordering::Relaxed))
    };
    let (frames, before) = (FRAMES.load(Ordering::Relaxed), drawn());
    let (cpu, at) = (cpu_ms(), Instant::now());
    let until = at + Duration::from_secs_f64(seconds);
    match mode.rate() {
        None => {
            cx.background_executor()
                .timer(Duration::from_secs_f64(seconds))
                .await
        }
        Some(rate) => {
            let every = Duration::from_secs_f64(1.0 / f64::from(rate));
            let mut n = 0;
            while Instant::now() < until {
                step(mode, n, &mut target, cx);
                n += 1;
                cx.background_executor().timer(every).await;
            }
            finish(mode, &target);
        }
    }
    let frames = FRAMES.load(Ordering::Relaxed) - frames;
    let after = drawn();
    let forced = forcing.is_some();
    // Whether the conversation is still at its newest message: one that follows must be.
    let at_end = match &target.chat {
        Some(chat) => cx
            .update(|cx| chat.read(cx).bench_at_end(cx))
            .unwrap_or(false),
        None => false,
    };
    if let Some(stop) = forcing {
        stop.set(true);
    }
    let [conversations, sidebars, details, roots, rows, parsed] =
        [0, 1, 2, 3, 4, 5].map(|i| after[i] - before[i]);
    let (cpu, wall) = (cpu_ms() - cpu, at.elapsed().as_secs_f64());
    println!(
        "bench: {frames} frames in {wall:.1} s ({conversations} drew the conversation, {sidebars} the sidebar, {details} the details), {cpu:.0} ms CPU ({:.2} ms/frame, {:.1} fps, {:.1}% of a core)",
        cpu / frames.max(1) as f64,
        frames as f64 / wall,
        cpu / wall / 10.0
    );
    let json = serde_json::json!({
        "mode": mode.name(),
        "seconds": (wall * 10.0).round() / 10.0,
        "frames": frames,
        "cpu_ms": cpu.round(),
        "ms_per_frame": (cpu / frames.max(1) as f64 * 100.0).round() / 100.0,
        "root": roots,
        "conversation": conversations,
        "sidebar": sidebars,
        "details": details,
        "rows": rows,
        "parsed_bytes": parsed,
        "forced": forced,
        "at_end": at_end,
        // Spinner rings Core Animation turns for the window: the working session's Activity Line.
        "native_rings": super::native_spin::probe().len(),
    });
    println!("bench-json: {json}");
}

#[cfg(test)]
mod tests {
    use gpui::size;

    use super::*;

    #[test]
    fn every_mode_parses_and_nothing_else_does() {
        let modes = [
            Mode::Idle,
            Mode::Scroll,
            Mode::ScrollFade,
            Mode::Hover,
            Mode::Select,
            Mode::Typing,
            Mode::Terminal,
            Mode::Reload,
            Mode::Menu,
        ];
        for mode in modes {
            assert_eq!(Mode::parse(mode.name()), Some(mode));
        }
        for bad in ["", "Idle", "scrolling", "append", " idle"] {
            assert_eq!(Mode::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_mouse_waves_stay_inside_the_lane() {
        let span = 500.0;
        for n in 0..500 {
            for wrap in [true, false] {
                let y = wave(n, wrap, span);
                assert!((0.0..=span).contains(&y), "{n} {wrap} {y}");
            }
        }
        assert_eq!(wave(0, false, span), 0.0);
        assert!(wave(2, false, span) > wave(1, false, span));
        let (top, long) = lane(size(px(1240.0), px(780.0)));
        assert_eq!(f32::from(top.y), LANE_TOP);
        assert_eq!(long, 500.0);
    }
}
