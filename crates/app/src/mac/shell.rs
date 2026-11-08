//! The menu bar app: the tray item, the Dock presence and the main window (Chat), wired together,
//! with the ⌘⇧P search panel (part U3) and the Artifacts window (part U4) it opens.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{App, Application, Global, WindowHandle, WindowId};
use objc2::MainThreadMarker;

use super::dock;
use super::tray::Tray;
use super::window::{open_main_window, ShellView};
use crate::actions::Actions;
use crate::chat::data::DataRoot;
use crate::chat::view::ChatView;
use crate::dock::{policy_for, Policy};
use crate::menu::{Command, MenuState};
use crate::phone::bridge::{now_ms, Bridge};
use crate::phone::pairing::PairingFeed;
use crate::search::hotkey::{self, Registration};
use crate::search::library::Library;
use crate::search::panel::AppSearch;
use crate::search::wire::SessionRef;
use crate::session_source::SessionSource;
use crate::tools::second_opinion::store_for_run;
use notchling_engine::delegate::second_opinion_store::SecondOpinionStore;

/// The app's state while it runs.
struct Shell {
    state: MenuState,
    tray: Option<Tray>,
    /// Our titled windows: the ones that earn a Dock icon.
    titled: HashSet<WindowId>,
    /// The Chat window, while one was opened over a data root (it may have closed since).
    chat: Option<WindowHandle<ChatView>>,
    /// Where Chat reads from; `None` when the environment names no home folder.
    root: Option<DataRoot>,
    /// The session rows, from the daemon when it answers; shared by Chat and the notch.
    sessions: Arc<SessionSource>,
    /// ⌘⇧P, while it is ours; dropping it would give the key up.
    _hot_key: Option<Box<dyn Registration>>,
}

impl Global for Shell {}

/// The actions Chat and the notch send to the daemon, shared, so both know what it said about
/// acting. Not set in a render: there the views are given their own.
pub struct AppActions(pub Arc<Actions>);

impl Global for AppActions {}

/// The phone bridge as the daemon last described it, shared by Chat's Phone panel and the poll that
/// keeps it fresh and re-asserts the user's choice (`bridgeOn`). Not set in a render.
pub struct AppBridge(pub Arc<Bridge>);

impl Global for AppBridge {}

/// The follower of the daemon's `bridge` topic: the phones waiting to pair, which `mac::pair_prompt`
/// shows in a window of their own. Not set in a render.
pub struct AppPairing(pub Arc<PairingFeed>);

impl Global for AppPairing {}

/// The second opinions asked in Chat (the engine's store over the app's own data folder, running
/// this binary's `delegate`); `None` over a fixture root, which never starts a tool.
pub struct AppOpinions(pub Option<SecondOpinionStore>);

impl Global for AppOpinions {}

/// How long a review still running when the app quits gets to end after SIGTERM before it is
/// killed. Short: the app is waiting on it to leave.
const OPINION_STOP_GRACE: Duration = Duration::from_secs(2);

/// How often the app reads the bridge while no panel is open.
const BRIDGE_POLL: Duration = Duration::from_secs(5);

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
        let bridge = Arc::new(Bridge::for_run(root.as_ref()));
        cx.set_global(AppBridge(bridge.clone()));
        let pairing = Arc::new(PairingFeed::for_run(root.as_ref()));
        cx.set_global(AppPairing(pairing.clone()));
        cx.set_global(AppSearch(Arc::new(Library::for_run(root.as_ref()))));
        let opinions = store_for_run(root.as_ref(), exe.as_deref());
        if let Some(store) = opinions.clone() {
            // Every way out of the app comes through here: ⌘Q, the Dock's Quit, the tray's Quit and
            // a logout all end in `applicationWillTerminate`. GPUI runs this closure itself, not
            // under the 100 ms it gives the returned future, so the wait finishes before the
            // process can exit.
            cx.on_app_quit(move |_| {
                store.stop_all_within(OPINION_STOP_GRACE);
                async {}
            })
            .detach();
        }
        cx.set_global(AppOpinions(opinions));
        if pairing.is_live() {
            // The daemon asks the person only while this follows `bridge`.
            super::pair_prompt::watch(cx, pairing, bridge.clone());
        }
        if bridge.is_live() {
            // Off at every daemon start: this brings the daemon back to what the user chose.
            cx.spawn(async move |cx| loop {
                let asked = bridge.clone();
                cx.background_executor()
                    .spawn(async move { asked.refresh(now_ms()) })
                    .await;
                cx.background_executor().timer(BRIDGE_POLL).await;
            })
            .detach();
        }
        // ⌘⇧P from anywhere opens the search panel. Another app holding the key only costs the
        // shortcut; the panel still opens from a session's page or search in Chat.
        let hot_key = {
            let async_cx = cx.to_async();
            hotkey::for_system()
                .register(Box::new(move || {
                    async_cx.update(crate::search::panel::toggle).ok();
                }))
                .map_err(|e| eprintln!("no ⌘⇧P: {e}"))
                .ok()
        };
        cx.set_global(Shell {
            state,
            tray,
            titled: HashSet::new(),
            chat: None,
            root,
            sessions,
            _hot_key: hot_key,
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
        // Reviews still running are stopped by the quit hook set in `run`, whichever way we leave.
        Command::Quit => cx.quit(),
    }
}

fn refresh_tray(shell: &Shell) {
    if let Some(tray) = &shell.tray {
        tray.update(&shell.state);
    }
}

/// The Chat window, if it is still open.
fn chat_window(cx: &App) -> Option<WindowHandle<ChatView>> {
    let handle = cx.global::<Shell>().chat?;
    let open = cx
        .windows()
        .iter()
        .any(|w| w.window_id() == handle.window_id());
    open.then_some(handle)
}

/// Brings the Chat window forward, opening it first if it isn't open.
fn open_chat(cx: &mut App) {
    if let Some(handle) = chat_window(cx) {
        handle
            .update(cx, |_, window, _| window.activate_window())
            .ok();
    } else {
        let root = cx.global::<Shell>().root.clone();
        let sessions = cx.global::<Shell>().sessions.clone();
        let id = match root {
            Some(root) => {
                let handle = open_main_window(cx, |window, cx| {
                    ChatView::with_source(root, sessions, window, cx)
                });
                cx.global_mut::<Shell>().chat = handle;
                handle.map(|h| h.window_id())
            }
            None => open_main_window(cx, |window, cx| ShellView::new(None, window, cx))
                .map(|h| h.window_id()),
        };
        if let Some(id) = id {
            cx.global_mut::<Shell>().titled.insert(id);
        }
    }
    update_dock(cx);
}

/// Opens Chat on a conversation the search panel found (Return on a session).
pub fn open_chat_session(cx: &mut App, target: SessionRef) {
    open_chat(cx);
    if let Some(handle) = chat_window(cx) {
        handle
            .update(cx, |view, _, cx| view.show_session(target, cx))
            .ok();
    }
}

/// Counts another titled window of ours (the Artifacts window) toward the Dock icon.
pub fn note_titled(cx: &mut App, id: WindowId) {
    cx.global_mut::<Shell>().titled.insert(id);
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
