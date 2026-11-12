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
                    cx.background_executor().timer(SETTLE).await;
                    if let Some(seconds) = bench_seconds {
                        bench::bench(seconds, mode, target, cx).await;
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

/// Drawn when a view name isn't known here (the CLI refuses those first).
struct Empty;

impl Render for Empty {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
        gpui::div()
    }
}
