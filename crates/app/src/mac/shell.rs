//! The menu bar app: the tray item, the Dock presence and the main window (Chat), wired together.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{App, Application, Global, WindowId};
use objc2::MainThreadMarker;

use super::dock;
use super::tray::Tray;
use super::window::{open_main_window, ShellView};
use crate::actions::Actions;
use crate::chat::data::DataRoot;
use crate::chat::view::ChatView;
use crate::dock::{policy_for, Policy};
use crate::menu::{Command, MenuState};
use crate::session_source::SessionSource;

/// The app's state while it runs.
struct Shell {
    state: MenuState,
    tray: Option<Tray>,
    /// Our titled windows: the ones that earn a Dock icon.
    titled: HashSet<WindowId>,
    /// Where Chat reads from; `None` when the environment names no home folder.
    root: Option<DataRoot>,
    /// The session rows, from the daemon when it answers; shared by Chat and the notch.
    sessions: Arc<SessionSource>,
}

impl Global for Shell {}

/// The actions Chat and the notch send to the daemon, shared, so both know what it said about
/// acting. Not set in a render: there the views are given their own.
pub struct AppActions(pub Arc<Actions>);

impl Global for AppActions {}

/// Runs the app until Quit; the exit code. `data_root` (else `NOTCHLING_DATA_ROOT`) is a fixture
/// root instead of the real folders; Chat and the notch read the same one.
pub fn run(data_root: Option<PathBuf>) -> i32 {
    let root = DataRoot::for_run(data_root.as_deref());
    let notch = root.as_ref().map(DataRoot::notch_source);
    Application::new().run(move |cx: &mut App| {
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
        // A fixture root reads files only and never touches a daemon (`session_source::config_for`).
        let exe = std::env::current_exe().ok();
        let sessions = Arc::new(SessionSource::for_run(root.as_ref(), exe.as_deref()));
        let for_notch = sessions.clone();
        let actions = Arc::new(Actions::for_run(root.as_ref()));
        cx.set_global(AppActions(actions.clone()));
        cx.set_global(Shell {
            state,
            tray,
            titled: HashSet::new(),
            root,
            sessions,
        });
        cx.on_window_closed(update_dock).detach();
        // Not titled, so it never counts toward the Dock icon.
        if let Some(source) = notch {
            crate::notch::panel::open(cx, source, for_notch, actions);
        }
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

/// Brings the Chat window forward, opening it first if it isn't open.
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
            let root = cx.global::<Shell>().root.clone();
            let sessions = cx.global::<Shell>().sessions.clone();
            let id = match root {
                Some(root) => open_main_window(cx, |window, cx| {
                    ChatView::with_source(root, sessions, window, cx)
                })
                .map(|h| h.window_id()),
                None => open_main_window(cx, |window, cx| ShellView::new(None, window, cx))
                    .map(|h| h.window_id()),
            };
            if let Some(id) = id {
                cx.global_mut::<Shell>().titled.insert(id);
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
    if policy == Policy::Regular {
        cx.activate(true);
    }
}
