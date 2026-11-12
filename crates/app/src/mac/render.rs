//! `--render <view> --out <file.png>` (part U10): opens the view in a borderless, non-activating
//! window tucked at the bottom left of the main display (on screen, since a capture needs the
//! window drawn, but never key and never taking clicks), captures it after it has drawn, writes
//! the PNG and exits. Prints `wrote <path> (<w>x<h>, <bytes> bytes)` on success.
//! `NOTCHLING_RENDER_APPEARANCE=light|dark` draws the view in that appearance instead of the
//! system's, for checking both without changing the Mac's setting. `NOTCHLING_RENDER_SCROLLBARS=1`
//! draws every overlay scroll bar as if it had just scrolled, since a still frame has no scroll.
//! `NOTCHLING_RENDER_BENCH=<seconds>` keeps the view up that long before the capture, driving
//! the activity `NOTCHLING_RENDER_BENCH_MODE` names (`bench`), and prints
//! `bench: <frames> frames in <s> s (<n> drew the conversation, <n> the sidebar, <n> the details), <cpu> ms CPU (<ms>/frame, <fps> fps, <pct>% of a core)`: what
//! the window costs while nothing happens but what it animates (a spinner, a clock, a reload).

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use gpui::{
    point, prelude::*, px, size, AnyView, App, Application, Bounds, Context, Render, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
};
use notchling_kit::Appearance;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSWindowStyleMask,
};

use super::bench::{self, Mode};
use super::{capture, dock, views};
use crate::dock::Policy;

/// How long the view gets to draw before the capture. GPUI compiles its shaders at launch
/// (`runtime_shaders`), so the first frame takes a moment.
const SETTLE: Duration = Duration::from_millis(900);

/// The environment's name for the appearance to render in.
pub const APPEARANCE_ENV: &str = "NOTCHLING_RENDER_APPEARANCE";

/// The environment's name for the switch that pins the scroll bars on.
pub const SCROLLBARS_ENV: &str = "NOTCHLING_RENDER_SCROLLBARS";

/// The environment's name for the bench's length in seconds.
pub const BENCH_ENV: &str = "NOTCHLING_RENDER_BENCH";

/// The environment's name for the switch that checks the conversation's selectable texts against
/// the model's blocks (`ChatView::check_blocks`) and prints `blocks-check: ok <n>`.
pub const CHECK_BLOCKS_ENV: &str = "NOTCHLING_RENDER_CHECK_BLOCKS";

/// The environment's name for the switch that selects text in the conversation with the pointer,
/// copies it and checks the clipboard (`check_copy`), printing `copy-check: ok <n>`: `1` or `key`
/// sends ⌘C to the window, `action` dispatches the Copy action, as the Edit menu's item does, and
/// `menu` right-clicks the text and clicks Copy in the menu that opens. `NOTCHLING_RENDER_CHECK_COPY_IN=file`
/// does it in the file panel (with `NOTCHLING_RENDER_FILE`) instead of the conversation.
pub const CHECK_COPY_ENV: &str = "NOTCHLING_RENDER_CHECK_COPY";

/// The environment's name for the switch that draws frames by hand when the screen sends none
/// (locked), so a render sees what loads after the first frame.
pub const FORCE_FRAMES_ENV: &str = "NOTCHLING_RENDER_FORCE_FRAMES";

/// The environment's name for the switch that reads Core Animation's spinner layers (where each
/// ring is and how far its presentation layer has turned, three times 150 ms apart) before the
/// capture and prints `native-spin: {...}`.
pub const NATIVE_PROBE_ENV: &str = "NOTCHLING_RENDER_NATIVE_PROBE";

/// Frames the window has drawn: the root view renders once per frame, whatever made it.
pub(super) static FRAMES: AtomicU64 = AtomicU64::new(0);

/// The process's CPU time so far (user and system), in milliseconds.
pub(super) fn cpu_ms() -> f64 {
    #[repr(C)]
    struct TimeVal {
        sec: i64,
        usec: i32,
    }
    #[repr(C)]
    struct RUsage {
        user: TimeVal,
        system: TimeVal,
        rest: [i64; 14],
    }
    extern "C" {
        fn getrusage(who: i32, usage: *mut RUsage) -> i32;
    }
    let mut usage = RUsage {
        user: TimeVal { sec: 0, usec: 0 },
        system: TimeVal { sec: 0, usec: 0 },
        rest: [0; 14],
    };
    // RUSAGE_SELF is 0; the struct is the size and layout of macOS's `struct rusage`.
    if unsafe { getrusage(0, &mut usage) } != 0 {
        return 0.0;
    }
    let ms = |t: &TimeVal| t.sec as f64 * 1000.0 + t.usec as f64 / 1000.0;
    ms(&usage.user) + ms(&usage.system)
}

