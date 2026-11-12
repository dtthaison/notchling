//! The window's big parts as views of their own, drawn cached: the conversation's scrolling column,
//! the Sidebar and the Details Panel. GPUI keeps a cached view's layout and paint from the last
//! frame until it is told to draw again, so a frame that only moves a spinner or the live line's
//! clock in the rest of the window costs that part alone. Before, every such frame laid out every
//! message (a long conversation: 50 ms a frame, a core), then every sidebar row and the panel.
//!
//! A part draws again whenever `ChatView` notifies (it observes the view, whose state it draws),
//! when it is scrolled or hovered (GPUI notifies the view an element is in), and on a window
//! refresh. Its spinners are slots a [`SpinLayer`] paints (`notchling_kit::ui::spin_layer`): a
//! spinner that was a view of its own would draw its part again ten times a second, since GPUI
//! redraws a notified view's ancestors. The clock and the window's other spinners wake only
//! themselves or the window's outer parts, never `ChatView`'s observers.

use std::sync::atomic::{AtomicU64, Ordering};

use gpui::{
    div, prelude::*, AnyElement, AnyView, App, Context, Entity, StyleRefinement, Subscription,
    WeakEntity, Window,
};
use notchling_kit::ui::{
    drop_spinner_host, host_spinners, scrollable, spin_host, window_appearance, SpinLayer,
    SpinSlots,
};
use notchling_kit::Appearance;

use super::{conversation, inspector, now_ms, sidebar, ChatView};
use crate::chat::follow::{self, Scroll};
use crate::chat::sidebar::SidebarModel;

/// Times each part was drawn, for `--render`'s bench.
pub static RENDERS: AtomicU64 = AtomicU64::new(0);
pub static SIDEBAR_RENDERS: AtomicU64 = AtomicU64::new(0);
pub static DETAILS_RENDERS: AtomicU64 = AtomicU64::new(0);
/// Conversation rows built, and times the window's root view (`ChatView`) was drawn.
pub static ROWS: AtomicU64 = AtomicU64::new(0);
pub static ROOT_RENDERS: AtomicU64 = AtomicU64::new(0);

/// What a part draws, from the chat's state.
type Draw = fn(&mut ChatView, Appearance, &mut Context<ChatView>) -> AnyElement;

/// What a model part draws, computed from the chat on each of its notifies.
type Select<M> = fn(&ChatView, &App) -> M;
/// Draws a model part from its model; listeners still reach `ChatView` through `cx`.
type DrawFrom<M> = fn(&M, Appearance, &mut Context<ChatView>) -> AnyElement;

/// A part that draws whatever `ChatView` says (the conversation, until Task 4.3).
pub struct RawPart {
    chat: WeakEntity<ChatView>,
    draw: Draw,
    renders: &'static AtomicU64,
    spins: SpinSlots,
    _changes: Subscription,
    _release: Subscription,
}

impl RawPart {
    fn new(
        chat: &Entity<ChatView>,
        draw: Draw,
        renders: &'static AtomicU64,
        cx: &mut Context<Self>,
    ) -> Self {
        let spins = SpinSlots::default();
        let id = cx.entity_id();
        host_spinners(cx, id, &spins);
        RawPart {
            chat: chat.downgrade(),
            draw,
            renders,
            spins,
            _changes: cx.observe(chat, |_, _, cx| cx.notify()),
            _release: cx.on_release(move |_, cx| drop_spinner_host(cx, id)),
        }
    }
}

impl Render for RawPart {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders.fetch_add(1, Ordering::Relaxed);
        self.spins.clear();
        let a = window_appearance(window);
        let Some(chat) = self.chat.upgrade() else {
            return div().into_any_element();
        };
        let draw = self.draw;
        chat.update(cx, |chat, cx| draw(chat, a, cx))
    }
}

