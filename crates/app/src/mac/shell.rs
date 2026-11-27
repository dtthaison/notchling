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
use crate::dock::{plan, policy_for, Cause, Policy};
use crate::inbox::tray_rows::{deny_route, Attention, DenyRoute};
use crate::menu::{tray_look, BlockAct, Command, MenuState};
use crate::notch::blocking::{allow_route, AllowRoute, NoteRoute};
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
use crate::tray_sync::TraySync;
use crate::welcome::window::{self as welcome_window, WelcomeSetup, Which};
use notchling_engine::delegate::second_opinion_store::SecondOpinionStore;

struct Shell {
    state: MenuState,
    tray: Option<Tray>,
    sync: TraySync,
    reduce_motion: bool,
    badge: Option<u32>,
    titled: HashSet<WindowId>,
    chat: Option<WindowHandle<ChatView>>,
    root: Option<DataRoot>,
    sessions: Arc<SessionSource>,
    hot_key: Option<Box<dyn Registration>>,
    search: Setting,
    search_failure: Option<String>,
    show_in_dock: bool,
    dock_retry: bool,
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

pub struct AppCluster(pub Arc<crate::cluster::feed::ClusterFeed>);

impl Global for AppCluster {}

pub struct AppAccounts(pub Arc<crate::accounts::store::Accounts>);

impl Global for AppAccounts {}

pub struct AppVault(pub Arc<crate::vault::store::Vault>);

impl Global for AppVault {}

pub struct AppOpinions(pub Option<SecondOpinionStore>);

impl Global for AppOpinions {}

const OPINION_STOP_GRACE: Duration = Duration::from_secs(2);

const BRIDGE_POLL: Duration = Duration::from_secs(5);

const DOCK_RETRY: Duration = Duration::from_millis(500);

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
        let show_in_dock = root
            .as_ref()
            .map(|r| crate::dock::read(&r.data_dir))
            .unwrap_or(true);
        dock::apply(policy_for(0, show_in_dock));
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
        let for_tray = sessions.clone();
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
        cx.set_global(AppCluster(Arc::new(
            crate::cluster::feed::ClusterFeed::for_run(root.as_ref()),
        )));
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
            let ending = vault.clone();
            cx.on_app_quit(move |_| {
                let ending = ending.clone();
                async move {
                    ending.flush_cancels(crate::vault::closing::QUIT_WAIT);
                }
            })
            .detach();
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
        let update_root = root.clone();
        cx.set_global(Shell {
            state,
            tray,
            sync: TraySync::default(),
            reduce_motion: false,
            badge: None,
            titled: HashSet::new(),
            chat: None,
            root,
            sessions,
            hot_key,
            search,
            search_failure,
            show_in_dock,
            dock_retry: false,
        });
        apply_dock(cx, Cause::Quiet);
        let inbox_feed = cx.global::<AppInbox>().0.clone();
        let data_dir = cx
            .global::<Shell>()
            .root
            .as_ref()
            .map(|r| r.data_dir.clone());
        super::attention_watch::watch(cx, inbox_feed, for_tray, data_dir);
        cx.on_app_quit(|_| {
            dock::set_badge(None);
            async {}
        })
        .detach();
        cx.on_window_closed(update_dock).detach();
        if let Some(source) = notch {
            if let Some(handle) = crate::notch::panel::open(cx, source, for_notch, actions) {
                cx.set_global(crate::notch::panel::NotchWindow(handle));
            }
        }
        if let Some(root) = update_root.as_ref().filter(|r| r.fixture.is_none()) {
            super::update_watch::watch(cx, root);
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
        Command::OpenUpdate => super::update_watch::open_release_page(cx),
        Command::SetAwake(_) | Command::ToggleDisplay => change_awake(command, cx),
        Command::Quit => cx.quit(),
        Command::Blocking(act, id) => blocking_act(act, &id, cx).detach(),
    }
}