/// Forces the app's appearance, so every window (and `window_appearance`) follows it.
fn force_appearance(appearance: Appearance) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let name = match appearance {
        Appearance::Light => unsafe { NSAppearanceNameAqua },
        Appearance::Dark => unsafe { NSAppearanceNameDarkAqua },
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setAppearance(NSAppearance::appearanceNamed(name).as_deref());
}

/// Holds the named view, whatever its type.
struct Root {
    view: AnyView,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
        FRAMES.fetch_add(1, Ordering::Relaxed);
        self.view.clone()
    }
}

/// Runs the render; the exit code. Unknown views are refused by `cli::parse` first.
pub fn run(view: &str, out: &Path, data_root: Option<&Path>) -> i32 {
    let Some(spec) = crate::views::spec(view) else {
        eprintln!("unknown view {view}");
        return 2;
    };
    let (width, height) = spec.size;
    let name = view.to_owned();
    let out = out.to_path_buf();
    let data_root = data_root.map(Path::to_path_buf);
    let forced = std::env::var(APPEARANCE_ENV)
        .ok()
        .and_then(|v| Appearance::parse(&v));
    let bench_seconds = std::env::var(BENCH_ENV)
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|s| *s > 0.0);
    let mode = match std::env::var(bench::MODE_ENV) {
        Ok(name) => match Mode::parse(&name) {
            Some(mode) => mode,
            None => {
                eprintln!("unknown bench mode {name}");
                return 2;
            }
        },
        Err(_) => Mode::Idle,
    };
    let app = Application::new().with_assets(notchling_kit::ui::KitAssets);
    app.run(move |cx: &mut App| {
        // Real app icons from this Mac, as the live app draws them; a render over made-up data
        // still differs per machine.
        crate::mac::app_icons::install();
        // No Dock icon for a render.
        dock::apply(Policy::Accessory);
        // The keys the real window binds, so a check that sends ⌘C sends what a person's does.
        dock::bind_keys(cx);
        if std::env::var_os(SCROLLBARS_ENV).is_some_and(|v| v == "1") {
            cx.set_global(notchling_kit::ui::PinScrollbars);
        }
        if let Some(appearance) = forced {
            force_appearance(appearance);
        }
        let origin = cx
            .primary_display()
            .map(|display| {
                let b = display.bounds();
                point(b.origin.x, b.origin.y + b.size.height - px(height))
            })
            .unwrap_or_else(|| point(px(0.0), px(0.0)));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin,
                size: size(px(width), px(height)),
            })),
            titlebar: None,
            focus: false,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Opaque,
            ..Default::default()
        };
        let opened = cx.open_window(options, |window, cx| {
            let ns = capture::ns_window(window);
            if let Some(ns) = &ns {
                // GPUI makes every window Titled, even with no title bar: drop that so the capture
                // is the view alone, and let clicks fall through for the moment it's on screen.
                ns.setStyleMask(
                    NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
                );
                ns.setHasShadow(false);
                ns.setIgnoresMouseEvents(true);
            }
            let view = views::build(&name, data_root.as_deref(), window, cx);
            let target = bench::Target {
                injector: super::bench_events::Injector::new(window),
                chat: view
                    .clone()
                    .and_then(|view| view.downcast::<crate::chat::view::ChatView>().ok()),
                size: size(px(width), px(height)),
            };
            cx.new(|cx| {
                cx.spawn(async move |_, cx| {
                    // A locked screen sends no frames: draw them, so what loads after the first
                    // frame shows (the blocks check and a capture need it).
                    let draw = bench_seconds.is_none()
                        && (std::env::var_os(CHECK_BLOCKS_ENV).is_some()
                            || std::env::var_os(CHECK_COPY_ENV).is_some()
                            || std::env::var_os(FORCE_FRAMES_ENV).is_some_and(|v| v == "1"));
                    let forcing = if draw {
                        bench::keep_drawing(&target, cx).await
                    } else {
                        None
                    };
                    cx.background_executor().timer(SETTLE).await;
                    if std::env::var_os(CHECK_BLOCKS_ENV).is_some_and(|v| v == "1") {
                        check_blocks(&target, cx).await;
                    }
                    if std::env::var_os(NATIVE_PROBE_ENV).is_some_and(|v| v == "1") {
                        native_probe(cx).await;
                    }
                    if let Ok(how) = std::env::var(CHECK_COPY_ENV) {
                        check_copy(&target, cx, &how).await;
                    }
                    if let Some(seconds) = bench_seconds {
                        bench::bench(seconds, mode, target, cx).await;
                    }
                    if let Some(stop) = forcing {
                        stop.set(true);
                    }
                    let result = ns
                        .as_deref()
                        .ok_or_else(|| "no NSWindow behind the GPUI window".to_owned())
                        .and_then(|w| capture::write_png(w, &out));
                    match result {
                        Ok(capture) => {
                            println!(
                                "wrote {} ({}x{}, {} bytes)",
                                out.display(),
                                capture.width,
                                capture.height,
                                capture.bytes
                            );
                            cx.update(|cx| cx.quit()).ok();
                        }
                        Err(message) => {
                            eprintln!("render failed: {message}");
                            std::process::exit(1);
                        }
                    }
                })
                .detach();
                Root {
                    view: view.unwrap_or_else(|| cx.new(|_| Empty).into()),
                }
            })
        });
        if let Err(e) = opened {
            eprintln!("render failed: {e}");
            std::process::exit(1);
        }
    });
    0
}

