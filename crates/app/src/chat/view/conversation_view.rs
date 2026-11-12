//! The conversation's column as a view of its own: its scroll, which groups and thoughts are open,
//! the hovered message and Copy's check, the Markdown cache. It is drawn cached apart from the rest
//! of the window (see `pane`) from a [`ConversationInput`] (what the column shows, taken from the
//! chat on each of its notifies) and its own state, and draws only when one of the two changed: a
//! chat change nothing here shows (a draft, a menu) compares equal and draws nothing. The rows are
//! a GPUI list (`conversation_list`): a draw builds only the rows on screen.
//!
//! What the draw reads is the input and the state, never `ChatView`: it acts on the session
//! through a weak handle to it (`ConversationInput::chat`).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use gpui::{
    div, prelude::*, App, Context, Entity, IntoElement, ListAlignment, ListState, Subscription,
    WeakEntity, Window,
};
use notchling_engine::transcript::model::Conversation;
use notchling_kit::ui::{drop_spinner_host, host_spinners, window_appearance, SpinSlots};

use super::conversation_list::{at_bottom, scroll_to_end, ListRows, Pending, OVERDRAW};
use super::pane::RENDERS;
use super::row_select::{self, SelState};
use super::{conversation, message_meta, now_ms, ChatView};
use crate::acting::Acting;
use crate::chat::data::Tool;
use crate::chat::log::Change;
use crate::chat::markdown_cache;

/// What the conversation keeps between draws.
pub(super) struct ConversationState {
    /// The rows' list: its scroll position is the column's.
    pub list: ListState,
    /// Each drawn message's Markdown, parsed once and its paths looked up once per load.
    pub markdown: RefCell<markdown_cache::Cache>,
    /// The step groups, steps and thoughts clicked open, by item or call id (`ChatExpansion` in
    /// the Swift app). Kept outside the rows so a reload doesn't fold them.
    pub expanded: HashSet<String>,
    /// Every group, step and thought drawn open: a render over a fixture
    /// (`NOTCHLING_RENDER_EXPAND=1`), since nothing clicks in a picture.
    pub expand_all: bool,
    /// Every message's row of actions drawn, as if hovered (`NOTCHLING_RENDER_ACTIONS=1`).
    pub actions_preview: bool,
    /// The "Latest" pill drawn as if scrolled up (`NOTCHLING_RENDER_LATEST=1`).
    pub latest_preview: bool,
    /// The message whose Copy was clicked and when (ms), for its check.
    pub copied: Option<(String, i64)>,
    /// The message the pointer is over. Set without a redraw: the row shows by a group hover, and
    /// this only says whether its time needs keeping fresh.
    pub hovered_message: Option<String>,
    /// When the conversation was last drawn (ms), so a hovered row's relative time is redrawn
    /// once it is stale.
    pub drawn_ms: i64,
    /// A message was sent and hasn't shown up in a reload yet: that reload goes to the end.
    pub follow_sent: bool,
    /// Scroll to the end once the picked session's conversation is in.
    pub scroll_to_end: bool,
    /// The next conversation that comes in goes to the end whatever was being read: a message
    /// was sent, or another session picked (`follow_content` sets it, the list takes it).
    pub follow_next: bool,
    /// Whether a second opinion in the selected session was running at the last tick, so the
    /// tick that sees it end draws its card too.
    pub opinions_running: bool,
    /// The selection, and the texts the last draw noted for the pointer (`row_select`).
    pub sel: SelState,
}

impl ConversationState {
    pub fn new() -> ConversationState {
        ConversationState {
            list: ListState::new(0, ListAlignment::Bottom, gpui::px(OVERDRAW)),
            markdown: RefCell::default(),
            expanded: HashSet::new(),
            expand_all: false,
            actions_preview: false,
            latest_preview: false,
            copied: None,
            hovered_message: None,
            drawn_ms: 0,
            follow_sent: false,
            // The first conversation opens at its newest message, as a picked one does.
            scroll_to_end: true,
            follow_next: false,
            opinions_running: false,
            sel: Rc::default(),
        }
    }

    /// Whether the group, step or thought `id` is drawn open.
    pub fn is_open(&self, id: &str) -> bool {
        self.expand_all || self.expanded.contains(id)
    }

    /// Opens or folds `id`.
    pub fn toggle(&mut self, id: &str) {
        if !self.expanded.remove(id) {
            self.expanded.insert(id.to_owned());
        }
    }

    /// The pointer entered or left message `id` (its row of actions shows by a group hover). The
    /// times it shows are as old as the last draw, so entering after a while asks for a redraw
    /// (`true`).
    pub fn hover(&mut self, id: &str, hovered: bool) -> bool {
        if hovered {
            self.hovered_message = Some(id.to_owned());
            message_meta::stale(self.drawn_ms, now_ms())
        } else {
            if self.hovered_message.as_deref() == Some(id) {
                self.hovered_message = None;
            }
            false
        }
    }

