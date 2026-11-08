//! The windows for first run, About and Handover (part U11, macOS): one of each, opened from the
//! Notchling menu and brought forward when already open; the first-run window also opens by itself,
//! once. Each is a titled window, so it counts toward the Dock icon (`mac::shell::note_titled`).

use std::sync::Arc;

use gpui::{
    prelude::*, px, size, App, Bounds, Context, Global, Render, TitlebarOptions, Window,
    WindowBounds, WindowHandle, WindowKind, WindowOptions,
};
use notchling_engine::control::RealLauncher;

use super::defaults::DefaultsTool;
use super::facts::{SwiftProbe, SystemProbe};
use super::first_run::FirstRun;
use super::handover_view::{HandoverView, Services};
use super::hooks::Hooks;
use super::own::{Flags, FIRST_RUN_KEY};
use super::updates::{CurlFetcher, NoFeed, KILL_SWITCH};
use super::view::{live_about, AboutEnv, AboutView, FirstRunEnv, FirstRunView};
use crate::chat::data::DataRoot;
use crate::mac::shell::{note_titled, AppActions, AppBridge, AppSwitchFeed};

/// What the windows need to know about the running app. Not set in a render.
pub struct WelcomeSetup {
    /// The folders the app runs over; a fixture root never starts a process or asks a daemon.
    pub root: Option<DataRoot>,
}

impl Global for WelcomeSetup {}

/// Which window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    FirstRun,
    About,
    Handover,
}

#[derive(Default)]
struct Windows {
    first_run: Option<WindowHandle<FirstRunView>>,
    about: Option<WindowHandle<AboutView>>,
    handover: Option<WindowHandle<HandoverView>>,
}

impl Global for Windows {}

fn root(cx: &App) -> Option<DataRoot> {
    cx.try_global::<WelcomeSetup>().and_then(|s| s.root.clone())
}

fn is_open<V: 'static>(cx: &App, handle: Option<WindowHandle<V>>) -> bool {
    handle.is_some_and(|h| cx.windows().iter().any(|w| w.window_id() == h.window_id()))
}

/// Opens a titled window of `size` over `build`, and counts it toward the Dock icon.
fn open_titled<V: Render>(
    cx: &mut App,
    title: &str,
    dimensions: (f32, f32),
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) -> Option<WindowHandle<V>> {
    let bounds = Bounds::centered(None, size(px(dimensions.0), px(dimensions.1)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(title.to_owned().into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        window_min_size: Some(size(px(dimensions.0.min(420.0)), px(360.0))),
        kind: WindowKind::Normal,
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| cx.new(|cx| build(window, cx))) {
        Ok(handle) => {
            note_titled(cx, handle.window_id());
            cx.activate(true);
            Some(handle)
        }
        Err(e) => {
            eprintln!("open the {title} window: {e}");
            None
        }
    }
}

/// Opens (or brings forward) one of the windows.
pub fn open(cx: &mut App, which: Which) {
    match which {
        Which::FirstRun => open_first_run(cx),
        Which::About => open_about(cx),
        Which::Handover => open_handover(cx),
    }
}

/// Brings `handle` forward when its window is open.
fn raise<V: 'static>(cx: &mut App, handle: Option<WindowHandle<V>>) -> bool {
    if !is_open(cx, handle) {
        return false;
    }
    let raised = handle
        .map(|h| {
            h.update(cx, |_, window, _| window.activate_window())
                .is_ok()
        })
        .unwrap_or(false);
    if raised {
        cx.activate(true);
    }
    raised
}

fn open_about(cx: &mut App) {
    let existing = cx.default_global::<Windows>().about;
    if raise(cx, existing) {
        return;
    }
    let live = root(cx).filter(|r| r.fixture.is_none());
    let env = match live {
        Some(r) => AboutEnv {
            fetcher: Arc::new(CurlFetcher::new(RealLauncher::new(&r.home))),
            switch: std::env::var(KILL_SWITCH).ok(),
        },
        None => AboutEnv {
            fetcher: Arc::new(NoFeed),
            switch: None,
        },
    };
    let handle = open_titled(cx, "About Notchling", (460.0, 560.0), |_, _| {
        live_about(env)
    });
    cx.default_global::<Windows>().about = handle;
}

fn open_handover(cx: &mut App) {
    let existing = cx.default_global::<Windows>().handover;
    if raise(cx, existing) {
        return;
    }
    let (Some(actions), Some(bridge), Some(switches)) = (
        cx.try_global::<AppActions>().map(|g| g.0.clone()),
        cx.try_global::<AppBridge>().map(|g| g.0.clone()),
        cx.try_global::<AppSwitchFeed>().map(|g| g.0.clone()),
    ) else {
        eprintln!("the Handover window needs the running app's daemon connections");
        return;
    };
    let hooks = Arc::new(Hooks::for_run(root(cx).as_ref()));
    let probe: Arc<dyn SwiftProbe> = Arc::new(SystemProbe::default());
    let services = Arc::new(Services {
        actions,
        bridge,
        switches,
        hooks,
        probe,
    });
    let handle = open_titled(cx, "Handover", (820.0, 900.0), |_, cx| {
        HandoverView::live(services, cx)
    });
    cx.default_global::<Windows>().handover = handle;
}

fn open_first_run(cx: &mut App) {
    let existing = cx.default_global::<Windows>().first_run;
    if raise(cx, existing) {
        return;
    }
    let setup = root(cx);
    let live = setup.as_ref().filter(|r| r.fixture.is_none()).cloned();
    // The daemon and the process list are asked off the UI thread, then the window opens with what
    // they said.
    let asked = live.clone();
    cx.spawn(async move |cx| {
        let model = cx
            .background_executor()
            .spawn(async move {
                let hooks = Hooks::for_run(asked.as_ref());
                FirstRun::new(SystemProbe::default().find(), hooks.plan())
            })
            .await;
        cx.update(|cx| {
            let env = live.map(|r| FirstRunEnv {
                hooks: Arc::new(Hooks::for_run(Some(&r))),
                defaults: Arc::new(DefaultsTool::new(RealLauncher::new(&r.home))),
                settings: r.data_dir.join("settings.json"),
            });
            let handle = open_titled(cx, "Welcome to Notchling", (640.0, 760.0), |_, _| {
                FirstRunView::new(model, env)
            });
            cx.default_global::<Windows>().first_run = handle;
        })
        .ok();
    })
    .detach();
}

/// Opens the first-run window the first time the app runs over the real folders, and marks it
/// shown (a flag in the app's own settings), so it comes by itself once. A fixture root never
/// opens it.
pub fn open_first_run_once(cx: &mut App) {
    let Some(root) = root(cx).filter(|r| r.fixture.is_none()) else {
        return;
    };
    let flags = Flags {
        file: root.data_dir.join("settings.json"),
    };
    if flags.get(FIRST_RUN_KEY) {
        return;
    }
    // Marked before it opens: shown once, even if the app is quit with the window up. When the
    // file can't be written the window doesn't open every launch either.
    if let Err(e) = flags.set(FIRST_RUN_KEY, true) {
        eprintln!("first run: not shown, the flag can't be kept ({e})");
        return;
    }
    open_first_run(cx);
}