/// Prints where the native spinner rings are and their rotation, three samples 150 ms apart.
async fn native_probe(cx: &mut gpui::AsyncApp) {
    let mut samples = Vec::new();
    for n in 0..3 {
        if n > 0 {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
        }
        samples.push(super::native_spin::probe());
    }
    let rings: Vec<serde_json::Value> = samples[0]
        .iter()
        .map(|first| {
            let rotation = |i: usize| {
                samples[i]
                    .iter()
                    .find(|p| p.key == first.key)
                    .and_then(|p| p.rotation)
            };
            serde_json::json!({
                "x": first.x, "y": first.y,
                "rotation": [rotation(0), rotation(1), rotation(2)],
            })
        })
        .collect();
    println!("native-spin: {}", serde_json::json!({ "rings": rings }));
}

/// Runs the blocks check once the conversation has drawn (it is loaded in the background, so an
/// empty or not yet drawn window is tried again for a few seconds); a mismatch ends the render with exit code 1.
async fn check_blocks(target: &bench::Target, cx: &mut gpui::AsyncApp) {
    let Some(chat) = target.chat.clone() else {
        println!("blocks-check: mismatch: this view is not a Chat");
        std::process::exit(1);
    };
    let mut last = Err("the window never drew".to_owned());
    for _ in 0..25 {
        last = cx
            .update(|cx| chat.read(cx).check_blocks(cx))
            .unwrap_or_else(|e| Err(e.to_string()));
        if matches!(last, Ok(n) if n > 0) {
            break;
        }
        cx.background_executor()
            .timer(Duration::from_millis(200))
            .await;
    }
    match last {
        Ok(n) if n > 0 => println!("blocks-check: ok {n}"),
        Ok(_) => {
            println!("blocks-check: mismatch: no selectable text was drawn");
            std::process::exit(1);
        }
        Err(message) => {
            println!("blocks-check: {message}");
            std::process::exit(1);
        }
    }
}