    /// Before new content replaces the conversation: stay on the newest message when the view is
    /// at the bottom (or a message was just sent, or another session was picked), else leave it
    /// where it is (`chat::follow`). Whether the view is at the bottom is read when the list takes
    /// the content (`ListRows::sync`), before it changes anything; this says whether to go to the
    /// end whatever the view was at.
    pub fn follow_content(&mut self, session_changed: bool) {
        let sent = std::mem::take(&mut self.follow_sent);
        if sent || session_changed {
            self.follow_next = true;
        }
    }

    /// Whether the conversation is at its newest message (or has nothing below it).
    pub fn at_bottom(&self) -> bool {
        at_bottom(&self.list)
    }

    /// Whether the "Latest" pill shows: the conversation is scrolled up from its newest message.
    pub fn latest_pill(&self, loading: bool) -> bool {
        if loading {
            return false;
        }
        self.latest_preview || !self.at_bottom()
    }

    /// The pill's click: to the newest message, and following again.
    pub fn jump_to_latest(&mut self) {
        self.latest_preview = false;
        self.scroll_to_end_now();
    }

    /// To the newest message now.
    pub fn scroll_to_end_now(&mut self) {
        scroll_to_end(&self.list);
    }
}

/// The subagent shown in place of the session's conversation.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SubagentShown {
    pub agent_id: String,
    pub running: bool,
    /// Its transcript has been read; until then the column says "Loading…".
    pub loaded: bool,
}

/// The selected session, as far as the column draws it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SessionShown {
    pub id: String,
    pub tool: Tool,
    pub cwd: String,
    /// Its row says it is working.
    pub working: bool,
}

/// Everything the column draws that isn't its own state, from the chat.
#[derive(Clone)]
pub(super) struct ConversationInput {
    /// What the column shows: the open subagent's conversation or the session's.
    pub shown: Arc<Conversation>,
    pub subagent: Option<SubagentShown>,
    pub session: Option<SessionShown>,
    /// Another `agy` holds the Antigravity session's turn.
    pub held_working: bool,
    /// What was typed to the session and isn't in the transcript yet; none while a subagent shows.
    pub outgoing: Vec<String>,
    pub loading: bool,
    /// What an empty column says.
    pub empty_note: Option<&'static str>,
    /// Which subagent each Agent call opens (`subagent_view::links`): all of the list the column
    /// shows, so a running subagent's file growing draws nothing here.
    pub subagents: Vec<(Option<String>, String)>,
    /// The Tasks bar sits over the top, so the first row starts under it.
    pub tasks_inset: bool,
    /// The row id: the second opinions' key.
    pub selected: Option<String>,
    /// The chat, for what a click does to the session (not part of equality; the view fills it in).
    pub chat: WeakEntity<ChatView>,
}

impl PartialEq for ConversationInput {
    fn eq(&self, other: &Self) -> bool {
        (Arc::ptr_eq(&self.shown, &other.shown) || *self.shown == *other.shown)
            && self.subagent == other.subagent
            && self.session == other.session
            && self.held_working == other.held_working
            && self.outgoing == other.outgoing
            && self.loading == other.loading
            && self.empty_note == other.empty_note
            && self.subagents == other.subagents
            && self.tasks_inset == other.tasks_inset
            && self.selected == other.selected
    }
}

pub struct ConversationView {
    chat: WeakEntity<ChatView>,
    pub(super) s: ConversationState,
    /// The input drawn last; `None` asks for a fresh one at the next draw.
    pub(super) input: Option<ConversationInput>,
    /// The list's items, kept in step with the conversation on show.
    pub(super) rows: ListRows,
    /// What the last load changed: the conversation it started from, the one it made, and the
    /// difference. The list splices from it when it is the step from what the list holds.
    change: Option<Pending>,
    pub(super) spins: SpinSlots,
    _changes: Subscription,
    _release: Subscription,
}

impl ConversationView {
    pub fn new(chat: &Entity<ChatView>, cx: &mut Context<Self>) -> Self {
        let spins = SpinSlots::default();
        let id = cx.entity_id();
        host_spinners(cx, id, &spins);
        let s = ConversationState::new();
        ConversationView {
            chat: chat.downgrade(),
            rows: ListRows::new(s.list.clone()),
            s,
            input: None,
            change: None,
            spins,
            _changes: cx.observe(chat, |this, chat, cx| {
                let app: &App = cx;
                let input = this.input_from(chat.read(app), app);
                this.set_input(input, cx);
            }),
            _release: cx.on_release(move |_, cx| drop_spinner_host(cx, id)),
        }
    }

