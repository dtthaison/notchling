//! `--render <view> --out <file.png>` (part U10): opens the view in a borderless, non-activating
//! window tucked at the bottom left of the main display (on screen, since a capture needs the
//! window drawn, but never key and never taking clicks), captures it after it has drawn, writes
//! the PNG and exits. Prints `wrote <path> (<w>x<h>, <bytes> bytes)` on success.

use std::path::Path;
use std::time::Duration;

use gpui::{
    point, prelude::*, px, size, AnyView, App, Application, Bounds, Context, Render, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
};
use objc2_app_kit::NSWindowStyleMask;

use super::{capture, dock, views};
use crate::dock::Policy;

/// How long the view gets to draw before the capture. GPUI compiles its shaders at launch
/// (`runtime_shaders`), so the first frame takes a moment.
const SETTLE: Duration = Duration::from_millis(900);

/// Holds the named view, whatever its type.
struct Root {
    view: AnyView,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
        self.view.clone()
    }
}

/// Runs the render; the exit code. Unknown views are refused by `cli::parse` first.
pub fn run(view: &str, out: &Path) -> i32 {
    let Some(spec) = crate::views::spec(view) else {
        eprintln!("unknown view {view}");
        return 2;
    };
    let (width, height) = spec.size;
    let name = view.to_owned();
    let out = out.to_path_buf();
    Application::new().run(move |cx: &mut App| {
        // No Dock icon for a render.
        dock::apply(Policy::Accessory);
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
            let view = views::build(&name, window, cx);
            cx.new(|cx| {
                cx.spawn(async move |_, cx| {
                    cx.background_executor().timer(SETTLE).await;
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