pub(crate) fn blocking_act(act: BlockAct, id: &str, cx: &mut App) -> gpui::Task<bool> {
    let Some(feed) = cx.try_global::<AppInbox>().map(|inbox| inbox.0.clone()) else {
        return gpui::Task::ready(false);
    };
    let item = feed
        .snapshot(crate::phone::bridge::now_ms())
        .item(id)
        .cloned();
    let (Some(item), true) = (item, feed.is_live()) else {
        open_chat_inbox_at(cx, id);
        return gpui::Task::ready(false);
    };
    if !crate::inbox::banners::answerable(&item) {
        open_chat_inbox_at(cx, id);
        return gpui::Task::ready(false);
    }
    match act {
        BlockAct::Allow => match allow_route(&item) {
            Some(route) => {
                let row = id.to_owned();
                cx.spawn(async move |cx| {
                    let refused = cx
                        .background_executor()
                        .spawn(async move {
                            let result = match route {
                                AllowRoute::Consent(consent) => feed.consent_answer(&consent, true),
                                AllowRoute::Switch { consent, switch } => {
                                    feed.allow_switch(&consent, &switch).map(|_| true)
                                }
                                AllowRoute::ApprovalOnce(ticket) => {
                                    feed.approval_allow_once(&ticket)
                                }
                            };
                            crate::inbox::banners::allow_refused(&result)
                        })
                        .await;
                    if refused {
                        cx.update(|cx| open_chat_inbox_at(cx, &row)).ok();
                    }
                    !refused
                })
            }
            None => {
                open_chat_inbox_at(cx, id);
                gpui::Task::ready(false)
            }
        },
        BlockAct::Deny => match deny_route(&item) {
            Some(route) => {
                let id = item.id.clone();
                cx.spawn(async move |cx| {
                    let refused = cx
                        .background_executor()
                        .spawn({
                            let id = id.clone();
                            async move {
                                let result = match route {
                                    DenyRoute::Consent(consent) => {
                                        feed.consent_answer(&consent, false)
                                    }
                                    DenyRoute::Answer(action) => feed.answer(&id, action),
                                };
                                crate::inbox::banners::allow_refused(&result)
                            }
                        })
                        .await;
                    if refused {
                        cx.update(|cx| open_chat_inbox_at(cx, &id)).ok();
                    }
                    !refused
                })
            }
            None => {
                open_chat_inbox_at(cx, id);
                gpui::Task::ready(false)
            }
        },
        BlockAct::Change | BlockAct::OpenInbox => {
            open_chat_inbox_at(cx, id);
            gpui::Task::ready(false)
        }
    }
}

pub fn pulse_tray(cx: &mut App) {
    if let Some(tray) = cx.try_global::<Shell>().and_then(|s| s.tray.as_ref()) {
        tray.pulse_again();
    }
}

pub fn on_notch_effect(cx: &mut App, effect: &crate::attention::Effect) {
    crate::notch::panel::apply_effects(cx, std::slice::from_ref(effect));
}

pub(crate) fn blocking_note(
    route: NoteRoute,
    note: String,
    cx: &mut App,
) -> Option<gpui::Task<Result<bool, String>>> {
    let feed = cx.try_global::<AppInbox>().map(|inbox| inbox.0.clone())?;
    if !feed.is_live() {
        return None;
    }
    Some(cx.background_executor().spawn(async move {
        match route {
            NoteRoute::Consent(consent) => feed.consent_deny_with(&consent, &note),
            NoteRoute::Approval(ticket) => feed.approval_deny_with(&ticket, &note),
            NoteRoute::Hook(_) => Ok(false),
        }
    }))
}

