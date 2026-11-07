//! S4: a PNG of a GPUI view with no person involved. GPUI 0.2.2 has no offscreen
//! path (`TestWindow::draw` is a no-op), so this opens the view in its own
//! non-activating window and captures that window with `CGWindowListCreateImage`.

use std::time::Duration;

use gpui::{
    div, point, prelude::*, px, size, App, Application, Bounds, Context, WindowBackgroundAppearance,
    WindowBounds, WindowKind, WindowOptions,
};
use objc2::rc::Retained;
use objc2::AllocAnyThread;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSView, NSWindow};
use objc2_core_graphics::{CGRectNull, CGWindowImageOption, CGWindowListCreateImage, CGWindowListOption};
use objc2_foundation::NSDictionary;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

const OPEN: (f32, f32) = (400., 120.);

/// The Task 2 shape at its expanded size.
struct Shape;

impl Render for Shape {
    fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().justify_center().items_start().child(
            div()
                .w(px(OPEN.0))
                .h(px(OPEN.1))
                .bg(gpui::black())
                .rounded_b(px(18.))
                .text_color(gpui::white())
                .flex()
                .justify_center()
                .items_end()
                .child("clicks: 0"),
        )
    }
}

fn ns_window(window: &gpui::Window) -> Option<Retained<NSWindow>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return None };
    let view: &NSView = unsafe { h.ns_view.cast::<NSView>().as_ref() };
    view.window()
}

#[allow(deprecated)]
fn capture(window: &NSWindow, path: &std::path::Path) -> Result<usize, String> {
    let id = window.windowNumber() as u32;
    let image = CGWindowListCreateImage(
        unsafe { CGRectNull },
        CGWindowListOption::OptionIncludingWindow,
        id,
        CGWindowImageOption::BoundsIgnoreFraming | CGWindowImageOption::BestResolution,
    )
    .ok_or("CGWindowListCreateImage returned nothing")?;
    let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &image);
    let data = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }
        .ok_or("PNG encoding failed")?;
    let bytes = data.to_vec();
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(path, &bytes).map_err(|e| e.to_string())?;
    Ok(bytes.len())
}

fn main() {
    Application::new().run(|cx: &mut App| {
        // On screen (a capture needs the window drawn) but tucked at the bottom left.
        let bounds = Bounds { origin: point(px(0.), px(700.)), size: size(px(OPEN.0 + 40.), px(OPEN.1 + 20.)) };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            window_background: WindowBackgroundAppearance::Transparent,
            focus: false,
            kind: WindowKind::PopUp,
            is_movable: false,
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            let ns = ns_window(window);
            cx.new(|cx| {
                cx.spawn(async move |_, cx| {
                    cx.background_executor().timer(Duration::from_millis(800)).await;
                    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../out/notch.png");
                    match ns.as_deref().ok_or("no NSWindow".to_string()).and_then(|w| capture(w, &out)) {
                        Ok(n) => println!("S4 capture: wrote {} ({n} bytes)", out.display()),
                        Err(e) => println!("S4 capture failed: {e}"),
                    }
                    let _ = cx.update(|cx| cx.quit());
                })
                .detach();
                Shape
            })
        })
        .expect("open window");
    });
}