/// A part that draws from a model of what it shows, and only when that model changes. It watches
/// `ChatView` like a [`RawPart`], but each notify only computes the model again: an equal one
/// draws nothing, so a change no part shows (a draft, a menu) costs the part a comparison.
pub struct ModelPart<M: PartialEq + 'static> {
    chat: WeakEntity<ChatView>,
    select: Select<M>,
    draw: DrawFrom<M>,
    /// The model drawn last; `None` asks for a fresh one at the next draw.
    model: Option<M>,
    renders: &'static AtomicU64,
    spins: SpinSlots,
    _changes: Subscription,
    _release: Subscription,
}

impl<M: PartialEq + 'static> ModelPart<M> {
    fn new(
        chat: &Entity<ChatView>,
        select: Select<M>,
        draw: DrawFrom<M>,
        renders: &'static AtomicU64,
        cx: &mut Context<Self>,
    ) -> Self {
        let spins = SpinSlots::default();
        let id = cx.entity_id();
        host_spinners(cx, id, &spins);
        ModelPart {
            chat: chat.downgrade(),
            select,
            draw,
            model: None,
            renders,
            spins,
            _changes: cx.observe(chat, |this, chat, cx| {
                let app: &App = cx;
                let model = (this.select)(chat.read(app), app);
                this.set(model, cx);
            }),
            _release: cx.on_release(move |_, cx| drop_spinner_host(cx, id)),
        }
    }

    fn set(&mut self, model: M, cx: &mut Context<Self>) {
        if self.model.as_ref() != Some(&model) {
            self.model = Some(model);
            cx.notify();
        }
    }

    /// Computes the model again and draws only if it changed. `chat` is the view being updated
    /// (reading its entity from here would be a double borrow).
    pub fn refresh(&mut self, chat: &ChatView, cx: &mut Context<Self>) {
        let app: &App = cx;
        let model = (self.select)(chat, app);
        self.set(model, cx);
    }

    /// Draws again whatever the model says, for what the model can't see (the appearance).
    pub fn redraw(&mut self, cx: &mut Context<Self>) {
        self.model = None;
        cx.notify();
    }
}

impl<M: PartialEq + 'static> Render for ModelPart<M> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders.fetch_add(1, Ordering::Relaxed);
        self.spins.clear();
        let a = window_appearance(window);
        let Some(chat) = self.chat.upgrade() else {
            return div().into_any_element();
        };
        let model = match self.model.take() {
            Some(model) => model,
            None => {
                let app: &App = cx;
                (self.select)(chat.read(app), app)
            }
        };
        let draw = self.draw;
        let element = chat.update(cx, |_, ccx| draw(&model, a, ccx));
        self.model = Some(model);
        element
    }
}

/// The parts and the layer that paints their spinners, made with the view.
pub struct Parts {
    pub conversation: Entity<RawPart>,
    pub sidebar: Entity<ModelPart<SidebarModel>>,
    pub details: Entity<RawPart>,
    pub spins: Entity<SpinLayer>,
}

impl Parts {
    pub fn new(chat: &Entity<ChatView>, cx: &mut Context<ChatView>) -> Parts {
        let conversation = cx.new(|cx| RawPart::new(chat, draw_conversation, &RENDERS, cx));
        let sidebar = cx
            .new(|cx| ModelPart::new(chat, sidebar::model, sidebar::render, &SIDEBAR_RENDERS, cx));
        let details = cx.new(|cx| RawPart::new(chat, draw_details, &DETAILS_RENDERS, cx));
        let hosts = vec![
            conversation.read(cx).spins.clone(),
            sidebar.read(cx).spins.clone(),
            details.read(cx).spins.clone(),
        ];
        Parts {
            spins: cx.new(|_| SpinLayer::new(hosts)),
            conversation,
            sidebar,
            details,
        }
    }
}

fn draw_conversation(chat: &mut ChatView, a: Appearance, cx: &mut Context<ChatView>) -> AnyElement {
    // A hovered message's relative time is kept fresh from when it was last drawn.
    chat.drawn_ms = now_ms();
    div()
        .size_full()
        .flex()
        .flex_col()
        .child(scrollable(conversation::render(chat, a, cx)).tracking(&chat.conversation_scroll))
        .into_any_element()
}

fn draw_details(chat: &mut ChatView, a: Appearance, cx: &mut Context<ChatView>) -> AnyElement {
    inspector::render(chat, a, cx).into_any_element()
}