pub fn on_attention(cx: &mut App, attention: &Attention, working: bool, reduce_motion: bool) {
    if !cx.has_global::<Shell>() {
        return;
    }
    let shell = cx.global_mut::<Shell>();
    shell.reduce_motion = reduce_motion;
    shell.state.blocking = attention.blocking.clone();
    shell.state.waiting_count = attention.waiting_count;
    shell.state.working = working;
    refresh_tray(shell);
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

pub fn on_update(cx: &mut App, version: Option<String>) {
    if !cx.has_global::<Shell>() {
        return;
    }
    let shell = cx.global_mut::<Shell>();
    if shell.state.update != version {
        shell.state.update = version;
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

fn refresh_tray(shell: &mut Shell) {
    let look = tray_look(&shell.state, shell.reduce_motion);
    if let Some(tray) = shell.tray.as_mut() {
        shell.sync.refresh(tray, look, &shell.state);
    }
    refresh_badge(shell);
}

fn refresh_badge(shell: &mut Shell) {
    let wanted = crate::dock::badge_for(shell.state.blocking.len(), shell.show_in_dock);
    if shell.badge != wanted {
        shell.badge = wanted;
        dock::set_badge(wanted);
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

pub fn open_chat_reload(cx: &mut App, id: &str) {
    open_chat(cx);
    if let Some(handle) = chat_window(cx) {
        let id = id.to_owned();
        handle
            .update(cx, |view, _, cx| view.show_reload(&id, cx))
            .ok();
    }
}

pub fn open_chat_ask(cx: &mut App, session: &str, id: &str) {
    open_chat(cx);
    if let Some(handle) = chat_window(cx) {
        let (session, id) = (session.to_owned(), id.to_owned());
        handle
            .update(cx, |view, _, cx| view.show_ask(&session, &id, cx))
            .ok();
    }
}

pub fn open_chat_vault(cx: &mut App, ids: &[String]) -> bool {
    let openable = chat_window(cx).is_some() || cx.global::<Shell>().root.is_some();
    if !openable {
        return false;
    }
    open_chat(cx);
    let Some(handle) = chat_window(cx) else {
        return false;
    };
    let ids = ids.to_vec();
    handle
        .update(cx, |view, window, cx| view.show_vault(&ids, window, cx))
        .is_ok()
}

pub fn open_chat_inbox(cx: &mut App) {
    open_chat(cx);
    if let Some(handle) = chat_window(cx) {
        handle.update(cx, |view, _, cx| view.show_inbox(cx)).ok();
    }
}

pub fn open_chat_inbox_at(cx: &mut App, id: &str) {
    open_chat(cx);
    if let Some(handle) = chat_window(cx) {
        let id = id.to_owned();
        handle
            .update(cx, |view, _, cx| view.show_inbox_at(&id, cx))
            .ok();
    }
}

pub fn open_chat_settings(cx: &mut App) {
    open_chat(cx);
    if let Some(handle) = chat_window(cx) {
        handle.update(cx, |view, _, cx| view.show_settings(cx)).ok();
    }
}

pub fn open_chat_settings_agents(cx: &mut App) {
    open_chat(cx);
    if let Some(handle) = chat_window(cx) {
        handle
            .update(cx, |view, _, cx| view.show_settings_agents(cx))
            .ok();
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

pub fn has_shell(cx: &App) -> bool {
    cx.has_global::<Shell>()
}

pub fn note_titled(cx: &mut App, id: WindowId) {
    cx.global_mut::<Shell>().titled.insert(id);
    update_dock(cx);
}

fn update_dock(cx: &mut App) {
    apply_dock(cx, Cause::Window);
}

fn popup_is_key(cx: &mut App) -> bool {
    let titled = cx.global::<Shell>().titled.clone();
    cx.windows()
        .into_iter()
        .filter(|w| !titled.contains(&w.window_id()))
        .any(|w| {
            w.update(cx, |_, window, _| window.is_window_active())
                .unwrap_or(false)
        })
}

fn apply_dock(cx: &mut App, cause: Cause) {
    let open: HashSet<WindowId> = cx.windows().iter().map(|w| w.window_id()).collect();
    let shell = cx.global_mut::<Shell>();
    shell.titled.retain(|id| open.contains(id));
    let (titled, show_in_dock) = (shell.titled.len(), shell.show_in_dock);
    let popup_key = policy_for(titled, show_in_dock) == Policy::Accessory && popup_is_key(cx);
    let next = plan(titled, show_in_dock, popup_key, cause);
    if next.hold {
        retry_dock(cx);
    } else {
        dock::apply(next.policy);
        dock::set_main_menu(cx, next.policy);
    }
    if next.activate {
        cx.activate(true);
    }
}

fn retry_dock(cx: &mut App) {
    if std::mem::replace(&mut cx.global_mut::<Shell>().dock_retry, true) {
        return;
    }
    cx.spawn(async move |cx| {
        cx.background_executor().timer(DOCK_RETRY).await;
        cx.update(|cx| {
            cx.global_mut::<Shell>().dock_retry = false;
            apply_dock(cx, Cause::Quiet);
        })
        .ok();
    })
    .detach();
}

pub fn on_dock_setting(cx: &mut App, key: &str, value: &serde_json::Value) {
    if !cx.has_global::<Shell>() {
        return;
    }
    let Some(wanted) = crate::dock::changed_to(key, value) else {
        return;
    };
    if cx.global::<Shell>().show_in_dock == wanted {
        return;
    }
    let shell = cx.global_mut::<Shell>();
    shell.show_in_dock = wanted;
    refresh_badge(shell);
    cx.spawn(async move |cx| {
        cx.update(|cx| apply_dock(cx, Cause::Quiet)).ok();
    })
    .detach();
}