    /// The input from the chat now, with the handle this view acts through.
    fn input_from(&self, chat: &ChatView, cx: &App) -> ConversationInput {
        ConversationInput {
            chat: self.chat.clone(),
            ..chat.conversation_input(cx)
        }
    }

    /// Takes the input and draws only if it differs from the one drawn.
    fn set_input(&mut self, input: ConversationInput, cx: &mut Context<Self>) {
        if self.input.as_ref() != Some(&input) {
            self.input = Some(input);
            cx.notify();
        }
    }

    /// The change the last load found, from the conversation it started from to `to`, for the
    /// list to splice.
    pub(super) fn take_change(
        &mut self,
        change: Option<(Arc<Conversation>, Change)>,
        to: &Arc<Conversation>,
    ) {
        self.change = change.map(|(from, change)| (from, to.clone(), change));
    }

    /// Does something to the chat from a click in the column.
    pub(super) fn act(
        &self,
        f: impl FnOnce(&mut ChatView, &mut Context<ChatView>),
        cx: &mut Context<Self>,
    ) {
        self.chat.update(cx, f).ok();
    }

    /// The selected text, for ⌘C, read from the rows' blocks (so rows off screen count too);
    /// `None` when nothing is selected or the column hasn't drawn yet.
    pub fn copied(&self, _cx: &App) -> Option<String> {
        let selected = {
            let frame = self.s.sel.borrow();
            frame.selection.anchor.is_some() && frame.selection.focus.is_some()
        };
        let input = self.input.as_ref().filter(|_| selected)?;
        let blocks = super::row_blocks::input_blocks(input, &self.s);
        super::row_select::copied(&self.s.sel.borrow().selection, &blocks)
    }

    /// A right click on text at `at`: the chat opens its Copy menu there (`enabled`: something is
    /// selected). Called from the window's mouse listener, so the chat isn't being updated.
    pub(super) fn right_click(
        &self,
        at: gpui::Point<gpui::Pixels>,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        self.act(move |chat, cx| chat.open_copy_menu(at, enabled, cx), cx);
    }

    /// The Copy menu is open: a press on it must not let go of what it copies.
    pub(super) fn copy_menu_open(&self, cx: &App) -> bool {
        self.chat
            .upgrade()
            .is_some_and(|chat| chat.read(cx).copy_menu.is_some())
    }

    /// Lets go of the selection (another session opened).
    pub fn clear_selection(&mut self) {
        self.s.sel.borrow_mut().selection.clear();
    }

    /// Opens or folds `id`, and draws.
    pub(super) fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        self.s.toggle(id);
        cx.notify();
    }

    /// The pointer entered or left message `id`; draws when its times are stale.
    pub(super) fn hover(&mut self, id: &str, hovered: bool, cx: &mut Context<Self>) {
        if self.s.hover(id, hovered) {
            cx.notify();
        }
    }
}

impl Render for ConversationView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        RENDERS.fetch_add(1, Ordering::Relaxed);
        self.spins.clear();
        let a = window_appearance(window);
        let input = match self.input.take() {
            Some(input) => input,
            None => {
                let Some(chat) = self.chat.upgrade() else {
                    return div().into_any_element();
                };
                let app: &App = cx;
                self.input_from(chat.read(app), app)
            }
        };
        // A hovered message's relative time is kept fresh from when it was last drawn.
        self.s.drawn_ms = now_ms();
        // The list takes what the loads changed before it lays out: only the rows they touched
        // are built again.
        let change = self.change.take();
        let synced = self.rows.sync(&input, change.as_ref(), self.s.follow_next);
        if synced.replaced {
            self.s.follow_next = false;
        }
        if synced.keys {
            // Where each row is, for the selection's ends; one whose row went is dropped.
            let mut frame = self.s.sel.borrow_mut();
            frame.rows = self.rows.places();
            frame.prune();
        }
        self.input = Some(input);
        let Some(input) = self.input.as_ref() else {
            return div().into_any_element();
        };
        let s = &self.s;
        let body = conversation::empty_column(input, s, a, cx);
        let pill = conversation::latest_pill(s, input.loading, a, cx);
        let list = s.list.clone();
        let column = match body {
            Some(column) => column.into_any_element(),
            None => div()
                .size_full()
                .relative()
                .flex()
                .flex_col()
                // First, so each draw's texts are noted afresh before the rows place theirs.
                .child(row_select::listener(s.sel.clone(), cx.entity().downgrade()))
                .child(self.list_column(cx))
                .when(super::scroll_probe::enabled(), |d| {
                    d.child(super::scroll_probe::frame_canvas(list, || {
                        (
                            super::pane::ROWS.load(Ordering::Relaxed),
                            super::pane::LIST_CHANGES.load(Ordering::Relaxed),
                        )
                    }))
                })
                .into_any_element(),
        };
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .child(column)
            .children(pill)
            .into_any_element()
    }
}

