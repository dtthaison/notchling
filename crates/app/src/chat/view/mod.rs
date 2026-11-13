mod accounts;
mod composer;
mod conversation;
mod conversation_list;
mod conversation_view;
mod copy;
mod copy_menu;
mod drop;
mod header;
mod inbox;
mod inspector;
mod limit_ring;
pub mod list_probe;
mod list_probe_run;
mod load;
mod markdown;
mod message_actions;
mod native_spin;
mod opinions;
mod page_panel;
pub mod pane;
mod phone;
mod prompts;
mod render_map;
mod root;
mod row_blocks;
mod row_menu;
mod row_select;
pub mod scroll_probe;
mod select;
mod sheets;
mod shell_probe;
mod sidebar;
mod simulator;
mod slash_menu;
mod steps_view;
mod subagent;
mod tasks;
mod terminal;
mod tool;
mod tools;

use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{div, prelude::*, px, Context, Entity, ExternalPaths, Render, Subscription, Window};
use notchling_engine::catalog::{ModelInfo, Tool as CatalogTool};
use notchling_engine::transcript::model::Conversation;
use notchling_engine::transcript::subagents::Subagent;
use notchling_kit::labels::Corner;
use notchling_kit::ui::{
    parts_on, resize_edges, set_parts, window_appearance, KitPart, KitStyled, TopBorder,
};
use notchling_kit::Ink;
use notchling_platform::watch::{Batch, Watcher};

use crate::chat::activity::{self, Activity};
use crate::chat::data::{DataRoot, Loaded, Loader, Session, Tool};
use crate::chat::log::Change;
use crate::chat::message_meta;
use crate::chat::reload::{watch_config, ReloadGate};
use crate::chat::sidebar::{row_for, Pending, PendingJob, PendingSession};
use crate::inbox::feed::InboxFeed;
use crate::inbox::view::InboxPanelView;
use crate::phone::bridge::Bridge;
use crate::phone::view::PhonePanelView;
use crate::search::wire::SessionRef;
use crate::session_source::SessionSource;

const TICK: Duration = Duration::from_millis(500);

pub const EXPAND_ENV: &str = "NOTCHLING_RENDER_EXPAND";
pub const RUN_ENV: &str = "NOTCHLING_RENDER_RUN";
pub const PARTS_ENV: &str = "NOTCHLING_RENDER_PARTS";
pub const LATEST_ENV: &str = "NOTCHLING_RENDER_LATEST";
pub const TYPED_ENV: &str = "NOTCHLING_RENDER_TYPED";
pub const WORKING_ENV: &str = "NOTCHLING_RENDER_WORKING";
pub const ADD_ENV: &str = "NOTCHLING_RENDER_ADD";
pub const PICKER_ENV: &str = "NOTCHLING_RENDER_PICKER";
pub const LIMITS_ENV: &str = limit_ring::LIMITS_ENV;
pub const TASKS_OPEN_ENV: &str = "NOTCHLING_RENDER_TASKS_OPEN";
pub const SIDEBAR_HIDDEN_ENV: &str = "NOTCHLING_RENDER_SIDEBAR_HIDDEN";
pub const DETAILS_HIDDEN_ENV: &str = "NOTCHLING_RENDER_DETAILS_HIDDEN";
pub const ACTIONS_ENV: &str = "NOTCHLING_RENDER_ACTIONS";
pub const DROP_ENV: &str = drop::PREVIEW_ENV;
pub const PROMPT_ENV: &str = "NOTCHLING_RENDER_PROMPT";
pub const TERMINAL_ENV: &str = terminal::RENDER_ENV;
pub const SIMULATOR_ENV: &str = simulator::RENDER_ENV;
pub const SLASH_ENV: &str = "NOTCHLING_RENDER_SLASH";
pub const ROW_MENU_ENV: &str = "NOTCHLING_RENDER_ROW_MENU";
pub const COPY_MENU_ENV: &str = "NOTCHLING_RENDER_COPY_MENU";
pub const PAGE_ENV: &str = page_panel::PAGE_ENV;
pub const INBOX_ENV: &str = "NOTCHLING_RENDER_INBOX";

