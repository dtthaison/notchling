use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use gpui::{point, px, AsyncApp, Entity, Size};
use notchling_engine::transcript::tail::PARSED_BYTES;

use super::bench_events::Injector;
use super::bench_scroll::Scenario;
use super::render::{cpu_ms, FRAMES};
use crate::chat::scroll_trace;
use crate::chat::view::pane::{DETAILS_RENDERS, RENDERS, ROOT_RENDERS, ROWS, SIDEBAR_RENDERS};
use crate::chat::view::{bench_point, scroll_probe, ChatView};

pub const MODE_ENV: &str = "NOTCHLING_RENDER_BENCH_MODE";

pub const FORCE_ENV: &str = "NOTCHLING_BENCH_FORCE_FRAMES";

const WARM_UP: Duration = Duration::from_millis(1500);

const FORCED_EVERY: Duration = Duration::from_millis(16);

const SETTLE: Duration = Duration::from_millis(150);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Idle,
    Scroll,
    ScrollFade,
    Hover,
    Select,
    Typing,
    Terminal,
    Reload,
    Menu,
    Panels,
    Sending,
    Trace(Scenario),
}

impl Mode {
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
            "panels" => Mode::Panels,
            "sending" => Mode::Sending,
            _ => return Scenario::parse(name).map(Mode::Trace),
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
            Mode::Panels => "panels",
            Mode::Sending => "sending",
            Mode::Trace(scenario) => scenario.name(),
        }
    }

    fn rate(self) -> Option<u32> {
        match self {
            Mode::Idle | Mode::ScrollFade | Mode::Sending => None,
            Mode::Scroll | Mode::Trace(_) => Some(60),
            Mode::Hover | Mode::Select | Mode::Terminal | Mode::Panels => Some(30),
            Mode::Typing => Some(10),
            Mode::Reload => Some(2),
            Mode::Menu => Some(5),
        }
    }
}

const MOUSE_STEP: f32 = 7.0;
const WHEEL_STEP: f32 = 60.0;
const WHEEL_RUN: u64 = 30;
const WHEEL_START: f32 = 2000.0;
const LANE_TOP: f32 = 90.0;
const LANE_BOTTOM: f32 = 190.0;

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

pub(super) fn lane(size: Size<gpui::Pixels>) -> (gpui::Point<gpui::Pixels>, f32) {
    let middle = bench_point(size);
    let span = (f32::from(size.height) - LANE_TOP - LANE_BOTTOM).max(MOUSE_STEP);
    (point(middle.x, px(LANE_TOP)), span)
}

fn step(mode: Mode, n: u64, target: &mut Target, cx: &mut AsyncApp) {
    let middle = bench_point(target.size);
    let (top, span) = lane(target.size);
    match mode {
        Mode::Idle | Mode::ScrollFade | Mode::Sending => {}
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
        Mode::Trace(scenario) => {
            let dy = scenario.delta(n);
            if let (true, Some(injector)) = (dy != 0.0, target.injector.as_mut()) {
                injector.scroll(middle.x, middle.y, dy);
                scroll_probe::injected(dy);
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
        Mode::Panels => target.chat(cx, |chat, cx| chat.bench_panels(n, cx)),
    }
}

async fn prepare(mode: Mode, target: &mut Target, cx: &mut AsyncApp) {
    let middle = bench_point(target.size);
    match mode {
        Mode::Scroll => {
            if let Some(injector) = target.injector.as_mut() {
                injector.scroll(middle.x, middle.y, WHEEL_START);
            }
        }
        Mode::Trace(scenario) => {
            let prime = scenario.prime();
            if let (true, Some(injector)) = (prime != 0.0, target.injector.as_mut()) {
                injector.scroll(middle.x, middle.y, prime);
            }
        }
        Mode::ScrollFade => {
            if let Some(injector) = target.injector.as_mut() {
                injector.scroll(middle.x, middle.y, WHEEL_START);
            }
            cx.background_executor().timer(WARM_UP).await;
            for n in 0..60 {
                step(Mode::Scroll, n, target, cx);
                cx.background_executor()
                    .timer(Duration::from_secs_f64(1.0 / 60.0))
                    .await;
            }
            return;
        }
        Mode::Select => {
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
        Mode::Sending => {
            let until = Instant::now() + Duration::from_secs(5);
            let mut sending = false;
            while !sending && Instant::now() < until {
                target.chat(cx, |chat, cx| sending = chat.bench_sending(cx));
                if !sending {
                    cx.background_executor().timer(FORCED_EVERY).await;
                }
            }
        }
        _ => {}
    }
    cx.background_executor().timer(WARM_UP).await;
}

fn finish(mode: Mode, target: &Target) {
    if let (Mode::Select, Some(injector)) = (mode, &target.injector) {
        let (top, span) = lane(target.size);
        injector.release(top.x, top.y + px(wave(0, false, span)));
    }
}

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

pub async fn keep_drawing(target: &Target, cx: &mut AsyncApp) -> Option<Rc<Cell<bool>>> {
    let injector = target.injector.as_ref()?;
    needs_forcing(target, cx)
        .await
        .then(|| force_frames(injector.frame_driver(), cx))
}

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
    if matches!(mode, Mode::Trace(_)) {
        scroll_probe::enable(|| FRAMES.load(Ordering::Relaxed));
    }
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
            let script = match mode {
                Mode::Trace(scenario) => Some((
                    scenario.steps(),
                    until + Duration::from_secs_f64(seconds * 3.0),
                )),
                _ => None,
            };
            let mut n = 0;
            loop {
                let more = match script {
                    Some((steps, cap)) => n < steps && Instant::now() < cap,
                    None => Instant::now() < until,
                };
                if !more {
                    break;
                }
                step(mode, n, &mut target, cx);
                n += 1;
                cx.background_executor().timer(every).await;
            }
            if script.is_some() {
                cx.background_executor().timer(SETTLE).await;
            }
            finish(mode, &target);
        }
    }
    let frames = FRAMES.load(Ordering::Relaxed) - frames;
    let after = drawn();
    let forced = forcing.is_some();
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
    let scroll = matches!(mode, Mode::Trace(_)).then(|| {
        let recording = scroll_probe::finish();
        super::bench_scroll::dump(&recording);
        let mut report =
            scroll_trace::report(&recording.frames, &recording.steps, recording.viewport).json();
        report["draws"] = scroll_trace::draw_report(&recording.draws).json();
        report
    });
    let mut json = serde_json::json!({
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
        "native_rings": super::native_spin::probe().len(),
    });
    if let Some(scroll) = scroll {
        json["scroll"] = scroll;
    }
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
            Mode::Sending,
        ]
        .into_iter()
        .chain(Scenario::ALL.map(Mode::Trace));
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
