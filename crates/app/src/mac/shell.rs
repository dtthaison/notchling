//! The menu bar app: the tray item, the Dock presence and the main window, wired together.

use std::collections::HashSet;

use gpui::{App, Application, Global, WindowId};
use objc2::MainThreadMarker;

use super::tray::Tray;
use super::{dock, window};
use crate::dock::policy_for;
use crate::menu::{Command, MenuState};

/// The app's state while it runs.
struct Shell {
    state: MenuState,
    tray: Option<Tray>,
    /// Our titled windows: the ones that earn a Dock icon.
    titled: HashSet<WindowId>,
}

impl Global for Shell {}

/// Runs the app until Quit; the exit code.
pub fn run() -> i32 {
    Application::new().run(|cx: &mut App| {
        dock::register(cx);
        dock::apply(policy_for(0));
        dock::set_main_menu(cx, policy_for(0));
        let state = MenuState::default();
        let tray = MainThreadMarker::new().map(|mtm| {
            let async_cx = cx.to_async();
            Tray::install(
                mtm,
                &state,
                Box::new(move |command| {
                    async_cx.update(|cx| handle(command, cx)).ok();
                }),
            )
        });
        if tray.is_none() {
            eprintln!("no menu bar item: not on the main thread");
        }
        cx.set_global(Shell {
            state,
            tray,
            titled: HashSet::new(),
        });
        cx.on_window_closed(update_dock).detach();
        // Not titled, so it never counts toward the Dock icon.
        crate::notch::panel::open(cx);
    });
    0
}

/// What a menu entry does. Accounts and Settings open Chat until their own parts land.
fn handle(command: Command, cx: &mut App) {
    match command {
        Command::OpenChat | Command::Accounts | Command::Settings => open_chat(cx),
        Command::SetAwake(mode) => {
            let shell = cx.global_mut::<Shell>();
            shell.state.awake = mode;
            refresh_tray(shell);
        }
        Command::ToggleDisplay => {
            let shell = cx.global_mut::<Shell>();
            shell.state.keep_display_on = !shell.state.keep_display_on;
            refresh_tray(shell);
        }
        Command::Quit => cx.quit(),
    }
}

fn refresh_tray(shell: &Shell) {
    if let Some(tray) = &shell.tray {
        tray.update(&shell.state);
    }
}

/// Brings the main window forward, opening it first if it isn't open.
fn open_chat(cx: &mut App) {
    let open: Vec<_> = cx.windows();
    let existing = open
        .iter()
        .find(|w| cx.global::<Shell>().titled.contains(&w.window_id()))
        .copied();
    match existing {
        Some(handle) => {
            handle
                .update(cx, |_, window, _| window.activate_window())
                .ok();
        }
        None => {
            if let Some(handle) = window::open_main_window(cx) {
                cx.global_mut::<Shell>().titled.insert(handle.window_id());
            }
        }
    }
    update_dock(cx);
}

/// A Dock icon, ⌘Tab and the main menu while a titled window is open; menu bar only otherwise.
fn update_dock(cx: &mut App) {
    let open: HashSet<WindowId> = cx.windows().iter().map(|w| w.window_id()).collect();
    let shell = cx.global_mut::<Shell>();
    shell.titled.retain(|id| open.contains(id));
    let policy = policy_for(shell.titled.len());
    dock::apply(policy);
    dock::set_main_menu(cx, policy);
    if policy == crate::dock::Policy::Regular {
        cx.activate(true);
    }
}
