//! The live notch: a borderless, non-activating panel over the camera housing (`NotchController`,
//! rebuilt from the GPUI spike's S1). The panel is a transparent canvas as wide as the screen,
//! changing size only with the display; only the black shape inside it changes. Every 50 ms the
//! mouse is tested against the shape: outside, the panel ignores the mouse so clicks fall through
//! to whatever is underneath; inside, it takes them. Every 2 s the sessions are read again, off the
//! main thread.

use std::time::Duration;

use gpui::{
    point, prelude::*, px, size, App, AsyncApp, Bounds, WeakEntity, WindowBackgroundAppearance,
    WindowBounds, WindowHandle, WindowKind, WindowOptions,
};
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSEvent, NSScreen, NSStatusWindowLevel, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use super::layout::{
    canvas_frame, geometry, hit_test, shape_rect, Geometry, NotchState, Rect, ScreenInfo, CANVAS,
};
use super::model::NotchModel;
use super::source::{now_ms, Source};
use super::view::NotchView;
use crate::mac::capture::ns_window;

const TICK: Duration = Duration::from_millis(50);
const POLL: Duration = Duration::from_secs(2);

/// The display with a camera housing, else the main one, as AppKit sees it.
fn screen_info(mtm: MainThreadMarker) -> Option<ScreenInfo> {
    let screens = NSScreen::screens(mtm);
    let screen = screens
        .iter()
        .find(|s| s.safeAreaInsets().top > 0.0)
        .or_else(|| NSScreen::mainScreen(mtm))?;
    let f = screen.frame();
    let width = |w: f64| (w > 0.0).then_some(w);
    let visible = screen.visibleFrame();
    Some(ScreenInfo {
        frame: Rect::new(f.origin.x, f.origin.y, f.size.width, f.size.height),
        visible_max_y: visible.origin.y + visible.size.height,
        safe_top: screen.safeAreaInsets().top,
        aux_left_width: width(screen.auxiliaryTopLeftArea().size.width),
        aux_right_width: width(screen.auxiliaryTopRightArea().size.width),
    })
}

/// Opens the notch over the camera, reading the sessions `source` reads: the same root Chat reads
/// (`--data-root`, `NOTCHLING_DATA_ROOT`, else the user's own). `None` when there's no screen or
/// GPUI can't open the window.
pub fn open(cx: &mut App, source: Source) -> Option<WindowHandle<NotchView>> {
    let mtm = MainThreadMarker::new()?;
    let geometry = geometry(&screen_info(mtm)?);
    let options = WindowOptions {
        // Placed for real on the first tick: AppKit keeps a new window below the menu bar, and
        // GPUI sets the frame again after opening.
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(px(CANVAS.w as f32), px(CANVAS.h as f32)),
        })),
        titlebar: None,
        window_background: WindowBackgroundAppearance::Transparent,
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        ..Default::default()
    };
    cx.open_window(options, |window, cx| {
        let ns = ns_window(window);
        if let Some(w) = &ns {
            configure(w);
        }
        cx.new(|cx| {
            let mut model = NotchModel::default();
            model.notch = geometry.notch;
            model.now_ms = now_ms();
            let mut view = NotchView::new(model, false);
            view.ns_window = ns;
            view.geometry = Some(geometry);
            view.source = Some(source);
            cx.spawn(async move |this, cx| loop {
                if this.update(cx, tick).is_err() {
                    break;
                }
                cx.background_executor().timer(TICK).await;
            })
            .detach();
            cx.spawn(async move |this, cx| poll_forever(this, cx).await)
                .detach();
            view
        })
    })
    .map_err(|e| eprintln!("open the notch: {e}"))
    .ok()
}

/// What makes the panel the notch: borderless and non-activating (GPUI makes every window Titled,
/// which AppKit keeps below the menu bar), over the menu bar, on every Space and beside full-screen
/// apps, out of the window cycle, no shadow, and passing clicks through until the mouse is on the
/// shape.
fn configure(w: &NSWindow) {
    w.setStyleMask(NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel);
    w.setLevel(NSStatusWindowLevel);
    w.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    w.setHasShadow(false);
    w.setMovable(false);
    w.setHidesOnDeactivate(false);
    w.setIgnoresMouseEvents(true);
}

/// Keeps the canvas over the camera, tests the mouse against the shape and runs the model's timers.
fn tick(view: &mut NotchView, cx: &mut gpui::Context<NotchView>) {
    let (Some(w), Some(g)) = (view.ns_window.clone(), view.geometry) else {
        return;
    };
    place(&w, &g);
    let now = now_ms();
    let state = view.model.state.clone();
    let shape = shape_rect(&g, view.model.size(&state), state == NotchState::Collapsed);
    let mouse = NSEvent::mouseLocation();
    let inside = w.isVisible() && hit_test(shape, (mouse.x, mouse.y));
    if w.ignoresMouseEvents() == inside {
        w.setIgnoresMouseEvents(!inside);
    }
    let was = view.model.hovering();
    view.model.hover(inside, now);
    let moved = view.model.tick(now);
    if moved || was != inside {
        cx.notify();
    }
}

/// Sets the canvas frame when it differs (the display changed, or GPUI moved it after opening).
fn place(w: &NSWindow, g: &Geometry) {
    let c = canvas_frame(g);
    let want = NSRect::new(NSPoint::new(c.x, c.y), NSSize::new(c.w, c.h));
    if w.frame() != want {
        w.setFrame_display(want, true);
    }
}

/// Reads the sessions every [`POLL`], off the main thread, and the display's geometry with them.
async fn poll_forever(this: WeakEntity<NotchView>, cx: &mut AsyncApp) {
    loop {
        let taken = this.update(cx, |view, _| view.source.take());
        let Ok(source) = taken else {
            break;
        };
        if let Some(mut source) = source {
            let (source, loaded) = cx
                .background_executor()
                .spawn(async move {
                    let loaded = source.load(now_ms());
                    (source, loaded)
                })
                .await;
            let applied = this.update(cx, |view, cx| {
                view.source = Some(source);
                let now = now_ms();
                view.model.now_ms = now;
                if let Some(info) = MainThreadMarker::new().and_then(screen_info) {
                    let g = geometry(&info);
                    view.model.notch = g.notch;
                    view.geometry = Some(g);
                }
                let peek = view
                    .model
                    .update(loaded.agents, loaded.snapshots, loaded.requests);
                if let Some(id) = peek {
                    view.model.peek(&id, now);
                }
                cx.notify();
            });
            if applied.is_err() {
                break;
            }
        }
        cx.background_executor().timer(POLL).await;
    }
}
