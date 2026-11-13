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
use notchling_kit::render_map::Region;
use notchling_kit::ui::{drop_spinner_host, host_spinners, window_appearance, SpinSlots};

use super::conversation_list::{at_bottom, scroll_to_end, ListRows, Pending, OVERDRAW};
use super::pane::RENDERS;
use super::row_select::{self, SelState};
use super::{conversation, message_meta, now_ms, ChatView};
use crate::acting::Acting;
use crate::chat::data::Tool;
use crate::chat::log::Change;
use crate::chat::markdown_cache;

pub(super) struct ConversationState {
    pub list: ListState,
    pub markdown: RefCell<markdown_cache::Cache>,
    pub expanded: HashSet<String>,
    pub expand_all: bool,
    pub actions_preview: bool,
    pub run_preview: bool,
    pub latest_preview: bool,
    pub copied: Option<(String, i64)>,
    pub hovered_message: Option<String>,
    pub drawn_ms: i64,
    pub follow_sent: bool,
    pub scroll_to_end: bool,
    pub follow_next: bool,
    pub opinions_running: bool,
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
            run_preview: false,
            latest_preview: false,
            copied: None,
            hovered_message: None,
            drawn_ms: 0,
            follow_sent: false,
            scroll_to_end: true,
            follow_next: false,
            opinions_running: false,
            sel: Rc::default(),
        }
    }

    pub fn is_open(&self, id: &str) -> bool {
        self.expand_all || self.expanded.contains(id)
    }

    pub fn toggle(&mut self, id: &str) {
        if !self.expanded.remove(id) {
            self.expanded.insert(id.to_owned());
        }
    }

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

    pub fn follow_content(&mut self, session_changed: bool) {
        let sent = std::mem::take(&mut self.follow_sent);
        if sent || session_changed {
            self.follow_next = true;
        }
    }

    pub fn at_bottom(&self) -> bool {
        at_bottom(&self.list)
    }

    pub fn latest_pill(&self, loading: bool) -> bool {
        if loading {
            return false;
        }
        self.latest_preview || !self.at_bottom()
    }

    pub fn jump_to_latest(&mut self) {
        self.latest_preview = false;
        self.scroll_to_end_now();
    }

    pub fn scroll_to_end_now(&mut self) {
        scroll_to_end(&self.list);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct SubagentShown {
    pub agent_id: String,
    pub running: bool,
    pub loaded: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct SessionShown {
    pub id: String,
    pub tool: Tool,
    pub cwd: String,
    pub working: bool,
}

#[derive(Clone)]
pub(super) struct ConversationInput {
    pub shown: Arc<Conversation>,
    pub subagent: Option<SubagentShown>,
    pub session: Option<SessionShown>,
    pub held_working: bool,
    pub held: Option<String>,
    pub outgoing: Vec<String>,
    pub loading: bool,
    pub empty_note: Option<&'static str>,
    pub subagents: Vec<(Option<String>, String)>,
    pub tasks_inset: bool,
    pub selected: Option<String>,
    pub can_run: bool,
    pub chat: WeakEntity<ChatView>,
}

impl PartialEq for ConversationInput {
    fn eq(&self, other: &Self) -> bool {
        (Arc::ptr_eq(&self.shown, &other.shown) || *self.shown == *other.shown)
            && self.subagent == other.subagent
            && self.session == other.session
            && self.held_working == other.held_working
            && self.held == other.held
            && self.outgoing == other.outgoing
            && self.loading == other.loading
            && self.empty_note == other.empty_note
            && self.subagents == other.subagents
            && self.tasks_inset == other.tasks_inset
            && self.selected == other.selected
            && self.can_run == other.can_run
    }
}

pub struct ConversationView {
    chat: WeakEntity<ChatView>,
    pub(super) s: ConversationState,
    pub(super) input: Option<ConversationInput>,
    pub(super) rows: ListRows,
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

    fn input_from(&self, chat: &ChatView, cx: &App) -> ConversationInput {
        let mut input = ConversationInput {
            chat: self.chat.clone(),
            ..chat.conversation_input(cx)
        };
        input.can_run |= self.s.run_preview;
        input
    }

    fn set_input(&mut self, input: ConversationInput, cx: &mut Context<Self>) {
        if self.input.as_ref() != Some(&input) {
            self.input = Some(input);
            cx.notify();
        }
    }

    pub(super) fn take_change(
        &mut self,
        change: Option<(Arc<Conversation>, Change)>,
        to: &Arc<Conversation>,
    ) {
        self.change = change.map(|(from, change)| (from, to.clone(), change));
    }

    pub(super) fn act(
        &self,
        f: impl FnOnce(&mut ChatView, &mut Context<ChatView>),
        cx: &mut Context<Self>,
    ) {
        self.chat.update(cx, f).ok();
    }

    pub fn copied(&self, _cx: &App) -> Option<String> {
        let selected = {
            let frame = self.s.sel.borrow();
            frame.selection.anchor.is_some() && frame.selection.focus.is_some()
        };
        let input = self.input.as_ref().filter(|_| selected)?;
        let blocks = super::row_blocks::input_blocks(input, &self.s);
        super::row_select::copied(&self.s.sel.borrow().selection, &blocks)
    }

    pub(super) fn right_click(
        &self,
        at: gpui::Point<gpui::Pixels>,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        self.act(move |chat, cx| chat.open_copy_menu(at, enabled, cx), cx);
    }

    pub(super) fn copy_menu_open(&self, cx: &App) -> bool {
        self.chat
            .upgrade()
            .is_some_and(|chat| chat.read(cx).copy_menu.is_some())
    }

    pub fn clear_selection(&mut self) {
        self.s.sel.borrow_mut().selection.clear();
    }

    pub(super) fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        self.s.toggle(id);
        cx.notify();
    }

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
        let _region = super::render_map::scope(Region::Conversation, cx);
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
        self.s.drawn_ms = now_ms();
        let change = self.change.take();
        let synced = self.rows.sync(&input, change.as_ref(), self.s.follow_next);
        if synced.replaced {
            self.s.follow_next = false;
        }
        if synced.keys {
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

impl ChatView {
    pub(super) fn conversation_state<R>(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut ConversationState) -> R,
    ) -> R {
        self.parts.conversation.update(cx, |v, _| f(&mut v.s))
    }

    pub(super) fn redraw_conversation(&self, cx: &mut Context<Self>) {
        self.parts.conversation.update(cx, |_, cx| cx.notify());
    }

    pub(super) fn refresh_conversation(&self, cx: &mut Context<Self>) {
        let input = ConversationInput {
            chat: cx.entity().downgrade(),
            ..self.conversation_input(cx)
        };
        self.parts
            .conversation
            .update(cx, |v, pcx| v.set_input(input, pcx));
    }

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
        let held = if self.open_subagent.is_some() {
            None
        } else {
            self.held_text()
        };
        let empty = shown.items.is_empty()
            && shown.queued.is_empty()
            && held.is_none()
            && outgoing.is_empty();
        let empty_note = empty.then(|| match session.map(|s| s.tool) {
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
            held,
            outgoing,
            loading: self.loading,
            empty_note,
            subagents: crate::chat::subagent_view::links(&self.subagents),
            tasks_inset: super::tasks::shown(self),
            selected: self.selected.clone(),
            can_run: self.run_available(cx),
            chat: WeakEntity::new_invalid(),
        }
    }

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

    pub(super) fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        self.parts.conversation.update(cx, |v, cx| v.toggle(id, cx));
        cx.notify();
    }

    pub(super) fn follow_content(&mut self, session_changed: bool, cx: &mut Context<Self>) {
        self.conversation_state(cx, |s| s.follow_content(session_changed));
    }

    pub(super) fn follow_sent_now(&mut self, cx: &mut Context<Self>) {
        self.conversation_state(cx, |s| {
            s.follow_sent = true;
            s.scroll_to_end_now();
        });
    }

    pub(super) fn scroll_to_end_now(&mut self, cx: &mut Context<Self>) {
        self.conversation_state(cx, ConversationState::scroll_to_end_now);
    }

    pub(super) fn open_at_end(&mut self, cx: &mut Context<Self>) {
        self.conversation_state(cx, |s| s.scroll_to_end = true);
    }

    pub(super) fn take_open_at_end(&mut self, cx: &mut Context<Self>) -> bool {
        self.conversation_state(cx, |s| std::mem::take(&mut s.scroll_to_end))
    }
}

#[cfg(test)]
#[path = "conversation_state_tests.rs"]
mod tests;
