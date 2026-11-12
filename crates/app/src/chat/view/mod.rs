//! The Chat window's GPUI views (macOS): sidebar, header, conversation, the reply box
//! (`composer`), inspector. The view reloads when `platform::watch` reports a change under the
//! data root (at most every 2 s while changes keep arriving) and every 30 s for the ages
//! (`chat::reload`). Every load runs on the background executor: the [`Loader`] is sent there and
//! taken back with the result, so the window never waits on the disk. The session rows come from
//! the daemon (`session_source`) while it answers, and so do the other tools' live state and an
//! Antigravity row's conversation (`tools`). Files dropped anywhere on the middle column go into
//! the reply box (`drop`), and the ? button names every part (`notchling_kit::ui::KitPart`).

mod accounts;
mod composer;
mod conversation;
mod conversation_view;
mod drop;
mod header;
mod inspector;
mod limit_ring;
pub mod list_probe;
mod list_probe_run;
mod load;
mod markdown;
mod message_actions;
mod opinions;
mod page_panel;
pub mod pane;
mod phone;
mod prompts;
mod root;
mod row_menu;
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
use crate::phone::bridge::Bridge;
use crate::phone::view::PhonePanelView;
use crate::search::wire::SessionRef;
use crate::session_source::SessionSource;

/// How often the view looks for watch batches.
const TICK: Duration = Duration::from_millis(500);

/// Set to `1`, a render draws every step group, step and thought open.
pub const EXPAND_ENV: &str = "NOTCHLING_RENDER_EXPAND";
/// Set to `1`, a render draws the part names, as the ? button shows them.
pub const PARTS_ENV: &str = "NOTCHLING_RENDER_PARTS";
/// Set to `1`, a render draws the "Latest" pill, as if the conversation were scrolled up.
pub const LATEST_ENV: &str = "NOTCHLING_RENDER_LATEST";
/// Set to `1`, a render types a sample into the reply box.
pub const TYPED_ENV: &str = "NOTCHLING_RENDER_TYPED";
/// Set to `1`, a render draws the session as working, so the reply box shows Stop.
pub const WORKING_ENV: &str = "NOTCHLING_RENDER_WORKING";
/// Set to `1`, a render draws the reply box's + menu open.
pub const ADD_ENV: &str = "NOTCHLING_RENDER_ADD";
/// Set to `model` or `effort`, a render draws that Claude Code picker's menu open.
pub const PICKER_ENV: &str = "NOTCHLING_RENDER_PICKER";
/// Set to `1`, a render draws the usage-limit card open (`limit_ring::LIMITS_ENV`).
pub const LIMITS_ENV: &str = limit_ring::LIMITS_ENV;
/// Set to `1`, a render draws the Tasks bar open, its list under it.
pub const TASKS_OPEN_ENV: &str = "NOTCHLING_RENDER_TASKS_OPEN";
/// Set to `1`, a render draws the sidebar hidden, as its toggle leaves it.
pub const SIDEBAR_HIDDEN_ENV: &str = "NOTCHLING_RENDER_SIDEBAR_HIDDEN";
/// Set to `1`, a render draws the Details panel hidden.
pub const DETAILS_HIDDEN_ENV: &str = "NOTCHLING_RENDER_DETAILS_HIDDEN";
/// Set to `1`, a render draws every message's row of actions, as if the pointer were over each.
pub const ACTIONS_ENV: &str = "NOTCHLING_RENDER_ACTIONS";
/// Set to `1`, a render draws the drop hint over the middle column (`drop::PREVIEW_ENV`).
pub const DROP_ENV: &str = drop::PREVIEW_ENV;
/// Set to `question` or `permission`, a render draws that card above the reply box, over the
/// selected Claude Code session, with its controls live (`chat::prompts::Preview`).
pub const PROMPT_ENV: &str = "NOTCHLING_RENDER_PROMPT";
/// Set to `1` (or `expanded`), a render draws the terminal panel with two tabs over a sample screen.
pub const TERMINAL_ENV: &str = terminal::RENDER_ENV;
/// Set, a render draws the iOS Simulator panel with a sample device.
pub const SIMULATOR_ENV: &str = simulator::RENDER_ENV;
/// Set to a text such as `/co`, a render types it in the box, so the command list shows.
pub const SLASH_ENV: &str = "NOTCHLING_RENDER_SLASH";
/// Set to `1`, a render draws the first sidebar row's right-click menu open.
pub const ROW_MENU_ENV: &str = "NOTCHLING_RENDER_ROW_MENU";
/// Set to a page id, a render draws the page panel over it (`page_panel::PAGE_ENV`).
pub const PAGE_ENV: &str = page_panel::PAGE_ENV;