/// `ChatView`'s side: the conversation's state is read and written through its entity, and its
/// input is made here.
impl ChatView {
    pub(super) fn conversation_state<R>(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut ConversationState) -> R,
    ) -> R {
        self.parts.conversation.update(cx, |v, _| f(&mut v.s))
    }

    /// Draws the conversation again whatever its input says (a clock, a Copy check).
    pub(super) fn redraw_conversation(&self, cx: &mut Context<Self>) {
        self.parts.conversation.update(cx, |_, cx| cx.notify());
    }

    /// Draws the conversation again if its input changed.
    pub(super) fn refresh_conversation(&self, cx: &mut Context<Self>) {
        let input = ConversationInput {
            chat: cx.entity().downgrade(),
            ..self.conversation_input(cx)
        };
        self.parts
            .conversation
            .update(cx, |v, pcx| v.set_input(input, pcx));
    }

    /// What the column draws from the chat now.
    pub(super) fn conversation_input(&self, cx: &App) -> ConversationInput {
        let session = self.selected_session();
        let subagent = self.open_subagent.as_ref().map(|o| SubagentShown {
            agent_id: o.agent.id.clone(),
            running: o.agent.is_running(),
            loaded: o.conversation.is_some(),
        });
        let shown = self
            .open_subagent
            .as_ref()
            .and_then(|o| o.conversation.clone())
            .unwrap_or_else(|| self.conversation.clone());
        let outgoing = if self.open_subagent.is_some() {
            Vec::new()
        } else {
            self.outgoing_texts()
        };
        let empty = shown.items.is_empty() && shown.queued.is_empty() && outgoing.is_empty();
        let empty_note = empty.then(|| match session.map(|s| s.tool) {
            // Loaded through the daemon, which holds the session open only while acting is on.
            Some(Tool::Antigravity) if !self.tools.agy.as_ref().is_some_and(|c| c.open) => {
                match self.acting_in(cx) {
                    Acting::On => "Loading from Antigravity…",
                    _ => "Antigravity sessions load through the daemon while acting is on.",
                }
            }
            _ if self.open_subagent.is_some() => "No transcript yet for this subagent",
            _ => "No transcript yet for this session",
        });
        ConversationInput {
            shown,
            subagent,
            session: session.map(|s| SessionShown {
                id: s.agent.id.clone(),
                tool: s.tool,
                cwd: s.agent.cwd.clone(),
                working: s.agent.group == notchling_engine::sessions::agent::AgentGroup::Working,
            }),
            held_working: self.selected_held_working(),
            outgoing,
            loading: self.loading,
            empty_note,
            subagents: crate::chat::subagent_view::links(&self.subagents),
            tasks_inset: super::tasks::shown(self),
            selected: self.selected.clone(),
            chat: WeakEntity::new_invalid(),
        }
    }

    /// The Tasks bar is open.
    pub(super) fn tasks_open(&self, cx: &App) -> bool {
        self.tasks_open_preview
            || self
                .parts
                .conversation
                .read(cx)
                .s
                .expanded
                .contains(super::tasks::OPEN_ID)
    }

    /// Opens or folds `id` (the Tasks bar's own key too, so the window's root draws as well).
    pub(super) fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        self.parts.conversation.update(cx, |v, cx| v.toggle(id, cx));
        cx.notify();
    }

    /// Before new content replaces the conversation (see `ConversationState::follow_content`).
    pub(super) fn follow_content(&mut self, session_changed: bool, cx: &mut Context<Self>) {
        self.conversation_state(cx, |s| s.follow_content(session_changed));
    }

    /// A message was sent: the next reload, and now, go to the end.
    pub(super) fn follow_sent_now(&mut self, cx: &mut Context<Self>) {
        self.conversation_state(cx, |s| {
            s.follow_sent = true;
            s.scroll_to_end_now();
        });
    }

    /// To the newest message now.
    pub(super) fn scroll_to_end_now(&mut self, cx: &mut Context<Self>) {
        self.conversation_state(cx, ConversationState::scroll_to_end_now);
    }

    /// The picked session's conversation should open at its end once it is in.
    pub(super) fn open_at_end(&mut self, cx: &mut Context<Self>) {
        self.conversation_state(cx, |s| s.scroll_to_end = true);
    }

    /// Whether the conversation was to open at its end (read once: it is asked for again by the
    /// next pick).
    pub(super) fn take_open_at_end(&mut self, cx: &mut Context<Self>) -> bool {
        self.conversation_state(cx, |s| std::mem::take(&mut s.scroll_to_end))
    }
}

#[cfg(test)]
#[path = "conversation_state_tests.rs"]
mod tests;
