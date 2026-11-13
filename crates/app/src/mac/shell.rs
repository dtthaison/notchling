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
use crate::pages::window::AppPagesDir;
use crate::phone::bridge::Bridge;
use crate::phone::pairing::PairingFeed;
use crate::search::hotkey::{self, Registration};
use crate::search::library::Library;
use crate::search::panel::AppSearch;
use crate::search::wire::SessionRef;
use crate::session_source::SessionSource;
use crate::switches::restart::{LiveTurnOn, RestartNotice};
use crate::tools::second_opinion::store_for_run;
use crate::welcome::window::{self as welcome_window, WelcomeSetup, Which};
use notchling_engine::delegate::second_opinion_store::SecondOpinionStore;

struct Shell {
    state: MenuState,
    tray: Option<Tray>,
    titled: HashSet<WindowId>,
    chat: Option<WindowHandle<ChatView>>,
    root: Option<DataRoot>,
    sessions: Arc<SessionSource>,
    _hot_key: Option<Box<dyn Registration>>,
}

impl Global for Shell {}

pub struct AppActions(pub Arc<Actions>);

impl Global for AppActions {}

pub struct AppBridge(pub Arc<Bridge>);

impl Global for AppBridge {}

pub struct AppPairing(pub Arc<PairingFeed>);

impl Global for AppPairing {}

pub struct AppSwitchFeed(pub Arc<crate::switches::feed::SwitchFeed>);

impl Global for AppSwitchFeed {}

pub struct AppInbox(pub Arc<crate::inbox::feed::InboxFeed>);

impl Global for AppInbox {}

pub struct AppAccounts(pub Arc<crate::accounts::store::Accounts>);

impl Global for AppAccounts {}

pub struct AppVault(pub Arc<crate::vault::store::Vault>);

impl Global for AppVault {}

pub struct AppOpinions(pub Option<SecondOpinionStore>);

impl Global for AppOpinions {}

const OPINION_STOP_GRACE: Duration = Duration::from_secs(2);

const BRIDGE_POLL: Duration = Duration::from_secs(5);

pub fn run(data_root: Option<PathBuf>, open_chat_now: bool) -> i32 {
    let data_root_is_real =
        data_root.is_none() && std::env::var_os("NOTCHLING_DATA_ROOT").is_none();
    let root = DataRoot::for_run(data_root.as_deref());
    let notch = root.as_ref().map(DataRoot::notch_source);
    let app = Application::new().with_assets(notchling_kit::ui::KitAssets);
    app.on_reopen(|cx| {
        open_chat(cx);
        cx.activate(true);
    });
    app.run(move |cx: &mut App| {
        crate::mac::app_icons::install();
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
        let exe = std::env::current_exe().ok();
        let sessions = Arc::new(SessionSource::for_run(root.as_ref(), exe.as_deref()));
        let for_notch = sessions.clone();
        let restart = Arc::new(RestartNotice::default());
        let actions = Arc::new(Actions::for_run(root.as_ref()).with_restart(restart.clone()));
        cx.set_global(AppActions(actions.clone()));
        let bridge = Arc::new(Bridge::for_run(root.as_ref()).with_restart(restart.clone()));
        cx.set_global(AppBridge(bridge.clone()));
        let pairing = Arc::new(PairingFeed::for_run(root.as_ref()));
        cx.set_global(AppPairing(pairing.clone()));
        cx.set_global(WelcomeSetup { root: root.clone() });
        cx.set_global(AppSearch(Arc::new(Library::for_run(root.as_ref()))));
        let feed = Arc::new(crate::switches::feed::SwitchFeed::for_run(
            root.as_ref(),
            restart.clone(),
        ));
        cx.set_global(AppSwitchFeed(feed.clone()));
        cx.set_global(AppInbox(Arc::new(crate::inbox::feed::InboxFeed::for_run(
            root.as_ref(),
        ))));
        if feed.is_live() {
            super::restart_notice::watch(
                cx,
                restart,
                Arc::new(LiveTurnOn {
                    actions: actions.clone(),
                    bridge: bridge.clone(),
                    feed: feed.clone(),
                }),
            );
        }
        let accounts = Arc::new(crate::accounts::store::Accounts::for_run(root.as_ref()));
        cx.set_global(AppAccounts(accounts.clone()));
        if accounts.is_live() {
            if let Some(root) = root.as_ref() {
                send_limit_settings(accounts, root.data_dir.join("settings.json"));
            }
        }
        let vault = Arc::new(crate::vault::store::Vault::for_run(root.as_ref()));
        cx.set_global(AppVault(vault.clone()));
        if feed.is_live() {
            super::vault_prompts::watch(cx, feed, vault);
        }
        if let Some(root) = root.as_ref() {
            cx.set_global(AppPagesDir(root.data_dir.join("pages")));
        }
        let opinions = store_for_run(root.as_ref(), exe.as_deref());
        if let Some(store) = opinions.clone() {
            cx.on_app_quit(move |_| {
                store.stop_all_within(OPINION_STOP_GRACE);
                async {}
            })
            .detach();
        }
        cx.set_global(AppOpinions(opinions));
        if pairing.is_live() {
            super::pair_prompt::watch(cx, pairing, bridge.clone());
        }
        if bridge.is_live() {
            let polled = actions.clone();
            cx.spawn(async move |cx| loop {
                let (asked, acting) = (bridge.clone(), polled.clone());
                cx.background_executor()
                    .spawn(async move {
                        asked.refresh();
                        acting.refresh();
                    })
                    .await;
                cx.background_executor().timer(BRIDGE_POLL).await;
            })
            .detach();
        }
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
        if let Some(source) = notch {
            crate::notch::panel::open(cx, source, for_notch, actions);
        }
        welcome_window::open_first_run_once(cx);
        if open_chat_now || data_root_is_real {
            open_chat(cx);
            cx.activate(true);
        }
    });
    0
}