/// Whether a render switch is on.
fn switch(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| v == "1")
}

/// The middle of the conversation column in a window of `size`, in window points: what the bench
/// moves the mouse and the wheel over.
pub fn bench_point(size: gpui::Size<gpui::Pixels>) -> gpui::Point<gpui::Pixels> {
    let sidebar = notchling_kit::metric::SIDEBAR_WIDTH;
    let column = (f32::from(size.width) - sidebar - inspector::WIDTH).max(0.0);
    gpui::point(px(sidebar + column / 2.0), size.height / 2.0)
}

pub struct ChatView {
    root: DataRoot,
    sessions: Vec<Session>,
    /// Codex and Antigravity rows New Session opened, listed until their first message starts
    /// the session or another row is picked.
    new_drafts: Vec<Session>,
    selected: Option<String>,
    conversation: Arc<Conversation>,
    /// What the last load changed in `conversation` and the conversation it started from; Task 4.4's
    /// `ConversationView` reads it to splice rows.
    pending_change: Option<(Arc<Conversation>, Change)>,
    subagents: Vec<Subagent>,
    /// The subagent shown read-only in place of the conversation (opened from the Details Panel or
    /// an Agent step). Kept here, not in the load, so it outlives every reload; cleared by Back,
    /// Esc, ⌘[ and picking another session.
    open_subagent: Option<crate::chat::subagent_view::Open>,
    now_ms: i64,
    /// `None` while a load has it on the background executor.
    loader: Option<Loader>,
    /// Another load is wanted as soon as the running one ends (a pick, or a change).
    load_again: bool,
    /// A session was picked and its conversation hasn't arrived: draw nothing rather than the
    /// previous session's.
    loading: bool,
    gate: ReloadGate,
    /// Claude Code's rows from the daemon while it answers; the folders are read otherwise.
    source: Arc<SessionSource>,
    /// The source's generation at the last tick, so a change of rows or link reloads.
    seen_generation: u64,
    /// The source's catalog generation at the last tick.
    seen_catalog: u64,
    /// What the sidebar footer says about the daemon ("Daemon not running", "Daemon not trusted").
    daemon_note: Option<&'static str>,
    /// The models each tool offers, as the daemon's `catalog.get` last said; a tool with no entry
    /// (no daemon, or it hasn't answered) shows the built-in list. The Model Picker, Effort Picker
    /// and Context Meter read it.
    catalog: HashMap<CatalogTool, Vec<ModelInfo>>,
    watch: Option<(Watcher, Receiver<Batch>)>,
    /// The conversation's column, the Sidebar and the Details Panel, each drawn cached apart from
    /// the rest of the window, and the layer that turns their spinners (`pane`).
    parts: pane::Parts,
    /// The account each sidebar row named when last polled: a poll that changes one draws the
    /// Sidebar again.
    row_accounts: Vec<Option<(String, bool)>>,
    /// Woken for a redraw of everything but the conversation (`redraw_chrome`).
    chrome: Entity<pane::Chrome>,
    /// The Tasks bar drawn open (`NOTCHLING_RENDER_TASKS_OPEN=1`).
    tasks_open_preview: bool,
    /// The drop hint drawn as if a file were over the column (`NOTCHLING_RENDER_DROP=1`), since
    /// nothing drags in a picture.
    drop_preview: bool,
    /// Whether a sheet, menu, panel or the part names were up when last drawn: Esc then belongs
    /// to them, not to Stop.
    overlay_open: bool,
    /// Whether something drawn over the page panel is open, so its web view hides.
    page_covered: bool,
    /// The reply box, the other tools' live state and the selected Antigravity row's conversation.
    tools: tools::ChatTools,
    /// New Session or Settings, over the window.
    sheet: Option<sheets::Sheet>,
    /// The phone bridge the Phone button and its panel read.
    bridge: Arc<Bridge>,
    /// The Phone panel, while it is open.
    phone: Option<Entity<PhonePanelView>>,
    /// Whether the Phone button's dot is lit, so a change redraws it.
    phone_dot: bool,
    /// A session the search panel asked for that isn't listed yet: selected when it appears.
    pending_session: Option<PendingSession>,
    /// The job New Session just started: selected when the sidebar lists it.
    pending_job: Option<PendingJob>,
    /// Codex and Antigravity rows closed in Chat (`chat::closed`), kept in the settings file.
    closed: std::collections::HashMap<String, f64>,
    /// What the Tasks bar saw of each session's list (`chat::tasks_bar`), by row id.
    tasks_seen: std::collections::HashMap<String, crate::chat::tasks_bar::Seen>,
    /// Codex and Antigravity rows used in Chat (`chat::used`): only these are in the Sidebar.
    used: std::collections::HashMap<String, f64>,
    /// The accounts store the header's meters and account menu read, and the switch feed.
    accounts: Arc<crate::accounts::store::Accounts>,
    switches: Arc<crate::switches::feed::SwitchFeed>,
    /// The question or permission card above the reply box.
    prompt: prompts::State,
    /// The reply box's + menu, and a render's forced states.
    composer: composer::State,
    /// The usage-limit ring's hover state under the reply box.
    limit_ring: limit_ring::State,
    /// The header's account menu is open.
    account_menu: bool,
    /// The menu a right click on a sidebar row opened.
    row_menu: Option<row_menu::RowMenu>,
    /// When the accounts store was last read for the header (ms).
    accounts_polled_ms: i64,
    /// Whether `codex` is installed (the account menu's Codex Accounts…), looked up with the
    /// accounts poll rather than on every render.
    codex_installed: bool,
    /// The sidebar is hidden (its toggle beside the traffic lights, ⌘⌃S). Kept for this window
    /// only: the Swift app has no such toggle, so there is no saved setting to follow.
    sidebar_hidden: bool,
    /// The Details panel is hidden. The Swift app saves this as `chatInspector` (hidden until
    /// first opened); here the panel starts shown and the choice lasts for the window.
    details_hidden: bool,
    shell_probe: shell_probe::Probe,
    /// The page panel: an Artifact clicked in a conversation, in the Details Panel's place. The
    /// window's, not a session's, so picking another session keeps it.
    page: page_panel::State,
    /// The terminal panel: its tabs, whether it shows, and whether it is expanded.
    terminal: terminal::State,
    /// The iOS Simulator panel (`view::simulator`).
    simulator: simulator::State,
    /// The command list over the reply box, and the commands of the folder last read.
    slash: crate::chat::slash_menu::SlashMenu,
    slash_commands: Option<(String, Vec<notchling_engine::control::slash::SlashCommand>)>,
    /// What a render types in the box (`SLASH_ENV`).
    slash_preview: Option<String>,
    _appearance_changes: Subscription,
    /// Esc hides the part names, wherever the keys go.
    _parts_escape: Subscription,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl ChatView {
    /// A Chat that reads the folders only (a render, a fixture root).
    pub fn new(root: DataRoot, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = ChatView::with_source(root, Arc::new(SessionSource::files()), window, cx);
        view.parts.conversation.update(cx, |v, _| {
            v.s.expand_all = switch(EXPAND_ENV);
            v.s.latest_preview = switch(LATEST_ENV);
            v.s.actions_preview = switch(ACTIONS_ENV);
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
        view.prompt.preview = std::env::var(PROMPT_ENV)
            .ok()
            .and_then(|v| crate::chat::prompts::Preview::parse(&v));
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
            // The sessions load after this; the menu needs only an id.
            view.row_menu = Some(row_menu::RowMenu {
                row_id: "render-session".into(),
                at: gpui::point(px(120.0), px(110.0)),
                confirm_remove: false,
            });
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

    /// Hides the sidebar, or shows it again.
    pub(super) fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_hidden = !self.sidebar_hidden;
        cx.notify();
    }

    /// Hides the Details panel, or shows it again. With a page open in its place, back to Details.
    pub(super) fn toggle_details(&mut self, cx: &mut Context<Self>) {
        if self.page.is_open() {
            self.close_page(cx);
            return;
        }
        self.details_hidden = !self.details_hidden;
        cx.notify();
    }

    /// Starts the first load (it selects the first session) and watching for changes; the session
    /// rows come from `source` while the daemon answers.
    pub fn with_source(
        root: DataRoot,
        source: Arc<SessionSource>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let this = cx.weak_entity();
        let changes = window.observe_window_appearance(move |_, cx| {
            // The parts read the appearance as they draw: ask each for a draw whatever its model.
            this.update(cx, |view, cx| {
                cx.notify();
                view.redraw_parts(cx);
            })
            .ok();
        });
        let watch = Watcher::start(root.watch_folders(), watch_config()).ok();
        // Esc turns the part names off from any window; the editor lets Esc through.
        // Otherwise Esc stops a working session's turn, as in the terminal, unless something was
        // up that Esc closes (observers run after the handlers, so it is read as last drawn).
        let parts_escape = cx.observe_keystrokes(|this, event, window, cx| {
            // ⌘⌃S, the system's shortcut for the sidebar, in the window that is up.
            let keys = &event.keystroke;
            if keys.key == "s"
                && keys.modifiers.platform
                && keys.modifiers.control
                && window.is_window_active()
            {
                this.toggle_sidebar(cx);
                return;
            }
            // ⌘[ goes back from a subagent, as in the Swift app.
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
                    || this.account_menu
                    || this.row_menu.is_some(),
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
                    this.account_menu = false;
                    this.row_menu = None;
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
            accounts_polled_ms: 0,
            codex_installed: accounts::codex_installed(&root.home),
            loader: Some(Loader::new(root.clone())),
            root,
            sessions: Vec::new(),
            new_drafts: Vec::new(),
            selected: None,
            conversation: Arc::default(),
            pending_change: None,
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
            tools: tools::ChatTools::new(cx),
            sheet: None,
            bridge: phone::app_bridge(cx),
            phone: None,
            phone_dot: false,
            pending_session: None,
            pending_job: None,
            closed: std::collections::HashMap::new(),
            used: std::collections::HashMap::new(),
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

    /// `--render`'s bench: types into the reply box, as a person would.
    pub fn bench_type(&mut self, text: &str, cx: &mut Context<Self>) {
        self.tools
            .editor
            .update(cx, |editor, cx| editor.insert(text, cx));
    }

    /// `--render`'s bench: a reload that finds nothing changed on disk.
    pub fn bench_reload(&mut self, cx: &mut Context<Self>) {
        self.reload_now(cx);
    }

    /// `--render`'s bench: opens or closes the account menu, a change no cached part shows.
    pub fn bench_menu(&mut self, cx: &mut Context<Self>) {
        self.toggle_account_menu(cx);
    }

    /// The session the conversation shows.
    pub fn selected_session(&self) -> Option<&Session> {
        self.sessions
            .iter()
            .find(|s| Some(&s.agent.id) == self.selected.as_ref())
    }

    /// The live line for the selected session: what it is doing, or none while it is idle.
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

    /// The prompt Retry would send again: a Claude Code session's turn that got nothing back
    /// (`activity::stalled`), unless a Stop or a Retry is on its way.
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
