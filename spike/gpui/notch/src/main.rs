//! S1: a GPUI pop-up panel around the camera that lets clicks outside its shape
//! fall through, like `NotchController`.

use std::time::Duration;

use gpui::{
    div, point, prelude::*, px, size, App, Application, Bounds, Context, MouseButton,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
};
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSScreen, NSStatusWindowLevel, NSView, NSWindow, NSWindowCollectionBehavior};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use spike_core::notch::{hit_test, notch_rect, Rect, ScreenInfo};

const CANVAS: (f64, f64) = (560., 640.);
const OPEN: (f64, f64) = (400., 120.);
const SLACK: f64 = 4.;

/// The display with a notch (else the main one) as AppKit sees it.
fn screen_info(mtm: MainThreadMarker) -> ScreenInfo {
    let screens = NSScreen::screens(mtm);
    let screen = screens
        .iter()
        .find(|s| s.safeAreaInsets().top > 0.)
        .or_else(|| NSScreen::mainScreen(mtm))
        .expect("a screen");
    let f = screen.frame();
    let width = |w: f64| (w > 0.).then_some(w);
    ScreenInfo {
        frame: Rect { x: f.origin.x, y: f.origin.y, w: f.size.width, h: f.size.height },
        safe_top: screen.safeAreaInsets().top,
        aux_left_width: width(screen.auxiliaryTopLeftArea().size.width),
        aux_right_width: width(screen.auxiliaryTopRightArea().size.width),
    }
}

struct Notch {
    ns_window: Option<Retained<NSWindow>>,
    /// The collapsed shape in AppKit screen coordinates.
    notch: Rect,
    hovered: bool,
    clicks: u32,
}

impl Notch {
    fn shape(&self) -> Rect {
        if !self.hovered {
            return self.notch;
        }
        let top = self.notch.y + self.notch.h;
        let mid = self.notch.x + self.notch.w / 2.;
        Rect { x: mid - OPEN.0 / 2., y: top - OPEN.1, w: OPEN.0, h: OPEN.1 }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let m = NSEvent::mouseLocation();
        let slack = if self.hovered { 0. } else { SLACK };
        let inside = hit_test(self.shape(), (m.x, m.y), slack);
        if let Some(w) = &self.ns_window {
            w.setIgnoresMouseEvents(!inside);
        }
        if inside != self.hovered {
            self.hovered = inside;
            eprintln!("hover {inside}");
            cx.notify();
        }
    }
}

impl Render for Notch {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = self.shape();
        let label = if self.hovered { format!("clicks: {}", self.clicks) } else { String::new() };
        div().size_full().flex().justify_center().items_start().child(
            div()
                .w(px(s.w as f32))
                .h(px(s.h as f32))
                .bg(gpui::black())
                .rounded_b(px(if self.hovered { 18. } else { 10. }))
                .text_color(gpui::white())
                .flex()
                .justify_center()
                .items_end()
                .child(label)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.clicks += 1;
                        eprintln!("click inside #{}", this.clicks);
                        cx.notify();
                    }),
                ),
        )
    }
}

/// The NSWindow behind a GPUI window, through its raw handle.
fn ns_window(window: &gpui::Window) -> Option<Retained<NSWindow>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(h) = handle.as_raw() else { return None };
    let view: &NSView = unsafe { h.ns_view.cast::<NSView>().as_ref() };
    view.window()
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let mtm = MainThreadMarker::new().expect("main thread");
        let info = screen_info(mtm);
        let notch = notch_rect(&info).unwrap_or_else(|| {
            // No notch: a 24 pt strip under the top edge, as today's fallback.
            let top = info.frame.y + info.frame.h;
            Rect { x: info.frame.x + info.frame.w / 2. - 100., y: top - 24., w: 200., h: 24. }
        });
        eprintln!("screen {:?}\nnotch {:?}", info, notch);

        // GPUI bounds are display-local with y down; pick the display of the same size.
        let display = cx.displays().into_iter().find(|d| {
            let b = d.bounds();
            (f32::from(b.size.width) as f64 - info.frame.w).abs() < 1.
                && (f32::from(b.size.height) as f64 - info.frame.h).abs() < 1.
        });
        let x = notch.x - info.frame.x + notch.w / 2. - CANVAS.0 / 2.;
        let bounds = Bounds {
            origin: point(px(x as f32), px(0.)),
            size: size(px(CANVAS.0 as f32), px(CANVAS.1 as f32)),
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            display_id: display.map(|d| d.id()),
            titlebar: None,
            window_background: WindowBackgroundAppearance::Transparent,
            focus: false,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            let ns = ns_window(window);
            match &ns {
                Some(w) => {
                    w.setLevel(NSStatusWindowLevel);
                    w.setCollectionBehavior(
                        NSWindowCollectionBehavior::CanJoinAllSpaces
                            | NSWindowCollectionBehavior::Stationary
                            | NSWindowCollectionBehavior::FullScreenAuxiliary
                            | NSWindowCollectionBehavior::IgnoresCycle,
                    );
                    w.setHasShadow(false);
                    w.setIgnoresMouseEvents(true);
                    eprintln!("ns window found through raw-window-handle");
                }
                None => eprintln!("no ns window from raw-window-handle"),
            }
            cx.new(|cx| {
                cx.spawn(async move |this, cx| loop {
                    cx.background_executor().timer(Duration::from_millis(50)).await;
                    if this.update(cx, |n: &mut Notch, cx| n.tick(cx)).is_err() {
                        break;
                    }
                })
                .detach();
                Notch { ns_window: ns, notch, hovered: false, clicks: 0 }
            })
        })
        .expect("open window");
    });
}
