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
mod drop;
mod header;
mod inspector;
mod limit_ring;
mod markdown;
mod message_actions;
mod opinions;
mod page_panel;
mod phone;
mod prompts;
mod row_menu;
mod sheets;
mod sidebar;
mod slash_menu;
mod tasks;
mod terminal;
mod tools;

use std::collections::HashSet;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{
    div, prelude::*, px, Context, Entity, ExternalPaths, Render, ScrollHandle, Subscription, Window,
};
use notchling_engine::transcript::model::Conversation;
use notchling_engine::transcript::subagents::Subagent;
use notchling_kit::labels::Corner;
use notchling_kit::ui::{
    parts_on, resize_edges, scrollable, set_parts, window_appearance, KitPart, KitStyled, TopBorder,
};
use notchling_kit::Ink;
use notchling_platform::watch::{Batch, Watcher};

use crate::chat::activity::{self, Activity};
use crate::chat::data::{DataRoot, Loaded, Loader, Session, Tool};
use crate::chat::follow::{self, Scroll};
use crate::chat::message_meta;
use crate::chat::reload::{watch_config, ReloadGate};
use crate::chat::sidebar::{row_for, Pending, PendingSession};
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

pub struct ChatView {
    root: DataRoot,
    sessions: Vec<Session>,
    selected: Option<String>,
    conversation: Conversation,
    subagents: Vec<Subagent>,
    now_ms: i64,
    /// `None` while a load has it on the background executor.
    loader: Option<Loader>,
    /// Another load is wanted as soon as the running one ends (a pick, or a change).
    load_again: bool,
    /// A session was picked and its conversation hasn't arrived: draw nothing rather than the
    /// previous session's.
    loading: bool,
    /// Scroll to the end once the picked session's conversation is in.
    scroll_to_end: bool,
    gate: ReloadGate,
    /// Claude Code's rows from the daemon while it answers; the folders are read otherwise.
    source: Arc<SessionSource>,
    /// The source's generation at the last tick, so a change of rows or link reloads.
    seen_generation: u64,
    /// What the sidebar footer says about the daemon ("Daemon not running", "Daemon not trusted").
    daemon_note: Option<&'static str>,
    watch: Option<(Watcher, Receiver<Batch>)>,
    conversation_scroll: ScrollHandle,
    /// The step groups, steps and thoughts clicked open, by item or call id (`ChatExpansion` in
    /// the Swift app). Kept outside the rows so a reload doesn't fold them.
    expanded: HashSet<String>,
    /// Every group, step and thought drawn open: a render over a fixture
    /// (`NOTCHLING_RENDER_EXPAND=1`), since nothing clicks in a picture.
    expand_all: bool,
    /// The Tasks bar drawn open (`NOTCHLING_RENDER_TASKS_OPEN=1`).
    tasks_open_preview: bool,
    /// The drop hint drawn as if a file were over the column (`NOTCHLING_RENDER_DROP=1`), since
    /// nothing drags in a picture.
    drop_preview: bool,
    /// The "Latest" pill drawn as if scrolled up (`NOTCHLING_RENDER_LATEST=1`).
    latest_preview: bool,
    /// Every message's row of actions drawn, as if hovered (`NOTCHLING_RENDER_ACTIONS=1`).
    actions_preview: bool,
    /// The message whose Copy was clicked and when (ms), for its check.
    copied: Option<(String, i64)>,
    /// The message the pointer is over. Set without a redraw: the row shows by a group hover, and
    /// this only says whether its time needs keeping fresh.
    hovered_message: Option<String>,
    /// When the conversation was last drawn (ms), so a hovered row's relative time is redrawn
    /// once it is stale.
    drawn_ms: i64,
    /// A message was sent and hasn't shown up in a reload yet: that reload goes to the end.
    follow_sent: bool,
    /// The wall-clock second the live line was last redrawn for (its clock ticks once a second).
    clock_second: i64,
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
    /// The page panel: an Artifact clicked in a conversation, in the Details Panel's place. The
    /// window's, not a session's, so picking another session keeps it.
    page: page_panel::State,
    /// The terminal panel: its tabs, whether it shows, and whether it is expanded.
    terminal: terminal::State,
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
        view.expand_all = switch(EXPAND_ENV);
        view.tasks_open_preview = switch(TASKS_OPEN_ENV);
        view.drop_preview = switch(DROP_ENV);
        view.latest_preview = switch(LATEST_ENV);
        view.actions_preview = switch(ACTIONS_ENV);
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
        if switch(ROW_MENU_ENV) {
            // The sessions load after this; the menu needs only an id.
            view.row_menu = Some(row_menu::RowMenu {
                row_id: "render-session".into(),
                at: gpui::point(px(120.0), px(110.0)),
            });
        }
        if let Ok(id) = std::env::var(PAGE_ENV) {
            view.page.preview(&id);
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

    /// The pointer entered or left message `id` (its row of actions shows by a group hover). The
    /// times it shows are as old as the last draw, so entering after a while redraws once.
    pub(super) fn message_hover(&mut self, id: &str, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            self.hovered_message = Some(id.to_owned());
            if message_meta::stale(self.drawn_ms, now_ms()) {
                cx.notify();
            }
        } else if self.hovered_message.as_deref() == Some(id) {
            self.hovered_message = None;
        }
    }