/// Presses in the conversation, drags down and lets go, copies (`how`: ⌘C sent to the window, or
/// the Copy action dispatched) and reads what the clipboard then holds, which must be what the
/// selection says it copies. Prints `copy-check: ok <n>` or why not and ends the render with exit
/// code 1. The clipboard is the Mac's real one: it holds a made-up line first, so a stale copy
/// can't pass, and the selection's text after.
async fn check_copy(target: &bench::Target, cx: &mut gpui::AsyncApp, how: &str) {
    let fail = |message: String| -> ! {
        println!("copy-check: {message}");
        std::process::exit(1);
    };
    let (Some(chat), Some(injector)) = (target.chat.clone(), target.injector.as_ref()) else {
        fail("this view is not a Chat with a window to send events to".to_owned());
    };
    // The conversation loads in the background: a press before its first draw lands on nothing.
    for _ in 0..50 {
        if crate::chat::view::pane::ROWS.load(Ordering::Relaxed) > 0 {
            break;
        }
        cx.background_executor()
            .timer(Duration::from_millis(100))
            .await;
    }
    cx.background_executor()
        .timer(Duration::from_millis(400))
        .await;
    let (mut top, span) = bench::lane(target.size);
    if std::env::var("NOTCHLING_RENDER_CHECK_COPY_IN").is_ok_and(|v| v == "file") {
        // The panel takes the right 520 pt: the drag runs through its middle.
        top.x = target.size.width - px(260.0);
    }
    let step = Duration::from_millis(30);
    // Start on the first line of text from the lane's top: a press between messages holds nothing.
    let mut expected = None;
    let mut pressed = top;
    for down in 0..40u32 {
        let y = top.y + px(down as f32 * 8.0);
        pressed = point(top.x - px(150.0), y);
        injector.press(pressed.x, pressed.y);
        for n in 1..=20u32 {
            injector.drag(top.x + px(150.0), y + px(span * n as f32 / 20.0));
            cx.background_executor().timer(step / 3).await;
        }
        injector.release(top.x + px(150.0), y + px(span));
        cx.background_executor().timer(step * 3).await;
        expected = cx
            .update(|cx| chat.read(cx).bench_selected_text(cx))
            .ok()
            .flatten();
        if expected.is_some() {
            break;
        }
    }
    let Some(expected) = expected else {
        fail("the drag selected nothing".to_owned());
    };
    let sentinel = "copy-check: nothing was copied";
    cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(sentinel.to_owned())))
        .ok();
    let how = match how {
        "action" => {
            cx.update(|cx| {
                if let Some(window) = cx.windows().first().copied() {
                    window
                        .update(cx, |_, window, cx| {
                            window.dispatch_action(Box::new(dock::Copy), cx)
                        })
                        .ok();
                }
            })
            .ok();
            "the Copy action"
        }
        "menu" => {
            // A right click on the text where the drag began: the menu opens at the pointer and
            // the selection stays.
            injector.right_click(pressed.x, pressed.y);
            frames_drawn(3, cx).await;
            let menu = cx
                .update(|cx| chat.read(cx).bench_copy_menu())
                .ok()
                .flatten();
            let Some((at, enabled)) = menu else {
                fail("a right click on selected text opened no menu".to_owned());
            };
            if !enabled {
                fail("the menu's Copy is off with text selected".to_owned());
            }
            let kept = cx
                .update(|cx| chat.read(cx).bench_selected_text(cx))
                .ok()
                .flatten();
            if kept.as_deref() != Some(expected.as_str()) {
                fail(format!(
                    "the right click changed the selection: it was {expected:?}, now {kept:?}"
                ));
            }
            // The menu is on screen once a few frames have drawn since it opened.
            frames_drawn(3, cx).await;
            // The menu's only item sits under its top left corner (a 6 pt pad, a row 26 pt high).
            injector.press(at.x + px(30.0), at.y + px(19.0));
            injector.release(at.x + px(30.0), at.y + px(19.0));
            "the right-click menu's Copy"
        }
        _ => {
            injector.command_key("c", 8);
            "⌘C"
        }
    };
    cx.background_executor().timer(step * 4).await;
    let copied = cx
        .update(|cx| cx.read_from_clipboard().and_then(|item| item.text()))
        .ok()
        .flatten();
    match copied {
        Some(text) if text == expected => println!("copy-check: ok {} chars ({how})", text.len()),
        other => fail(format!(
            "mismatch: the selection says {expected:?}; after {how} the clipboard has {other:?}"
        )),
    }
}

/// Waits until the window has drawn `n` more frames (two seconds at most): what a click on
/// something that just appeared needs, since the thing is hit-tested against the last frame.
async fn frames_drawn(n: u64, cx: &mut gpui::AsyncApp) {
    let until = FRAMES.load(Ordering::Relaxed) + n;
    for _ in 0..200 {
        if FRAMES.load(Ordering::Relaxed) >= until {
            return;
        }
        cx.background_executor()
            .timer(Duration::from_millis(10))
            .await;
    }
}

/// Drawn when a view name isn't known here (the CLI refuses those first).
struct Empty;

impl Render for Empty {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
        gpui::div()
    }
}