fn switch(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| v == "1")
}

pub fn bench_point(size: gpui::Size<gpui::Pixels>) -> gpui::Point<gpui::Pixels> {
    let sidebar = notchling_kit::metric::SIDEBAR_WIDTH;
    let column = (f32::from(size.width) - sidebar - inspector::WIDTH).max(0.0);
    gpui::point(px(sidebar + column / 2.0), size.height / 2.0)
}

pub struct ChatView {
    root: DataRoot,
    sessions: Vec<Session>,
    new_drafts: Vec<Session>,
    selected: Option<String>,
    conversation: Arc<Conversation>,
    subagents: Vec<Subagent>,
    open_subagent: Option<crate::chat::subagent_view::Open>,
    now_ms: i64,
    loader: Option<Loader>,
    load_again: bool,
    loading: bool,
    gate: ReloadGate,
    source: Arc<SessionSource>,
    seen_generation: u64,
    seen_catalog: u64,
    daemon_note: Option<&'static str>,
    catalog: HashMap<CatalogTool, Vec<ModelInfo>>,
    watch: Option<(Watcher, Receiver<Batch>)>,
    parts: pane::Parts,
    row_accounts: Vec<Option<(String, bool)>>,
    chrome: Entity<pane::Chrome>,
    tasks_open_preview: bool,
    drop_preview: bool,
    overlay_open: bool,
    page_covered: bool,
    activity_drawn: std::cell::Cell<Option<i64>>,
    tools: tools::ChatTools,
    sheet: Option<sheets::Sheet>,
    bridge: Arc<Bridge>,
    phone: Option<Entity<PhonePanelView>>,
    phone_dot: bool,
    inbox: Arc<InboxFeed>,
    inbox_panel: Option<Entity<InboxPanelView>>,
    seen_inbox: u64,
    pending_session: Option<PendingSession>,
    pending_job: Option<PendingJob>,
    closed: std::collections::HashMap<String, f64>,
    tasks_seen: std::collections::HashMap<String, crate::chat::tasks_bar::Seen>,
    used: std::collections::HashMap<String, f64>,
    delegated: std::collections::HashSet<String>,
    accounts: Arc<crate::accounts::store::Accounts>,
    switches: Arc<crate::switches::feed::SwitchFeed>,
    prompt: prompts::State,
    composer: composer::State,
    limit_ring: limit_ring::State,
    account_menu: bool,
    row_menu: Option<row_menu::RowMenu>,
    copy_menu: Option<copy_menu::CopyMenu>,
    keys: copy::Keys,
    accounts_polled_ms: i64,
    codex_installed: bool,
    sidebar_hidden: bool,
    details_hidden: bool,
    shell_probe: shell_probe::Probe,
    page: page_panel::State,
    terminal: terminal::State,
    simulator: simulator::State,
    slash: crate::chat::slash_menu::SlashMenu,
    slash_commands: Option<(String, Vec<notchling_engine::control::slash::SlashCommand>)>,
    slash_preview: Option<String>,
    _appearance_changes: Subscription,
    _parts_escape: Subscription,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl ChatView {
    pub fn new(root: DataRoot, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = ChatView::with_source(root, Arc::new(SessionSource::files()), window, cx);
        view.parts.conversation.update(cx, |v, _| {
            v.s.expand_all = switch(EXPAND_ENV);
            v.s.latest_preview = switch(LATEST_ENV);
            v.s.actions_preview = switch(ACTIONS_ENV);
            v.s.run_preview = switch(RUN_ENV);
        });
        view.tasks_open_preview = switch(TASKS_OPEN_ENV);
        view.drop_preview = switch(DROP_ENV);
        view.limit_ring = limit_ring::State::preview(switch(LIMITS_ENV));
        view.preview_composer(
            composer::Preview {
                typed: switch(TYPED_ENV),
                working: switch(WORKING_ENV),
                menu: switch(ADD_ENV),
                picker: std::env::var(PICKER_ENV)
                    .ok()
                    .and_then(|v| crate::chat::claude_picker::Kind::parse(&v)),
            },
            cx,
        );
        if let Some(preview) = std::env::var(PROMPT_ENV)
            .ok()
            .and_then(|v| crate::chat::prompts::Preview::parse(&v))
        {
            view.prompt.apply_preview(preview);
        }
        view.sidebar_hidden = switch(SIDEBAR_HIDDEN_ENV);
        view.details_hidden = switch(DETAILS_HIDDEN_ENV);
        view.slash_preview = std::env::var(SLASH_ENV).ok();
        view.load_closed();
        if let Ok(how) = std::env::var(TERMINAL_ENV) {
            view.render_terminal(how == "expanded", cx);
        }
        if let Ok(how) = std::env::var(SIMULATOR_ENV) {
            view.render_simulator(&how, cx);
        }
        if switch(ROW_MENU_ENV) {
            view.row_menu = Some(row_menu::RowMenu {
                row_id: "render-session".into(),
                at: gpui::point(px(120.0), px(110.0)),
                confirm_remove: false,
            });
        }
        if let Ok(how) = std::env::var(COPY_MENU_ENV) {
            view.copy_menu = Some(copy_menu::CopyMenu {
                at: gpui::point(px(520.0), px(240.0)),
                enabled: how != "off",
            });
        }
        if switch(INBOX_ENV) {
            view.preview_inbox(cx);
        }
        if let Ok(id) = std::env::var(PAGE_ENV) {
            view.page.preview(&id);
        }
        if let Ok(path) = std::env::var(page_panel::FILE_ENV) {
            view.page.preview_file(&path);
        }
        view.page.render_only = true;
        if switch(PARTS_ENV) {
            set_parts(cx, true);
        }
        view
    }

    pub(super) fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_hidden = !self.sidebar_hidden;
        cx.notify();
    }