    /// Whether the group, step or thought `id` is drawn open.
    pub(super) fn is_open(&self, id: &str) -> bool {
        self.expand_all || self.expanded.contains(id)
    }

    /// Whether the Tasks bar is open.
    pub(super) fn tasks_open(&self) -> bool {
        self.tasks_open_preview || self.expanded.contains(tasks::OPEN_ID)
    }

    /// Opens or folds `id`.
    pub(super) fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(id) {
            self.expanded.insert(id.to_owned());
        }
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
            this.update(cx, |_, cx| cx.notify()).ok();
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
            selected: None,
            conversation: Conversation::default(),
            subagents: Vec::new(),
            now_ms: 0,
            load_again: false,
            loading: true,
            // The first conversation opens at its newest message, as a picked one does.
            scroll_to_end: true,
            gate: ReloadGate::default(),
            seen_generation: source.generation(),
            daemon_note: source.view().note(),
            source,
            watch,
            conversation_scroll: ScrollHandle::new(),
            expanded: HashSet::new(),
            expand_all: false,
            tasks_open_preview: false,
            drop_preview: false,
            latest_preview: false,
            actions_preview: false,
            copied: None,
            hovered_message: None,
            drawn_ms: 0,
            sidebar_hidden: false,
            details_hidden: false,
            page: page_panel::State::default(),
            terminal: terminal::State::default(),
            slash: Default::default(),
            slash_commands: None,
            slash_preview: None,
            follow_sent: false,
            clock_second: 0,
            overlay_open: false,
            page_covered: false,
            tools: tools::ChatTools::new(cx),
            sheet: None,
            bridge: phone::app_bridge(cx),
            phone: None,
            phone_dot: false,
            pending_session: None,
            closed: std::collections::HashMap::new(),
            used: std::collections::HashMap::new(),
            tasks_seen: std::collections::HashMap::new(),
            _appearance_changes: changes,
            _parts_escape: parts_escape,
        };
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

    /// The session the conversation shows.
    pub fn selected_session(&self) -> Option<&Session> {
        self.sessions
            .iter()
            .find(|s| Some(&s.agent.id) == self.selected.as_ref())
    }

    /// Shows a conversation the search panel found: selects its row when it is listed, else
    /// selects it as soon as a load lists it (`chat::sidebar::row_for`).
    pub fn show_session(&mut self, target: SessionRef, cx: &mut Context<Self>) {
        match row_for(&self.sessions, &target) {
            Some(id) => {
                self.pending_session = None;
                self.select(&id, cx);
            }
            None => {
                self.pending_session = Some(PendingSession::new(target, now_ms()));
                self.gate.changed();
                cx.notify();
            }
        }
    }

    /// Reads everything again on the background executor, for the session picked now. A load
    /// already running is not doubled: another follows it.
    fn start_load(&mut self, cx: &mut Context<Self>) {
        self.gate.reloaded(now_ms());
        let Some(mut loader) = self.loader.take() else {
            self.load_again = true;
            return;
        };
        let asked = self.selected.clone();
        let wanted = asked.clone();
        let view = self.source.view();
        let (claude, live) = (view.agents, view.tools);
        // The Swift hook's files are read only while the daemon's held prompts aren't followed.
        loader.files_prompts = view.prompts.is_none();
        cx.spawn(async move |this, cx| {
            let (loader, loaded) = cx
                .background_executor()
                .spawn(async move {
                    let loaded = loader.load_live(wanted.as_deref(), claude, &live, now_ms());
                    (loader, loaded)
                })
                .await;
            this.update(cx, |view, cx| view.apply(asked, loader, loaded, cx))
                .ok();
        })
        .detach();
    }

    /// Takes a finished load. `asked` is the session it was started for: if another was picked
    /// meanwhile, its sessions list still counts but its conversation does not.
    fn apply(
        &mut self,
        asked: Option<String>,
        loader: Loader,
        loaded: Loaded,
        cx: &mut Context<Self>,
    ) {
        self.loader = Some(loader);
        self.now_ms = loaded.now_ms;
        let all = loaded.sessions;
        self.prompt.files = loaded.requests;
        // The search panel's session, now that the list has it (closed or not), or never.
        if let Some(pending) = self.pending_session.as_mut() {
            match pending.after_load(&all, now_ms()) {
                Pending::Found(id) => {
                    self.pending_session = None;
                    self.reopen_row(&id);
                    self.sessions = self.open_rows(all);
                    self.select(&id, cx);
                    return;
                }
                Pending::Waiting => {}
                Pending::GaveUp => self.pending_session = None,
            }
        }
        self.sessions = self.open_rows(all);
        if self.selected == asked {
            let was = self.selected.take();
            self.selected = loaded.selected;
            // The loader picks from every row; a closed one gives way to the first one shown.
            if let Some(sel) = self.selected.clone() {
                if !self.sessions.iter().any(|s| s.agent.id == sel) {
                    self.selected = crate::chat::sidebar::sections(&self.sessions, self.now_ms)
                        .into_iter()
                        .flat_map(|s| s.rows)
                        .map(|r| r.id)
                        .next();
                    if self.selected.is_some() {
                        self.scroll_to_end = true;
                        self.reload_now(cx);
                    }
                }
            }
            if self.selected != was {
                self.show_draft(cx);
            }
            // An Antigravity row's conversation is the daemon's, fetched by `sync_tools`; the
            // files have none to put in its place.
            let agy = self.selected_session().map(|s| s.tool) == Some(Tool::Antigravity);
            let session_changed = std::mem::take(&mut self.scroll_to_end);
            if !agy {
                self.follow_content(session_changed);
                self.conversation = loaded.conversation;
                self.loading = false;
            } else if session_changed {
                self.conversation_scroll.scroll_to_bottom();
            }
            self.subagents = loaded.subagents;
            self.prompt.transcript = loaded.question;
        } else {
            self.load_again = true;
        }
        if std::mem::take(&mut self.load_again) {
            self.start_load(cx);
        }
        self.sync_tools(cx);
        cx.notify();
    }

    /// Before new content replaces the conversation: stay on the newest message when the view is
    /// at the bottom (or a message was just sent, or another session was picked), else leave it
    /// where it is (`chat::follow`). The numbers are those of the last frame, so a reply that
    /// grew since is measured by the next one, which the scroll handle then pins to the end.
    pub(super) fn follow_content(&mut self, session_changed: bool) {
        let handle = &self.conversation_scroll;
        let prev = Scroll {
            offset_y: f32::from(handle.offset().y),
            max_y: f32::from(handle.max_offset().height),
        };
        let sent = std::mem::take(&mut self.follow_sent);
        if follow::decide(prev, prev.max_y, sent, session_changed).scroll {
            handle.scroll_to_bottom();
        }
    }

    /// The live line for the selected session: what it is doing, or none while it is idle.
    pub(super) fn activity_line(&self) -> Option<Activity> {
        if self.loading {
            return None;
        }
        activity::line(
            &self.conversation,
            self.selected_working(),
            now_ms() as f64 / 1000.0,
        )
    }

    /// Whether the "Latest" pill shows: the conversation is scrolled up from its newest message.
    fn latest_pill(&self) -> bool {
        if self.loading {
            return false;
        }
        let handle = &self.conversation_scroll;
        self.latest_preview
            || !Scroll {
                offset_y: f32::from(handle.offset().y),
                max_y: f32::from(handle.max_offset().height),
            }
            .at_bottom()
    }

    /// The pill's click: to the newest message, and following again.
    fn jump_to_latest(&mut self, cx: &mut Context<Self>) {
        self.latest_preview = false;
        self.conversation_scroll.scroll_to_bottom();
        cx.notify();
    }

    /// Reads again now, past the gate (something this window did changed the files).
    pub(super) fn reload_now(&mut self, cx: &mut Context<Self>) {
        self.gate.changed();
        self.start_load(cx);
    }

    pub(super) fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.selected.as_deref() == Some(id) {
            return;
        }
        self.selected = Some(id.to_owned());
        self.mark_used(id);
        self.conversation = Conversation::default();
        self.subagents.clear();
        self.prompt.transcript = None;
        self.loading = true;
        self.scroll_to_end = true;
        self.tools.error = None;
        self.tools.picker = None;
        self.tools.agy = None;
        self.tools.agy_fetched = None;
        self.tools.agy_config.clear();
        self.show_draft(cx);
        self.start_load(cx);
        cx.notify();
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        // New rows or a lost daemon: look again (the gate still spaces the reads).
        let generation = self.source.generation();
        if generation != self.seen_generation {
            self.seen_generation = generation;
            self.gate.changed();
            let note = self.source.view().note();
            if note != self.daemon_note {
                self.daemon_note = note;
            }
            self.sync_tools(cx);
            cx.notify();
        }
        let dot = self.bridge_on();
        if dot != self.phone_dot {
            self.phone_dot = dot;
            cx.notify();
        }
        if let Some((_, batches)) = &self.watch {
            while batches.try_recv().is_ok() {
                self.gate.changed();
            }
        }
        // The live line's clock: a redraw each second while it shows, none while nothing runs.
        let clock = now_ms() / 1000;
        if clock != self.clock_second && self.activity_line().is_some() {
            self.clock_second = clock;
            cx.notify();
        }
        // The check on Copy goes after a moment; a hovered row's time is kept fresh.
        if let Some((_, at)) = &self.copied {
            if !message_meta::copied_shows(*at, now_ms()) {
                self.copied = None;
                cx.notify();
            }
        }
        if self.hovered_message.is_some() && message_meta::stale(self.drawn_ms, now_ms()) {
            cx.notify();
        }
        // A second opinion's card changes when its command ends, which nothing else signals.
        if opinions::any_running(self, cx) {
            cx.notify();
        }
        if self.gate.due(now_ms()) {
            self.start_load(cx);
        }
        // The header's meters and account menu: the accounts store, every few seconds.
        let now = now_ms();
        if now - self.accounts_polled_ms >= accounts::POLL.as_millis() as i64 {
            self.accounts_polled_ms = now;
            self.codex_installed = accounts::codex_installed(&self.root.home);
            self.poll_accounts(cx);
        }
    }
}

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        self.drawn_ms = now_ms();
        self.take_terminal_focus(window);
        if let Some(text) = self.slash_preview.clone() {
            if self.selected_session().is_some() && self.tools.editor.read(cx).text() != text {
                self.tools
                    .editor
                    .update(cx, |editor, cx| editor.set_text(&text, cx));
                self.slash_typed(&text);
            }
        }
        self.sync_prompt(cx);
        self.sync_tasks(cx);
        self.overlay_open = self.sheet.is_some()
            || self.account_menu
            || self.row_menu.is_some()
            || self.phone.is_some()
            || self.tools.picker.is_some()
            || self.composer.is_open()
            || parts_on(cx);
        self.page_covered =
            crate::chat::page_panel::page_covered(crate::chat::page_panel::Covers {
                sheet: self.sheet.is_some(),
                phone: self.phone.is_some(),
                parts: parts_on(cx),
            });
        // Files from Finder land anywhere on the middle column while the box can take them.
        let takes_drops = self.composer_control(cx).enabled;
        let conversation = div()
            .relative()
            .flex_1()
            .min_h(px(0.0))
            .flex()
            .flex_col()
            .child(
                scrollable(conversation::render(self, a, cx)).tracking(&self.conversation_scroll),
            )
            // Over the messages, so it stays while they scroll.
            .children(tasks::render(self, a, cx))
            .children(conversation::latest_pill(self, a, cx))
            .part_at("Conversation", Corner::Bottom, a, cx);
        let terminal_expanded =
            self.terminal.shown() && self.terminal.expanded && !self.page.is_open();
        let column = div()
            .id("chat-column")
            .group(drop::GROUP)
            .relative()
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .flex()
            .flex_col()
            .child(conversation)
            .child(composer::render(self, a, window, cx))
            .when(takes_drops, |column| {
                column.on_drop::<ExternalPaths>(cx.listener(
                    |this, paths: &ExternalPaths, w, cx| {
                        this.drop_paths(paths.paths(), w, cx);
                    },
                ))
            })
            // The hint only where a drop would land; a render over a fixture has no acting, so
            // its switch draws the hint anyway.
            .when(takes_drops || self.drop_preview, |column| {
                column.child(drop::hint(self.drop_preview, a))
            });
        div()
            .relative()
            .size_full()
            .flex()
            .fill(Ink::Window, a)
            // A click in the page panel gives its web view the keyboard; a click anywhere GPUI
            // draws takes it back, or typing would go on reaching the page.
            .capture_any_mouse_down(cx.listener(|this, _, _, _| this.page.take_keys_back()))
            // The command list's keys, before the reply box moves its caret on Up and Down.
            .capture_key_down(
                cx.listener(|this, event, window, cx| this.capture_slash_key(event, window, cx)),
            )
            .children((!self.sidebar_hidden).then(|| sidebar::render(self, a, cx)))
            // The header runs across the conversation and the Details Panel, so both start under it.
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(header::render(self, a, cx))
                    // A page opened from a link takes the Details Panel's place.
                    .child(page_panel::follows_edge(
                        div()
                            .flex_1()
                            .min_h(px(0.0))
                            .flex()
                            // An expanded terminal takes the conversation's room too.
                            .when(!terminal_expanded, |row| row.child(column))
                            .children(page_panel::render(self, a, window, cx))
                            .children(terminal::render(self, a, window, cx))
                            .children(
                                (!self.page.is_open()
                                    && !self.terminal.shown()
                                    && !self.details_hidden)
                                    .then(|| inspector::render(self, a, cx).into_any_element()),
                            ),
                        cx,
                    )),
            )
            // Beside the traffic lights, over the sidebar's top or the header's start.
            .child(sidebar::toggle_button(self, a, cx))
            .child(sidebar::new_session_button(self, a, cx))
            .children(accounts::render_menu(self, a, cx))
            .children(row_menu::render(self, a, cx))
            .children(sheets::render(self, a))
            .children(phone::render(self.phone.as_ref(), a, cx))
            .child(resize_edges(window, TopBorder::InContent))
    }
}
