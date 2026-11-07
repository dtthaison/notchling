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
    /// `--auto` drives the hit test with this instead of the real mouse.
    fake_mouse: Option<(f64, f64)>,
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

    /// AppKit keeps new windows below the menu bar, and GPUI sets the frame again after
    /// opening, so the frame goes over the notch here, on the first tick, as
    /// `NotchController` sets its own.
    fn place(&mut self) {
        let Some(w) = &self.ns_window else { return };
        let top = self.notch.y + self.notch.h;
        let x = self.notch.x + self.notch.w / 2. - CANVAS.0 / 2.;
        let want = objc2_foundation::NSRect::new(
            objc2_foundation::NSPoint::new(x, top - CANVAS.1),
            objc2_foundation::NSSize::new(CANVAS.0, CANVAS.1),
        );
        if w.frame() != want {
            w.setFrame_display(want, true);
        }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        self.place();
        let m = NSEvent::mouseLocation();
        if self.fake_mouse.is_none() {
            self.tick_at((m.x, m.y), cx);
        } else {
            self.tick_at(self.fake_mouse.unwrap(), cx);
        }
    }

    fn tick_at(&mut self, m: (f64, f64), cx: &mut Context<Self>) {
        let m = objc2_foundation::NSPoint::new(m.0, m.1);
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
                    // GPUI makes every window Titled, even with no titlebar. A titled window is
                    // kept below the menu bar and can become key; the notch panel is borderless
                    // and non-activating, like NotchController's.
                    w.setStyleMask(
                        objc2_app_kit::NSWindowStyleMask::Borderless
                            | objc2_app_kit::NSWindowStyleMask::NonactivatingPanel,
                    );
                    w.setIgnoresMouseEvents(true);
                    eprintln!("ns window found through raw-window-handle");
                }
                None => eprintln!("no ns window from raw-window-handle"),
            }
            cx.new(|cx| {
                if std::env::args().any(|a| a == "--auto") {
                    cx.spawn(async move |this, cx| auto::run(this, cx).await).detach();
                }
                cx.spawn(async move |this, cx| loop {
                    cx.background_executor().timer(Duration::from_millis(50)).await;
                    if this.update(cx, |n: &mut Notch, cx| n.tick(cx)).is_err() {
                        break;
                    }
                })
                .detach();
                Notch { ns_window: ns, notch, hovered: false, clicks: 0, fake_mouse: None }
            })
        })
        .expect("open window");
    });
}

/// `notch --auto`: checks S1 without a person. It reads back the panel's level,
/// Spaces behaviour and key state, drives the hit test with a fake mouse inside
/// and outside the shape, sends a real mouse-down/up NSEvent to the window (in
/// process, so no Accessibility permission), captures the panel in both states,
/// and checks the front app never changed.
mod auto {
    use super::*;
    use objc2_app_kit::{NSEventType, NSWorkspace};
    use objc2_core_graphics::{CGRectNull, CGWindowImageOption, CGWindowListCreateImage, CGWindowListOption};
    use objc2_foundation::NSDictionary;

    fn front() -> String {
        NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .and_then(|a| a.localizedName())
            .map(|n| n.to_string())
            .unwrap_or_default()
    }

    #[allow(deprecated)]
    fn capture(window: &NSWindow, name: &str) -> String {
        let image = CGWindowListCreateImage(
            unsafe { CGRectNull },
            CGWindowListOption::OptionIncludingWindow,
            window.windowNumber() as u32,
            CGWindowImageOption::BoundsIgnoreFraming | CGWindowImageOption::BestResolution,
        );
        let Some(image) = image else { return "capture failed".into() };
        let rep = objc2_app_kit::NSBitmapImageRep::initWithCGImage(
            <objc2_app_kit::NSBitmapImageRep as objc2::AllocAnyThread>::alloc(),
            &image,
        );
        let data = unsafe {
            rep.representationUsingType_properties(objc2_app_kit::NSBitmapImageFileType::PNG, &NSDictionary::new())
        };
        let path = format!("{}/Desktop/notchling-spike/{name}", std::env::var("HOME").unwrap_or_default());
        match data.map(|d| std::fs::write(&path, d.to_vec())) {
            Some(Ok(())) => path,
            _ => "write failed".into(),
        }
    }

    fn click(window: &NSWindow, at_screen: (f64, f64)) {
        let p = window.convertPointFromScreen(objc2_foundation::NSPoint::new(at_screen.0, at_screen.1));
        for kind in [NSEventType::LeftMouseDown, NSEventType::LeftMouseUp] {
            let e = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                kind,
                p,
                objc2_app_kit::NSEventModifierFlags::empty(),
                0.,
                window.windowNumber(),
                None,
                0,
                1,
                1.,
            );
            if let Some(e) = e {
                window.sendEvent(&e);
            }
        }
    }

    pub async fn run(this: gpui::WeakEntity<Notch>, cx: &mut gpui::AsyncApp) {
        let exec = cx.background_executor().clone();
        let wait = move |ms| exec.timer(Duration::from_millis(ms));
        wait(800).await;
        let before = front();
        let Ok((w, notch)) = this.update(cx, |n, _| (n.ns_window.clone(), n.notch)) else { return };
        let Some(w) = w else {
            println!("S1 auto: no NSWindow");
            return;
        };
        let f = w.frame();
        let behavior = w.collectionBehavior();
        let want = NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle;
        println!("S1 notch {notch:?}");
        println!(
            "S1 panel frame x={} y={} w={} h={}; centred on notch: {}; top at screen top: {}",
            f.origin.x, f.origin.y, f.size.width, f.size.height,
            (f.origin.x + f.size.width / 2. - (notch.x + notch.w / 2.)).abs() <= 1.0,
            (f.origin.y + f.size.height - (notch.y + notch.h)).abs() < 0.5,
        );
        println!("S1 level {} (status = 25); all Spaces + full screen behaviour: {}", w.level(), behavior.contains(want));
        println!("S1 key window: {} (want false); can become key: {}", w.isKeyWindow(), w.canBecomeKeyWindow());

        let outside = (notch.x - 30., notch.y + notch.h / 2.);
        let inside = (notch.x + notch.w / 2., notch.y + notch.h / 2.);
        this.update(cx, |n, cx| { n.fake_mouse = Some(outside); n.tick(cx) }).ok();
        wait(150).await;
        println!("S1 mouse beside the notch: ignores mouse events = {} (want true: clicks fall through)", w.ignoresMouseEvents());
        println!("S1 collapsed capture: {}", capture(&w, "s1-auto-collapsed.png"));
        this.update(cx, |n, cx| { n.fake_mouse = Some(inside); n.tick(cx) }).ok();
        wait(300).await;
        let hovered = this.update(cx, |n, _| n.hovered).unwrap_or(false);
        println!("S1 mouse on the notch: hovered = {hovered}, ignores mouse events = {} (want false)", w.ignoresMouseEvents());
        println!("S1 open capture: {}", capture(&w, "s1-auto-open.png"));
        click(&w, (inside.0, notch.y + notch.h - 60.));
        wait(200).await;
        let clicks = this.update(cx, |n, _| n.clicks).unwrap_or(0);
        println!("S1 click inside the open shape: received {clicks} (want 1)");
        let after = front();
        println!("S1 front app before {before:?}, after {after:?}: unchanged = {}", before == after);
        let _ = cx.update(|cx| cx.quit());
    }
}
