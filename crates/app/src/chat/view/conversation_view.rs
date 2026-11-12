//! The conversation's column as a view of its own: its scroll, which groups and thoughts are open,
//! the hovered message and Copy's check, the Markdown cache. It is drawn cached apart from the rest
//! of the window (see `pane`), reads the chat only while it draws, and still draws whenever
//! `ChatView` notifies (Task 4.4 makes it draw only when its input changed).
//!
//! The state lives here, not on `ChatView`, so a draw function gets it as an argument and never
//! reads this entity from inside the chat (this view is being updated while it draws).

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::atomic::Ordering;

use gpui::{
    div, prelude::*, App, Context, Entity, IntoElement, ScrollHandle, Subscription, WeakEntity,
    Window,
};
use notchling_kit::ui::{
    drop_spinner_host, host_spinners, scrollable, window_appearance, SpinSlots,
};

use super::pane::RENDERS;
use super::{conversation, message_meta, now_ms, ChatView};
use crate::chat::follow::{self, Scroll};
use crate::chat::markdown_cache;

/// What the conversation keeps between draws.
pub(super) struct ConversationState {
    pub scroll: ScrollHandle,
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
}

impl ConversationState {
    pub fn new() -> ConversationState {
        ConversationState {
            scroll: ScrollHandle::new(),
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
    /// where it is (`chat::follow`). The numbers are those of the last frame, so a reply that
    /// grew since is measured by the next one, which the scroll handle then pins to the end.
    pub fn follow_content(&mut self, session_changed: bool) {
        let handle = &self.scroll;
        let prev = Scroll {
            offset_y: f32::from(handle.offset().y),
            max_y: f32::from(handle.max_offset().height),
        };
        let sent = std::mem::take(&mut self.follow_sent);
        if follow::decide(prev, prev.max_y, sent, session_changed).scroll {
            handle.scroll_to_bottom();
        }
    }

    /// Whether the "Latest" pill shows: the conversation is scrolled up from its newest message.
    pub fn latest_pill(&self, loading: bool) -> bool {
        if loading {
            return false;
        }
        let handle = &self.scroll;
        self.latest_preview
            || !Scroll {
                offset_y: f32::from(handle.offset().y),
                max_y: f32::from(handle.max_offset().height),
            }
            .at_bottom()
    }

    /// The pill's click: to the newest message, and following again.
    pub fn jump_to_latest(&mut self) {
        self.latest_preview = false;
        self.scroll.scroll_to_bottom();
    }

    /// To the newest message now.
    pub fn scroll_to_end_now(&mut self) {
        self.scroll.scroll_to_bottom();
    }
}

pub struct ConversationView {
    chat: WeakEntity<ChatView>,
    pub(super) s: ConversationState,
    pub(super) spins: SpinSlots,
    _changes: gpui::Subscription,
    _release: Subscription,
}

impl ConversationView {
    pub fn new(chat: &Entity<ChatView>, cx: &mut Context<Self>) -> Self {
        let spins = SpinSlots::default();
        let id = cx.entity_id();
        host_spinners(cx, id, &spins);
        ConversationView {
            chat: chat.downgrade(),
            s: ConversationState::new(),
            spins,
            _changes: cx.observe(chat, |_, _, cx| cx.notify()),
            _release: cx.on_release(move |_, cx| drop_spinner_host(cx, id)),
        }
    }
}

impl Render for ConversationView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        RENDERS.fetch_add(1, Ordering::Relaxed);
        self.spins.clear();
        let a = window_appearance(window);
        let Some(chat) = self.chat.upgrade() else {
            return div().into_any_element();
        };
        // A hovered message's relative time is kept fresh from when it was last drawn.
        self.s.drawn_ms = now_ms();
        let s = &self.s;
        chat.update(cx, |chat, ccx| {
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(scrollable(conversation::render(chat, s, a, ccx)).tracking(&s.scroll))
                .into_any_element()
        })
    }
}

/// `ChatView`'s side: the conversation's state is read and written through its entity.
impl ChatView {
    pub(super) fn conversation_state<R>(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut ConversationState) -> R,
    ) -> R {
        self.parts.conversation.update(cx, |v, _| f(&mut v.s))
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

    /// Opens or folds `id`.
    pub(super) fn toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        self.conversation_state(cx, |s| s.toggle(id));
        cx.notify();
    }

    /// The pointer entered or left message `id`; a redraw when its times are stale.
    pub(super) fn message_hover(&mut self, id: &str, hovered: bool, cx: &mut Context<Self>) {
        if self.conversation_state(cx, |s| s.hover(id, hovered)) {
            cx.notify();
        }
    }

    /// Before new content replaces the conversation (see `ConversationState::follow_content`).
    pub(super) fn follow_content(&mut self, session_changed: bool, cx: &mut Context<Self>) {
        self.conversation_state(cx, |s| s.follow_content(session_changed));
    }

    /// The "Latest" pill's click.
    pub(super) fn jump_to_latest(&mut self, cx: &mut Context<Self>) {
        self.conversation_state(cx, ConversationState::jump_to_latest);
        cx.notify();
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
