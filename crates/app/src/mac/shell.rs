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
use crate::search::shortcut::{self, Setting};
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
    hot_key: Option<Box<dyn Registration>>,
    search: Setting,
    search_failure: Option<String>,
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
    crate::language::start_for_run();
    let data_root_is_real =
        data_root.is_none() && std::env::var_os("NOTCHLING_DATA_ROOT").is_none();
    let root = DataRoot::for_run(data_root.as_deref());
    if let Some(root) = root.as_ref() {
        crate::chat::history::apply(&root.data_dir);
    }
    let notch = root.as_ref().map(DataRoot::notch_source);
    let app = Application::new().with_assets(notchling_kit::ui::KitAssets);
    app.on_reopen(|cx| {
        open_chat(cx);
        cx.activate(true);
    });
    let preference = root
        .as_ref()
        .and_then(|r| crate::appearance::read(&r.data_dir))
        .unwrap_or_default();
    app.run(move |cx: &mut App| {
        crate::appearance::apply(preference);
        crate::mac::app_icons::install();
        dock::register(cx);
        dock::apply(policy_for(0));
        dock::set_main_menu(cx, policy_for(0));
        let state = root
            .as_ref()
            .map(|r| crate::keep_awake::read(&r.data_dir))
            .unwrap_or_default();
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
        if let Some(root) = root.as_ref().filter(|_| feed.is_live()) {
            super::settings_live::watch(cx, feed.clone(), root.data_dir.clone());
        }
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
        let wanted = root
            .as_ref()
            .map(|r| shortcut::read(&r.data_dir))
            .unwrap_or(Setting::DEFAULT);
        let (hot_key, search, search_failure) = match wanted {
            Setting::Off => (None, Setting::Off, None),
            Setting::On(key) => match register_search(cx, key) {
                Ok(registration) => (Some(registration), wanted, None),
                Err(e) => {
                    eprintln!("no {}: {e}", key.label());
                    (None, Setting::Off, Some(e))
                }
            },
        };
        cx.set_global(Shell {
            state,
            tray,
            titled: HashSet::new(),
            chat: None,
            root,
            sessions,
            hot_key,
            search,
            search_failure,
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

fn register_search(cx: &App, key: shortcut::Shortcut) -> Result<Box<dyn Registration>, String> {
    let async_cx = cx.to_async();
    hotkey::for_system(key)?.register(Box::new(move || {
        async_cx.update(crate::search::panel::toggle).ok();
    }))
}

pub fn set_search_shortcut(cx: &mut App, setting: Setting) -> Result<(), String> {
    if !cx.has_global::<Shell>() {
        return Err("The shortcut can't change in this window.".to_owned());
    }
    if cx.global::<Shell>().search == setting {
        return Ok(());
    }
    let registration = match setting {
        Setting::Off => None,
        Setting::On(key) => Some(register_search(cx, key)?),
    };
    let shell = cx.global_mut::<Shell>();
    shell.hot_key = registration;
    shell.search = setting;
    shell.search_failure = None;
    Ok(())
}

pub fn on_search_setting(cx: &mut App, key: &str, value: &serde_json::Value) {
    let before = active_search(cx);
    let Some(wanted) = shortcut::changed_to(key, value, before) else {
        return;
    };
    let outcome = match wanted {
        Setting::On(chosen) => match chosen.problem() {
            Some(problem) => Err(problem.to_owned()),
            None => set_search_shortcut(cx, wanted),
        },
        Setting::Off => set_search_shortcut(cx, wanted),
    };
    if let Err(e) = outcome {
        eprintln!("search shortcut {}: {e}", wanted.label());
        if cx.has_global::<Shell>() {
            cx.global_mut::<Shell>().search_failure = Some(shortcut::refusal(&e, before));
        }
    }
}

pub fn active_search(cx: &App) -> Setting {
    cx.try_global::<Shell>()
        .map(|s| s.search)
        .unwrap_or(Setting::Off)
}

pub fn search_failure(cx: &App) -> Option<String> {
    cx.try_global::<Shell>()?.search_failure.clone()
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
        Command::SetAwake(_) | Command::ToggleDisplay => change_awake(command, cx),
        Command::Quit => cx.quit(),
    }
}

fn change_awake(command: Command, cx: &mut App) {
    let shell = cx.global::<Shell>();
    let Some((key, value)) = crate::keep_awake::wanted(command, &shell.state) else {
        return;
    };
    if let Some(root) = shell.root.as_ref().filter(|r| r.fixture.is_none()) {
        if let Err(e) = crate::keep_awake::write(&root.data_dir, key, &value) {
            eprintln!("keep awake: {key} wasn't saved ({e})");
            return;
        }
    }
    super::settings_live::apply(cx, &[(key.to_owned(), value)]);
}

pub fn on_awake_setting(cx: &mut App, key: &str, value: &serde_json::Value) {
    if !cx.has_global::<Shell>() {
        return;
    }
    let shell = cx.global_mut::<Shell>();
    if let Some(next) = crate::keep_awake::with(&shell.state, key, value) {
        shell.state = next;
        refresh_tray(shell);
    }
}

pub fn on_awake_hold(cx: &mut App, held: bool) {
    if !cx.has_global::<Shell>() {
        return;
    }
    let shell = cx.global_mut::<Shell>();
    if let Some(next) = crate::keep_awake::holding(&shell.state, held) {
        shell.state = next;
        refresh_tray(shell);
    }
}

fn refresh_tray(shell: &Shell) {
    if let Some(tray) = &shell.tray {
        tray.update(&shell.state);
    }
}

pub(crate) fn chat_window(cx: &App) -> Option<WindowHandle<ChatView>> {
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

pub fn open_chat_session(cx: &mut App, target: SessionRef, child: Option<String>) {
    open_chat(cx);
    if let Some(handle) = chat_window(cx) {
        handle
            .update(cx, |view, _, cx| view.show_session(target, child, cx))
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
