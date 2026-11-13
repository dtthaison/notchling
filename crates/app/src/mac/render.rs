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

const SETTLE: Duration = Duration::from_millis(900);

pub const APPEARANCE_ENV: &str = "NOTCHLING_RENDER_APPEARANCE";
pub const PREFERENCE_ENV: &str = "NOTCHLING_RENDER_APPEARANCE_SETTING";

pub const SCROLLBARS_ENV: &str = "NOTCHLING_RENDER_SCROLLBARS";

pub const BENCH_ENV: &str = "NOTCHLING_RENDER_BENCH";

pub const CHECK_BLOCKS_ENV: &str = "NOTCHLING_RENDER_CHECK_BLOCKS";

pub const CHECK_COPY_ENV: &str = "NOTCHLING_RENDER_CHECK_COPY";

pub const FORCE_FRAMES_ENV: &str = "NOTCHLING_RENDER_FORCE_FRAMES";

pub const NATIVE_PROBE_ENV: &str = "NOTCHLING_RENDER_NATIVE_PROBE";

pub(super) static FRAMES: AtomicU64 = AtomicU64::new(0);

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
    if unsafe { getrusage(0, &mut usage) } != 0 {
        return 0.0;
    }
    let ms = |t: &TimeVal| t.sec as f64 * 1000.0 + t.usec as f64 / 1000.0;
    ms(&usage.user) + ms(&usage.system)
}

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

struct Root {
    view: AnyView,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
        FRAMES.fetch_add(1, Ordering::Relaxed);
        crate::chat::view::scroll_probe::draw_started();
        self.view.clone()
    }
}

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
        crate::mac::app_icons::install();
        dock::apply(Policy::Accessory);
        dock::bind_keys(cx);
        if std::env::var_os(SCROLLBARS_ENV).is_some_and(|v| v == "1") {
            cx.set_global(notchling_kit::ui::PinScrollbars);
        }
        if let Some(appearance) = forced {
            force_appearance(appearance);
        }
        if let Some(chosen) = std::env::var(PREFERENCE_ENV)
            .ok()
            .and_then(|v| notchling_kit::Preference::parse(&v))
        {
            notchling_kit::set_preference(chosen);
            if chosen != notchling_kit::Preference::System {
                super::appearance::set_native(chosen);
            }
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

async fn check_copy(target: &bench::Target, cx: &mut gpui::AsyncApp, how: &str) {
    let fail = |message: String| -> ! {
        println!("copy-check: {message}");
        std::process::exit(1);
    };
    let (Some(chat), Some(injector)) = (target.chat.clone(), target.injector.as_ref()) else {
        fail("this view is not a Chat with a window to send events to".to_owned());
    };
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
        top.x = target.size.width - px(260.0);
    }
    let step = Duration::from_millis(30);
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
            frames_drawn(3, cx).await;
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

struct Empty;

impl Render for Empty {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
        gpui::div()
    }
}
