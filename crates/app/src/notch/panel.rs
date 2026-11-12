//! The live notch: a borderless, non-activating panel over the camera housing (`NotchController`,
//! rebuilt from the GPUI spike's S1). The panel is a transparent canvas centred on the camera, as
//! big as the largest shape the notch can take (`layout::canvas_size`; the Swift app's was as wide
//! as the screen, which under GPUI's Metal drawables cost most of the app's memory), changing size
//! only with the display; only the black shape inside it changes. Every 50 ms the mouse is tested
//! against the shape: outside, the panel ignores the mouse so clicks fall through to whatever is
//! underneath; inside, it takes them. Every 2 s the sessions are read again, off the main thread.

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    point, prelude::*, px, size, App, AsyncApp, Bounds, WeakEntity, WindowBackgroundAppearance,
    WindowBounds, WindowHandle, WindowKind, WindowOptions,
};
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSEvent, NSScreen, NSStatusWindowLevel, NSWindow, NSWindowCollectionBehavior,
    NSWindowOcclusionState, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use super::layout::{
    canvas_frame, geometry, hit_test, shape_rect, Geometry, NotchState, Rect, ScreenInfo,
};
use super::model::NotchModel;
use super::prefs::NotchPrefs;
use super::source::{now_ms, Source};
use super::view::NotchView;
use crate::actions::Actions;
use crate::mac::capture::ns_window;
use crate::session_source::{Link, SessionSource};
use crate::tools::live::ToolLive;
use crate::tools::requests::{agy_conversation, parse_conversation};

pub(super) const TICK: Duration = Duration::from_millis(50);
const POLL: Duration = Duration::from_secs(2);
/// How many polls between asking the daemon whether acting is on.
const ACTING_EVERY: u32 = 5;

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
pub fn open(
    cx: &mut App,
    source: Source,
    sessions: Arc<SessionSource>,
    actions: Arc<Actions>,
) -> Option<WindowHandle<NotchView>> {
    let mtm = MainThreadMarker::new()?;
    let geometry = geometry(&screen_info(mtm)?);
    let canvas = canvas_frame(&geometry);
    let options = WindowOptions {
        // Placed for real on the first tick: AppKit keeps a new window below the menu bar, and
        // GPUI sets the frame again after opening. The size is right from the start, so the
        // drawables are never made at another.
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(px(canvas.w as f32), px(canvas.h as f32)),
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
            let mut view = NotchView::new(model, false, actions, cx);
            view.ns_window = ns;
            view.geometry = Some(geometry);
            let mut source = source;
            // Before the first draw, so a notch turned off in Settings never flashes up.
            if let Some(prefs) = NotchPrefs::read(&source.root.data_dir) {
                view.apply_prefs(prefs);
            }
            // An open Antigravity session's lines come from the daemon (a read, peeking so the
            // poll never keeps an idle process alive); the notch can't read its files.
            let reader = view.actions.clone();
            source.agy_reader = Some(Arc::new(move |id: &str| {
                let reply = reader.read(&agy_conversation(id, true)).ok()?;
                parse_conversation(&reply)
                    .filter(|c| c.open)
                    .map(|c| c.conversation)
            }));
            view.source = Some(source);
            view.sessions = Some(sessions);
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
    // On screen: shown (the notch can be turned off) and not covered (the display asleep or
    // locked). Off screen the pet stays put and draws nothing: twenty frames a second of a window
    // nobody sees.
    let shown = w.isVisible();
    let on_screen = shown && w.occlusionState().contains(NSWindowOcclusionState::Visible);
    let inside = shown && hit_test(shape, (mouse.x, mouse.y));
    if w.ignoresMouseEvents() == inside {
        w.setIgnoresMouseEvents(!inside);
    }
    let was = view.model.hovering();
    view.model.hover(inside, now);
    let moved = view.model.tick(now);
    let pet = on_screen && view.step_pet(now as f64 / 1000.0);
    if moved || pet || was != inside {
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
    let mut polls = 0u32;
    let mut was_attached = false;
    loop {
        polls = polls.wrapping_add(1);
        // Whether acting is on, now and then and only while the daemon serves the rows; at once
        // after a (re)connect, so a daemon that started off gets the user's choice back.
        let asking = this.update(cx, |view, _| {
            let attached = view
                .sessions
                .as_ref()
                .is_some_and(|s| s.view().link == Link::Daemon);
            let fresh = attached && !was_attached;
            was_attached = attached;
            (attached && (fresh || polls % ACTING_EVERY == 1)).then(|| view.actions.clone())
        });
        match asking {
            Ok(Some(actions)) => {
                cx.background_executor()
                    .spawn(async move { actions.refresh() })
                    .await;
            }
            Ok(None) => {}
            Err(_) => break,
        }
        let taken = this.update(cx, |view, _| {
            let answer = view.sessions.as_ref().map(|s| s.view());
            (view.source.take(), answer)
        });
        let Ok((source, answer)) = taken else {
            break;
        };
        let (claude, prompts, live, note, answered) = match answer {
            Some(answer) => {
                let note = answer.note();
                (
                    answer.agents,
                    answer.prompts,
                    answer.tools,
                    note,
                    answer.answered,
                )
            }
            None => (None, None, ToolLive::default(), None, Default::default()),
        };
        if let Some(mut source) = source {
            let (source, loaded, prefs) = cx
                .background_executor()
                .spawn(async move {
                    let loaded = source.load_live(claude, prompts, &live, now_ms());
                    // What the Settings sheet saved since the last poll.
                    let prefs = NotchPrefs::read(&source.root.data_dir);
                    (source, loaded, prefs)
                })
                .await;
            let applied = this.update(cx, |view, cx| {
                view.source = Some(source);
                if let Some(prefs) = prefs {
                    view.apply_prefs(prefs);
                }
                view.model.daemon_note = note;
                // What Chat answered is answered here too.
                view.model.answered.extend(answered);
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
