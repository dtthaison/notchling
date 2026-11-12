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
use notchling_kit::render_map::Region;
use notchling_kit::ui::{
    drop_spinner_host, host_spinners, spin_host, window_appearance, SpinGate, SpinLayer, SpinSlots,
};
use notchling_kit::Appearance;

use super::conversation_view::ConversationView;
use super::inspector::DetailsModel;
use super::render_map::{self, Place};
use super::{inspector, sidebar, ChatView};
use crate::chat::sidebar::SidebarModel;

/// Times each part was drawn, for `--render`'s bench.
pub static RENDERS: AtomicU64 = AtomicU64::new(0);
pub static SIDEBAR_RENDERS: AtomicU64 = AtomicU64::new(0);
pub static DETAILS_RENDERS: AtomicU64 = AtomicU64::new(0);
/// Conversation rows built, and times the window's root view (`ChatView`) was drawn.
pub static ROWS: AtomicU64 = AtomicU64::new(0);
pub static ROOT_RENDERS: AtomicU64 = AtomicU64::new(0);

/// What a model part draws, computed from the chat on each of its notifies.
type Select<M> = fn(&ChatView, &App) -> M;
/// Draws a model part from its model; listeners still reach `ChatView` through `cx`.
type DrawFrom<M> = fn(&M, Appearance, &mut Context<ChatView>) -> AnyElement;

/// A part that draws from a model of what it shows, and only when that model changes. It watches
/// `ChatView`, but each notify only computes the model again: an equal one
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
    pub conversation: Entity<ConversationView>,
    pub sidebar: Entity<ModelPart<SidebarModel>>,
    pub details: Entity<ModelPart<DetailsModel>>,
    pub spins: Entity<SpinLayer>,
    /// The spinners `ChatView` lays out itself (the Activity Line's): it is a host that takes only
    /// those, cleared and marked on screen at the start of each draw.
    pub root_spins: SpinSlots,
    /// Whether the window's spinners may be drawn by Core Animation: cleared by `ChatView` while
    /// something GPUI draws over the window is open (`native_spin::covered`).
    pub native: SpinGate,
    _release: gpui::Subscription,
}

impl Parts {
    pub fn new(chat: &Entity<ChatView>, cx: &mut Context<ChatView>) -> Parts {
        let conversation = cx.new(|cx| ConversationView::new(chat, cx));
        let sidebar = cx
            .new(|cx| ModelPart::new(chat, sidebar::model, sidebar::render, &SIDEBAR_RENDERS, cx));
        let details = cx.new(|cx| {
            ModelPart::new(
                chat,
                inspector::model,
                inspector::render,
                &DETAILS_RENDERS,
                cx,
            )
        });
        let root_spins = SpinSlots::only(super::native_spin::ROOT_SPINNERS);
        let root = cx.entity_id();
        host_spinners(cx, root, &root_spins);
        let hosts = vec![
            conversation.read(cx).spins.clone(),
            sidebar.read(cx).spins.clone(),
            details.read(cx).spins.clone(),
            root_spins.clone(),
        ];
        let native = SpinGate::default();
        let layer = if super::native_spin::enabled() {
            SpinLayer::native(
                hosts,
                crate::mac::native_spin::CaSpinners::new(),
                native.clone(),
            )
        } else {
            SpinLayer::new(hosts)
        };
        Parts {
            spins: cx.new(|_| layer),
            conversation,
            sidebar,
            details,
            root_spins,
            native,
            _release: cx.on_release(move |_, cx| drop_spinner_host(cx, root)),
        }
    }
}

/// A part as an element of `ChatView`'s tree, in a box of `style`.
fn element(part: AnyView, spins: &SpinSlots, style: StyleRefinement) -> AnyElement {
    spin_host(spins, part.cached(style)).into_any_element()
}

/// Where the render map's badges sit from the bottom of the Sidebar (over its footer buttons) and
/// of the Details Panel: the part names take their top corners and the edges' middles.
const SIDEBAR_BADGE_BOTTOM: f32 = 60.0;
const DETAILS_BADGE_BOTTOM: f32 = 12.0;

/// A part, and while the ? button is on, the render map over it. The map's box takes the part's
/// style and the part fills it, so nothing moves; the outline and badge are the window's own
/// elements next to the part, never inside it.
fn framed(
    region: Region,
    place: Place,
    part: AnyView,
    spins: &SpinSlots,
    style: StyleRefinement,
    a: Appearance,
    cx: &App,
) -> AnyElement {
    if !render_map::map_on(cx) {
        return element(part, spins, style);
    }
    let mut frame = div().relative().flex();
    frame.style().refine(&style);
    frame
        .child(element(part, spins, StyleRefinement::default().size_full()))
        .children(render_map::overlay(region, place, a, cx))
        .into_any_element()
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
    pub fn sidebar(&self, a: Appearance, cx: &Context<ChatView>) -> AnyElement {
        let width = gpui::px(notchling_kit::metric::SIDEBAR_WIDTH);
        framed(
            Region::Sidebar,
            Place::Bottom(SIDEBAR_BADGE_BOTTOM),
            self.sidebar.clone().into(),
            &self.sidebar.read(cx).spins,
            StyleRefinement::default().w(width).h_full().flex_shrink_0(),
            a,
            cx,
        )
    }

    /// The Details Panel, its own width and the row's height.
    pub fn details(&self, a: Appearance, cx: &Context<ChatView>) -> AnyElement {
        let width = gpui::px(inspector::WIDTH);
        framed(
            Region::Details,
            Place::Bottom(DETAILS_BADGE_BOTTOM),
            self.details.clone().into(),
            &self.details.read(cx).spins,
            StyleRefinement::default().w(width).h_full().flex_shrink_0(),
            a,
            cx,
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
        self.parts.details.update(cx, |part, pcx| part.redraw(pcx));
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

    /// `--render`'s bench: whether the conversation is at its newest message.
    pub fn bench_at_end(&self, cx: &App) -> bool {
        self.parts.conversation.read(cx).s.at_bottom()
    }

    /// Draws the Sidebar again if what it shows changed (and the window's outer parts do not
    /// follow): for a change only the Sidebar shows, or a load of other sessions' files.
    pub(super) fn redraw_sidebar(&self, cx: &mut Context<ChatView>) {
        self.parts
            .sidebar
            .update(cx, |part, pcx| part.refresh(self, pcx));
    }

    /// Draws the Details Panel again whatever its model says: for a change only it shows and its
    /// model can't (a section folded).
    pub(super) fn redraw_details(&self, cx: &mut Context<ChatView>) {
        self.parts.details.update(cx, |part, pcx| part.redraw(pcx));
    }

    /// Draws the Details Panel again if what it shows changed.
    pub(super) fn refresh_details(&self, cx: &mut Context<ChatView>) {
        self.parts
            .details
            .update(cx, |part, pcx| part.refresh(self, pcx));
    }

    /// After a load that changed nothing the conversation shows (another session's files did):
    /// the rows' ages and states (the Sidebar draws only if one changed), the Details Panel and
    /// the window's other parts.
    pub(super) fn redraw_all_but_conversation(&self, cx: &mut Context<ChatView>) {
        self.redraw_sidebar(cx);
        self.refresh_details(cx);
        self.refresh_conversation(cx);
        self.redraw_chrome(cx);
    }
}