    pub(super) fn toggle_details(&mut self, cx: &mut Context<Self>) {
        if self.page.is_open() {
            self.close_page(cx);
            return;
        }
        self.details_hidden = !self.details_hidden;
        cx.notify();
    }

    pub fn with_source(
        root: DataRoot,
        source: Arc<SessionSource>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let this = cx.weak_entity();
        let changes = window.observe_window_appearance(move |_, cx| {
            this.update(cx, |view, cx| {
                cx.notify();
                view.redraw_parts(cx);
            })
            .ok();
        });
        let watch = Watcher::start(root.watch_folders(), watch_config()).ok();
        let parts_escape = cx.observe_keystrokes(|this, event, window, cx| {
            let keys = &event.keystroke;
            if keys.key == "s"
                && keys.modifiers.platform
                && keys.modifiers.control
                && window.is_window_active()
            {
                this.toggle_sidebar(cx);
                return;
            }
            if keys.key == "["
                && keys.modifiers.platform
                && window.is_window_active()
                && this.open_subagent.is_some()
            {
                this.close_subagent(cx);
                return;
            }
            if keys.key != "escape" || this.terminal_has_keys(window) {
                return;
            }
            use crate::chat::escape::{escape, Escape, Open};
            let open = Open {
                parts: parts_on(cx),
                menu: this.composer.is_open() || this.tools.picker.is_some(),
                popup: this.sheet.is_some()
                    || this.phone.is_some()
                    || this.inbox_panel.is_some()
                    || this.account_menu
                    || this.row_menu.is_some()
                    || this.copy_menu.is_some(),
                subagent: this.open_subagent.is_some(),
                working: this.selected_working(),
            };
            match escape(open) {
                Escape::HideParts => set_parts(cx, false),
                Escape::CloseMenu => {
                    this.composer.close_menu();
                    this.tools.picker = None;
                    cx.notify();
                }
                Escape::ClosePopup => {
                    this.sheet = None;
                    this.phone = None;
                    this.inbox_panel = None;
                    this.account_menu = false;
                    this.row_menu = None;
                    this.copy_menu = None;
                    cx.notify();
                }
                Escape::CloseSubagent => this.close_subagent(cx),
                Escape::Stop => this.stop(cx),
                Escape::Nothing => {}
            }
        });
        let (accounts, switches) = crate::accounts::window::app_stores(cx);
        let mut view = ChatView {
            accounts,
            switches,
            prompt: prompts::State::new(None, cx),
            composer: composer::State::default(),
            limit_ring: limit_ring::State::default(),
            account_menu: false,
            row_menu: None,
            copy_menu: None,
            keys: copy::Keys::new(cx),
            accounts_polled_ms: 0,
            codex_installed: accounts::codex_installed(&root.home),
            loader: Some(Loader::new(root.clone())),
            root,
            sessions: Vec::new(),
            new_drafts: Vec::new(),
            selected: None,
            conversation: Arc::default(),
            subagents: Vec::new(),
            open_subagent: None,
            now_ms: 0,
            load_again: false,
            loading: true,
            gate: ReloadGate::default(),
            seen_generation: source.generation(),
            seen_catalog: source.catalog_generation(),
            daemon_note: source.view().note(),
            catalog: HashMap::from([(CatalogTool::Claude, source.catalog(CatalogTool::Claude))]),
            source,
            watch,
            parts: {
                let chat = cx.entity();
                pane::Parts::new(&chat, cx)
            },
            row_accounts: Vec::new(),
            chrome: cx.new(|_| pane::Chrome),
            tasks_open_preview: false,
            drop_preview: false,
            sidebar_hidden: false,
            details_hidden: false,
            shell_probe: shell_probe::Probe::default(),
            page: page_panel::State::default(),
            terminal: terminal::State::default(),
            simulator: simulator::State::default(),
            slash: Default::default(),
            slash_commands: None,
            slash_preview: None,
            overlay_open: false,
            page_covered: false,
            activity_drawn: std::cell::Cell::new(None),
            tools: tools::ChatTools::new(cx),
            sheet: None,
            bridge: phone::app_bridge(cx),
            phone: None,
            phone_dot: false,
            inbox: inbox::app_inbox(cx),
            inbox_panel: None,
            seen_inbox: 0,
            pending_session: None,
            pending_job: None,
            closed: std::collections::HashMap::new(),
            used: std::collections::HashMap::new(),
            delegated: std::collections::HashSet::new(),
            tasks_seen: std::collections::HashMap::new(),
            _appearance_changes: changes,
            _parts_escape: parts_escape,
        };
        view.restore_drafts(cx);
        view.start_load(cx);
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(TICK).await;
            if this.update(cx, |view, cx| view.tick(cx)).is_err() {
                break;
            }
        })
        .detach();
        view
    }

    pub fn bench_type(&mut self, text: &str, cx: &mut Context<Self>) {
        self.tools
            .editor
            .update(cx, |editor, cx| editor.insert(text, cx));
    }

    pub fn bench_reload(&mut self, cx: &mut Context<Self>) {
        self.reload_now(cx);
    }

    pub fn bench_menu(&mut self, cx: &mut Context<Self>) {
        self.toggle_account_menu(cx);
    }

    pub fn selected_session(&self) -> Option<&Session> {
        self.sessions
            .iter()
            .find(|s| Some(&s.agent.id) == self.selected.as_ref())
    }

    pub(super) fn activity_line(&self) -> Option<Activity> {
        if self.loading {
            return None;
        }
        activity::line(
            &self.conversation,
            self.selected_working() || self.selected_held_working(),
            now_ms() as f64 / 1000.0,
        )
    }

    pub(super) fn stalled_prompt(&self) -> Option<String> {
        if self.loading || self.stopping() {
            return None;
        }
        let session = self.selected_session()?;
        if session.tool != crate::chat::data::Tool::Claude || !session.agent.can_reply() {
            return None;
        }
        let retrying = self.tools.retrying.as_ref().is_some_and(|(row, at)| {
            *row == session.agent.id && now_ms() - at < crate::chat::activity::STOPPING_MS * 3
        });
        if retrying {
            return None;
        }
        activity::stalled(
            &self.conversation,
            self.selected_working(),
            now_ms() as f64 / 1000.0,
        )
    }
}
