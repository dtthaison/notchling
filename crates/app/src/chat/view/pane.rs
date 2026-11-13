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

pub static RENDERS: AtomicU64 = AtomicU64::new(0);
pub static SIDEBAR_RENDERS: AtomicU64 = AtomicU64::new(0);
pub static DETAILS_RENDERS: AtomicU64 = AtomicU64::new(0);
pub static ROWS: AtomicU64 = AtomicU64::new(0);
pub static LIST_CHANGES: AtomicU64 = AtomicU64::new(0);
pub static ROOT_RENDERS: AtomicU64 = AtomicU64::new(0);

type Select<M> = fn(&ChatView, &App) -> M;
type DrawFrom<M> = fn(&M, Appearance, &mut Context<ChatView>) -> AnyElement;

pub struct ModelPart<M: PartialEq + 'static> {
    chat: WeakEntity<ChatView>,
    select: Select<M>,
    draw: DrawFrom<M>,
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

    pub fn refresh(&mut self, chat: &ChatView, cx: &mut Context<Self>) {
        let app: &App = cx;
        let model = (self.select)(chat, app);
        self.set(model, cx);
    }

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

pub struct Parts {
    pub conversation: Entity<ConversationView>,
    pub sidebar: Entity<ModelPart<SidebarModel>>,
    pub details: Entity<ModelPart<DetailsModel>>,
    pub spins: Entity<SpinLayer>,
    pub root_spins: SpinSlots,
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

fn element(part: AnyView, spins: &SpinSlots, style: StyleRefinement) -> AnyElement {
    spin_host(spins, part.cached(style)).into_any_element()
}

const SIDEBAR_BADGE_BOTTOM: f32 = 60.0;
const DETAILS_BADGE_BOTTOM: f32 = 12.0;

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
    pub fn conversation(&self, cx: &Context<ChatView>) -> AnyElement {
        element(
            self.conversation.clone().into(),
            &self.conversation.read(cx).spins,
            StyleRefinement::default().flex_1().min_h_0().w_full(),
        )
    }

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

pub struct Chrome;

impl Render for Chrome {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().absolute().size_0()
    }
}

impl ChatView {
    pub(super) fn redraw_parts(&self, cx: &mut Context<ChatView>) {
        self.parts.sidebar.update(cx, |part, pcx| part.redraw(pcx));
        self.parts.conversation.update(cx, |_, pcx| pcx.notify());
        self.parts.details.update(cx, |part, pcx| part.redraw(pcx));
    }

    pub(super) fn redraw_chrome(&self, cx: &mut Context<ChatView>) {
        self.chrome.update(cx, |_, cx| cx.notify());
    }

    pub fn bench_chrome(&self, cx: &mut Context<ChatView>) {
        self.redraw_chrome(cx);
    }

    pub fn bench_selected_text(&self, cx: &App) -> Option<String> {
        self.shown_selection(cx)
    }

    pub fn bench_copy_menu(&self) -> Option<(gpui::Point<gpui::Pixels>, bool)> {
        self.copy_menu.as_ref().map(|menu| (menu.at, menu.enabled))
    }

    pub fn bench_at_end(&self, cx: &App) -> bool {
        self.parts.conversation.read(cx).s.at_bottom()
    }

    pub(super) fn redraw_sidebar(&self, cx: &mut Context<ChatView>) {
        self.parts
            .sidebar
            .update(cx, |part, pcx| part.refresh(self, pcx));
    }

    pub(super) fn redraw_details(&self, cx: &mut Context<ChatView>) {
        self.parts.details.update(cx, |part, pcx| part.redraw(pcx));
    }

    pub(super) fn refresh_details(&self, cx: &mut Context<ChatView>) {
        self.parts
            .details
            .update(cx, |part, pcx| part.refresh(self, pcx));
    }

    pub(super) fn redraw_all_but_conversation(&self, cx: &mut Context<ChatView>) {
        self.redraw_sidebar(cx);
        self.refresh_details(cx);
        self.refresh_conversation(cx);
        self.redraw_chrome(cx);
    }
}
