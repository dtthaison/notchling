//! The search panel's window (macOS; the Swift `SearchPanel` and `SearchPanelController`).
//! Borderless and non-activating, like Spotlight: it floats near the top of the screen the mouse
//! is on, over every Space and beside full-screen apps, and goes away when you click elsewhere.
//! A non-activating panel only gets the keyboard while its app is active, so showing it activates
//! Notchling, and closing it with Esc gives the app you were in its focus back; opening a page or
//! a session doesn't, since the Artifacts or Chat window takes over.

use std::sync::Arc;

use gpui::{
    point, prelude::*, px, size, App, Bounds, Global, Window, WindowBackgroundAppearance,
    WindowBounds, WindowHandle, WindowKind, WindowOptions,
};
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplicationActivationOptions, NSEvent, NSModalPanelWindowLevel, NSRunningApplication,
    NSScreen, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use super::library::Library;
use super::model::{layout::WIDTH, SearchModel};
use super::view::SearchPanelView;
use super::wire::Results;
use crate::mac::capture::ns_window;

/// The library the panel and the Artifacts window read, shared. Not set in a render.
pub struct AppSearch(pub Arc<Library>);

impl Global for AppSearch {}

/// The panel while it is open, and the app to give focus back to when it closes.
#[derive(Default)]
pub struct SearchWindows {
    panel: Option<WindowHandle<SearchPanelView>>,
    previous_pid: Option<i32>,
}

impl Global for SearchWindows {}

/// The app's library, or one with nothing behind it (a render, a fixture root).
pub fn app_library(cx: &App) -> Arc<Library> {
    cx.try_global::<AppSearch>()
        .map(|l| l.0.clone())
        .unwrap_or_else(|| Arc::new(Library::fixed(Results::default())))
}

/// ⌘⇧P: opens the panel, or closes it when it is open.
pub fn toggle(cx: &mut App) {
    let open = cx.default_global::<SearchWindows>().panel.is_some();
    if open {
        hide(cx, true);
    } else {
        show(cx);
    }
}

/// Opens the panel near the top of the screen the mouse is on and activates the app, remembering
/// the app that was in front.
pub fn show(cx: &mut App) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let library = app_library(cx);
    let previous = previous_app();
    let top = top_point(mtm);
    let height = SearchModel::new().height();
    let options = WindowOptions {
        // Placed for real once open: AppKit keeps a new titled window below the menu bar, and
        // GPUI sets the frame again after opening.
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(px(WIDTH), px(height)),
        })),
        titlebar: None,
        focus: true,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Opaque,
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        let ns = ns_window(window);
        if let Some(w) = &ns {
            configure(w);
            place(w, top, height);
        }
        cx.new(|cx| SearchPanelView::live(library, ns, top, window, cx))
    });
    match opened {
        Ok(handle) => {
            let windows = cx.default_global::<SearchWindows>();
            windows.panel = Some(handle);
            windows.previous_pid = previous;
            cx.activate(true);
        }
        Err(e) => eprintln!("open the search panel: {e}"),
    }
}

/// Closes the panel from outside its window. `returning` gives the app you were in its focus back.
pub fn hide(cx: &mut App, returning: bool) {
    let (handle, previous) = forget(cx);
    if let Some(handle) = handle {
        handle
            .update(cx, |_, window, _| window.remove_window())
            .ok();
    }
    if returning {
        give_back(previous);
    }
}

/// Closes the panel from inside its own window (a key, a click on a row, a click elsewhere).
pub fn close(cx: &mut App, window: &mut Window, returning: bool) {
    let (_, previous) = forget(cx);
    window.remove_window();
    if returning {
        give_back(previous);
    }
}

/// Lets go of the panel's handle and the remembered app.
fn forget(cx: &mut App) -> (Option<WindowHandle<SearchPanelView>>, Option<i32>) {
    let windows = cx.default_global::<SearchWindows>();
    (windows.panel.take(), windows.previous_pid.take())
}

/// The app that was in front, unless it was this one.
fn previous_app() -> Option<i32> {
    let front = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let pid = front.processIdentifier();
    (pid != std::process::id() as i32).then_some(pid)
}

/// Brings the remembered app back to the front.
fn give_back(pid: Option<i32>) {
    if let Some(app) = pid.and_then(NSRunningApplication::runningApplicationWithProcessIdentifier)
    {
        app.activateWithOptions(NSApplicationActivationOptions::empty());
    }
}

/// Like Spotlight: centred, about a quarter of the way down the screen the mouse is on (the main
/// screen without one). AppKit coordinates: the point the panel hangs from.
fn top_point(mtm: MainThreadMarker) -> (f64, f64) {
    let mouse = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    let frame = screens
        .iter()
        .map(|s| s.frame())
        .find(|f| {
            mouse.x >= f.origin.x
                && mouse.x < f.origin.x + f.size.width
                && mouse.y >= f.origin.y
                && mouse.y < f.origin.y + f.size.height
        })
        .or_else(|| NSScreen::mainScreen(mtm).map(|s| s.frame()))
        .unwrap_or_else(|| NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1440.0, 900.0)));
    (
        frame.origin.x + frame.size.width / 2.0,
        frame.origin.y + frame.size.height - frame.size.height * 0.22,
    )
}

/// What makes the window the panel: borderless and non-activating (GPUI makes every window
/// Titled), at the modal panel level, on every Space and beside full-screen apps, transient, with
/// a shadow, and staying put when the app deactivates (closing is the panel's own decision).
fn configure(w: &NSWindow) {
    w.setStyleMask(NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel);
    w.setLevel(NSModalPanelWindowLevel);
    w.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Transient,
    );
    w.setHasShadow(true);
    w.setMovable(false);
    w.setHidesOnDeactivate(false);
}

/// Hangs the panel from `top` at `height`, when its frame differs.
pub(super) fn place(w: &NSWindow, top: (f64, f64), height: f32) {
    let height = f64::from(height);
    let want = NSRect::new(
        NSPoint::new(top.0 - f64::from(WIDTH) / 2.0, top.1 - height),
        NSSize::new(f64::from(WIDTH), height),
    );
    if w.frame() != want {
        w.setFrame_display(want, true);
        w.invalidateShadow();
    }
}
