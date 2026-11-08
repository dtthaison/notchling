//! The Chat window's GPUI views (macOS): sidebar, header, conversation, disabled reply box,
//! inspector. Read-only: picking a session is the only thing a click does. The view reloads when
//! `platform::watch` reports a change under the data root (at most every 2 s while changes keep
//! arriving) and every 30 s for the ages (`chat::reload`). Every load runs on the background
//! executor: the [`Loader`] is sent there and taken back with the result, so the window never
//! waits on the disk. The session rows come from the daemon (`session_source`) while it answers.

mod conversation;
mod header;
mod inspector;
mod phone;
mod sheets;
mod sidebar;

use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{div, prelude::*, px, Context, Div, Entity, Render, ScrollHandle, Subscription, Window};
use notchling_engine::transcript::model::Conversation;
use notchling_engine::transcript::subagents::Subagent;
use notchling_kit::metric::{RADIUS_COMPOSER, SEND_DISC, SIDE_PADDING};
use notchling_kit::ui::{hsla, window_appearance, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_platform::watch::{Batch, Watcher};

use crate::chat::data::{DataRoot, Loaded, Loader, Session};
use crate::chat::reload::{watch_config, ReloadGate};
use crate::phone::bridge::Bridge;
use crate::phone::view::PhonePanelView;
use crate::session_source::SessionSource;

/// How often the view looks for watch batches.
const TICK: Duration = Duration::from_millis(500);

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
    /// New Session or Settings, over the window.
    sheet: Option<sheets::Sheet>,
    /// The phone bridge the Phone button and its panel read.
    bridge: Arc<Bridge>,
    /// The Phone panel, while it is open.
    phone: Option<Entity<PhonePanelView>>,
    /// Whether the Phone button's dot is lit, so a change redraws it.
    phone_dot: bool,
    _appearance_changes: Subscription,
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
        ChatView::with_source(root, Arc::new(SessionSource::files()), window, cx)
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
        let mut view = ChatView {
            loader: Some(Loader::new(root.clone())),
            root,
            sessions: Vec::new(),
            selected: None,
            conversation: Conversation::default(),
            subagents: Vec::new(),
            now_ms: 0,
            load_again: false,
            loading: true,
            scroll_to_end: false,
            gate: ReloadGate::default(),
            seen_generation: source.generation(),
            daemon_note: source.view().note(),
            source,
            watch,
            conversation_scroll: ScrollHandle::new(),
            sheet: None,
            bridge: phone::app_bridge(cx),
            phone: None,
            phone_dot: false,
            _appearance_changes: changes,
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
        let claude = self.source.view().agents;
        cx.spawn(async move |this, cx| {
            let (loader, loaded) = cx
                .background_executor()
                .spawn(async move {
                    let loaded = loader.load_from(wanted.as_deref(), claude, now_ms());
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
        self.sessions = loaded.sessions;
        if self.selected == asked {
            self.selected = loaded.selected;
            self.conversation = loaded.conversation;
            self.subagents = loaded.subagents;
            self.loading = false;
            if std::mem::take(&mut self.scroll_to_end) {
                self.conversation_scroll.scroll_to_bottom();
            }
        } else {
            self.load_again = true;
        }
        if std::mem::take(&mut self.load_again) {
            self.start_load(cx);
        }
        cx.notify();
    }

    fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.selected.as_deref() == Some(id) {
            return;
        }
        self.selected = Some(id.to_owned());
        self.conversation = Conversation::default();
        self.subagents.clear();
        self.loading = true;
        self.scroll_to_end = true;
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
                cx.notify();
            }
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
        if self.gate.due(now_ms()) {
            self.start_load(cx);
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
            .relative()
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
                    .child(header::render(self, a, cx))
                    .child(conversation::render(self, a))
                    .child(composer(a)),
            )
            .child(inspector::render(self, a))
            .children(sheets::render(self, a))
            .children(phone::render(self.phone.as_ref(), a, cx))
    }
}