fn send_limit_settings(accounts: Arc<crate::accounts::store::Accounts>, file: PathBuf) {
    std::thread::spawn(move || {
        let Ok(settings) = notchling_engine::settings::Settings::open(&file) else {
            return;
        };
        let values = settings.values().clone();
        let keys = crate::accounts::wire::Call::LIMIT_KEYS;
        for _ in 0..30 {
            let sent = keys.iter().all(|key| match values.get(*key) {
                Some(value) => accounts.send_limit_setting(key, value).is_ok(),
                None => true,
            });
            if sent {
                return;
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });
}

fn handle(command: Command, cx: &mut App) {
    match command {
        Command::OpenChat => open_chat(cx),
        Command::Settings => open_chat_settings(cx),
        Command::Accounts => {
            crate::accounts::window::open(cx, None);
        }
        Command::About => welcome_window::open(cx, Which::About),
        Command::Welcome => welcome_window::open(cx, Which::FirstRun),
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

fn chat_window(cx: &App) -> Option<WindowHandle<ChatView>> {
    let handle = cx.global::<Shell>().chat?;
    let open = cx
        .windows()
        .iter()
        .any(|w| w.window_id() == handle.window_id());
    open.then_some(handle)
}

fn open_chat(cx: &mut App) {
    dock::apply(Policy::Regular);
    cx.activate(true);
    if let Some(handle) = chat_window(cx) {
        handle
            .update(cx, |_, window, _| window.activate_window())
            .ok();
    } else {
        let root = cx.global::<Shell>().root.clone();
        let sessions = cx.global::<Shell>().sessions.clone();
        let id = match root {
            Some(root) => {
                let settings = root
                    .fixture
                    .is_none()
                    .then(|| root.data_dir.join("settings.json"));
                let handle = open_main_window(cx, settings, |window, cx| {
                    ChatView::with_source(root, sessions, window, cx)
                });
                cx.global_mut::<Shell>().chat = handle;
                handle.map(|h| h.window_id())
            }
            None => open_main_window(cx, None, |window, cx| ShellView::new(None, window, cx))
                .map(|h| h.window_id()),
        };
        if let Some(id) = id {
            cx.global_mut::<Shell>().titled.insert(id);
        }
    }
    update_dock(cx);
}

pub fn open_chat_session(cx: &mut App, target: SessionRef) {
    open_chat(cx);
    if let Some(handle) = chat_window(cx) {
        handle
            .update(cx, |view, _, cx| view.show_session(target, cx))
            .ok();
    }
}

pub fn open_chat_inbox(cx: &mut App) {
    open_chat(cx);
    if let Some(handle) = chat_window(cx) {
        handle.update(cx, |view, _, cx| view.show_inbox(cx)).ok();
    }
}

pub fn open_chat_settings(cx: &mut App) {
    open_chat(cx);
    if let Some(handle) = chat_window(cx) {
        handle.update(cx, |view, _, cx| view.show_settings(cx)).ok();
    }
}

pub fn open_chat_secret(cx: &mut App, name: &str) {
    open_chat(cx);
    if let Some(handle) = chat_window(cx) {
        let name = name.to_owned();
        handle
            .update(cx, |view, window, cx| view.show_secret(&name, window, cx))
            .ok();
    }
}

pub fn note_titled(cx: &mut App, id: WindowId) {
    cx.global_mut::<Shell>().titled.insert(id);
    update_dock(cx);
}

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