/// A part as an element of `ChatView`'s tree, in a box of `style`.
fn element(part: AnyView, spins: &SpinSlots, style: StyleRefinement) -> AnyElement {
    spin_host(spins, part.cached(style)).into_any_element()
}

impl Parts {
    /// The conversation: it fills the column's room above the reply box.
    pub fn conversation(&self, cx: &Context<ChatView>) -> AnyElement {
        element(
            self.conversation.clone().into(),
            &self.conversation.read(cx).spins,
            StyleRefinement::default().flex_1().min_h_0().w_full(),
        )
    }

    /// The Sidebar, its own width and the window's height.
    pub fn sidebar(&self, cx: &Context<ChatView>) -> AnyElement {
        let width = gpui::px(notchling_kit::metric::SIDEBAR_WIDTH);
        element(
            self.sidebar.clone().into(),
            &self.sidebar.read(cx).spins,
            StyleRefinement::default().w(width).h_full().flex_shrink_0(),
        )
    }

    /// The Details Panel, its own width and the row's height.
    pub fn details(&self, cx: &Context<ChatView>) -> AnyElement {
        let width = gpui::px(inspector::WIDTH);
        element(
            self.details.clone().into(),
            &self.details.read(cx).spins,
            StyleRefinement::default().w(width).h_full().flex_shrink_0(),
        )
    }
}

/// An empty view in `ChatView`'s tree, outside the parts: waking it draws the window's other parts
/// again (GPUI redraws a view's ancestors) without telling the parts, for a change only they show
/// (the live line's clock, the Phone button's dot, the header's meters).
pub struct Chrome;

impl Render for Chrome {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().absolute().size_0()
    }
}

impl ChatView {
    /// Draws every part again whatever its model says: for what a model can't see (the
    /// appearance, which the parts read at draw time).
    pub(super) fn redraw_parts(&self, cx: &mut Context<ChatView>) {
        self.parts.sidebar.update(cx, |part, pcx| part.redraw(pcx));
        self.parts.conversation.update(cx, |_, pcx| pcx.notify());
        self.parts.details.update(cx, |_, pcx| pcx.notify());
    }

    /// Draws the window again but not its cached parts, whose drawing stays: for a change shown
    /// only outside them. `cx.notify()` draws them too.
    pub(super) fn redraw_chrome(&self, cx: &mut Context<ChatView>) {
        self.chrome.update(cx, |_, cx| cx.notify());
    }

    /// `--render`'s bench: makes the window draw without waking a part, to see whether frames
    /// arrive at all.
    pub fn bench_chrome(&self, cx: &mut Context<ChatView>) {
        self.redraw_chrome(cx);
    }

    /// Draws the Sidebar again if what it shows changed (and the window's outer parts do not
    /// follow): for a change only the Sidebar shows, or a load of other sessions' files.
    pub(super) fn redraw_sidebar(&self, cx: &mut Context<ChatView>) {
        self.parts
            .sidebar
            .update(cx, |part, pcx| part.refresh(self, pcx));
    }

    /// Draws the Details Panel again (and the window's outer parts), not the conversation: for a
    /// change only it shows (a section folded).
    pub(super) fn redraw_details(&self, cx: &mut Context<ChatView>) {
        self.parts.details.update(cx, |_, cx| cx.notify());
    }

    /// After a load that changed nothing the conversation shows (another session's files did):
    /// the rows' ages and states (the Sidebar draws only if one changed), the Details Panel and
    /// the window's other parts.
    pub(super) fn redraw_all_but_conversation(&self, cx: &mut Context<ChatView>) {
        self.redraw_sidebar(cx);
        self.redraw_details(cx);
        self.redraw_chrome(cx);
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

    /// Whether the "Latest" pill shows: the conversation is scrolled up from its newest message.
    pub(super) fn latest_pill(&self) -> bool {
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
    pub(super) fn jump_to_latest(&mut self, cx: &mut Context<Self>) {
        self.latest_preview = false;
        self.conversation_scroll.scroll_to_bottom();
        cx.notify();
    }
}
