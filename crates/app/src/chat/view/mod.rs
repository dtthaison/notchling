//! The Chat window's GPUI views (macOS): sidebar, header, conversation, disabled reply box,
//! inspector. Read-only: picking a session is the only thing a click does. The view reloads when
//! `platform::watch` reports a change under the data root, and every 30 s for the ages.

mod conversation;
mod header;
mod inspector;
mod sidebar;

use std::sync::mpsc::Receiver;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{div, prelude::*, px, Context, Div, Render, ScrollHandle, Subscription, Window};
use notchling_engine::transcript::model::Conversation;
use notchling_engine::transcript::subagents::Subagent;
use notchling_kit::metric::{RADIUS_COMPOSER, SEND_DISC, SIDE_PADDING};
use notchling_kit::ui::{hsla, window_appearance, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_platform::watch::{Batch, WatchConfig, Watcher};

use crate::chat::data::{DataRoot, Session};

/// How often the view looks for watch batches.
const TICK: Duration = Duration::from_millis(500);
/// Ages and the Done window move on their own: a full reload every this many ticks (30 s).
const RELOAD_EVERY: u64 = 60;

pub struct ChatView {
    root: DataRoot,
    sessions: Vec<Session>,
    selected: Option<String>,
    conversation: Conversation,
    subagents: Vec<Subagent>,
    now_ms: i64,
    ticks: u64,
    watch: Option<(Watcher, Receiver<Batch>)>,
    conversation_scroll: ScrollHandle,
    _appearance_changes: Subscription,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl ChatView {
    /// Lists the root's sessions, selects the first and starts watching for changes.
    pub fn new(root: DataRoot, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let this = cx.weak_entity();
        let changes = window.observe_window_appearance(move |_, cx| {
            this.update(cx, |_, cx| cx.notify()).ok();
        });
        let watch = Watcher::start(
            root.watch_folders(),
            WatchConfig {
                poll: Duration::from_secs(1),
                debounce: Duration::from_millis(500),
                ..WatchConfig::default()
            },
        )
        .ok();
        let mut view = ChatView {
            root,
            sessions: Vec::new(),
            selected: None,
            conversation: Conversation::default(),
            subagents: Vec::new(),
            now_ms: 0,
            ticks: 0,
            watch,
            conversation_scroll: ScrollHandle::new(),
            _appearance_changes: changes,
        };
        view.reload();
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

    fn reload(&mut self) {
        self.now_ms = now_ms();
        self.sessions = self.root.sessions(self.now_ms);
        let known = self
            .selected
            .as_ref()
            .is_some_and(|id| self.sessions.iter().any(|s| &s.agent.id == id));
        if !known {
            self.selected = self.sessions.first().map(|s| s.agent.id.clone());
        }
        self.load_conversation();
    }

    fn load_conversation(&mut self) {
        let Some(session) = self.selected_session().cloned() else {
            self.conversation = Conversation::default();
            self.subagents.clear();
            return;
        };
        self.conversation = self.root.conversation(&session);
        self.subagents =
            self.root
                .subagents(&session, &self.conversation, self.now_ms as f64 / 1000.0);
    }

    fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.selected.as_deref() == Some(id) {
            return;
        }
        self.selected = Some(id.to_owned());
        self.load_conversation();
        self.conversation_scroll.scroll_to_bottom();
        cx.notify();
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        self.ticks += 1;
        let mut changed = false;
        if let Some((_, batches)) = &self.watch {
            while batches.try_recv().is_ok() {
                changed = true;
            }
        }
        if changed || self.ticks.is_multiple_of(RELOAD_EVERY) {
            self.reload();
            cx.notify();
        }
    }
}

/// The reply box, disabled: replies arrive with Milestone 3.
fn composer(a: Appearance) -> Div {
    div()
        .flex_shrink_0()
        .px(px(SIDE_PADDING))
        .pt(px(8.0))
        .pb(px(SIDE_PADDING))
        .child(
            div()
                .w_full()
                .min_h(px(44.0))
                .pl(px(14.0))
                .pr(px(8.0))
                .py(px(7.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .rounded(px(RADIUS_COMPOSER))
                .fill(Ink::Card, a)
                .border(px(1.0))
                .border_color(hsla(Ink::Hairline.color(a)))
                .opacity(0.55)
                .child(
                    div()
                        .flex_1()
                        .kit_type(TypeRole::Body)
                        .ink(Ink::TextFaint, a)
                        .child("Read-only for now. Replies arrive with Milestone 3."),
                )
                .child(
                    div()
                        .size(px(SEND_DISC))
                        .flex_shrink_0()
                        .rounded_full()
                        .fill(Ink::Accent, a)
                        .flex()
                        .items_center()
                        .justify_center()
                        .ink(Ink::OnAccent, a)
                        .child(Icon::Send.glyph()),
                ),
        )
}

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        div()
            .size_full()
            .flex()
            .fill(Ink::Window, a)
            .child(sidebar::render(self, a, cx))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(header::render(self, a))
                    .child(conversation::render(self, a))
                    .child(composer(a)),
            )
            .child(inspector::render(self, a))
    }
}
