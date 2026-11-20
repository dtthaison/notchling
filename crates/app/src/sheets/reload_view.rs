use gpui::{div, prelude::*, px, Context, Div, EventEmitter, Render, SharedString, Window};
use notchling_kit::metric::{RADIUS_CARD, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, hsla, label, window_appearance, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::header::sheet_header;
use super::view::Dismiss;
use crate::chat::session_reload::{Act, Card};

pub const WIDTH: f32 = 560.0;

pub struct ReloadChoice(pub Act);

pub struct ReloadView {
    card: Card,
}

impl EventEmitter<Dismiss> for ReloadView {}
impl EventEmitter<ReloadChoice> for ReloadView {}

pub fn body(card: &Card, a: Appearance) -> Div {
    let status = card.status();
    let ink = if card.problem() {
        Ink::Needs
    } else {
        Ink::Text
    };
    div()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(label(&card.intro(), TypeRole::Body, Ink::Text, a))
        .children(status.map(|text| card_row(a).child(label(&text, TypeRole::Body, ink, a))))
        .children(
            (card.stage == crate::chat::session_reload::Stage::Terminal)
                .then(|| card_row(a).child(label(&card.command, TypeRole::Mono, Ink::Text, a))),
        )
        .children(
            card.note()
                .map(|text| label(&text, TypeRole::Meta, Ink::TextMuted, a)),
        )
}

fn card_row(a: Appearance) -> Div {
    card(a).px(px(12.0)).py(px(10.0))
}

pub fn buttons<V: 'static>(
    card: &Card,
    a: Appearance,
    cx: &mut Context<V>,
    send: impl Fn(Act, &mut Context<V>) + Clone + 'static,
) -> Vec<gpui::AnyElement> {
    card.acts()
        .into_iter()
        .enumerate()
        .map(|(i, act)| {
            let kind = if i == 0 {
                ButtonKind::Primary
            } else {
                ButtonKind::Secondary
            };
            let send = send.clone();
            button(
                SharedString::from(format!("reload-act-{i}")),
                act.label(),
                kind,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(move |_, _, _, cx| send(act, cx)))
            .into_any_element()
        })
        .collect()
}

impl ReloadView {
    pub fn new(card: Card) -> ReloadView {
        ReloadView { card }
    }

    pub fn card(&self) -> &Card {
        &self.card
    }

    pub fn show(&mut self, card: Card, cx: &mut Context<Self>) {
        if self.card != card {
            self.card = card;
            cx.notify();
        }
    }

    fn footer(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        div()
            .flex_shrink_0()
            .px(px(SIDE_PADDING))
            .py(px(12.0))
            .flex()
            .items_center()
            .justify_end()
            .gap(px(8.0))
            .border_t(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .children(buttons(&self.card, a, cx, |act, cx| {
                cx.emit(ReloadChoice(act))
            }))
    }
}

impl Render for ReloadView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        div()
            .relative()
            .w_full()
            .flex()
            .flex_col()
            .rounded(px(RADIUS_CARD))
            .overflow_hidden()
            .fill(Ink::Window, a)
            .child(sheet_header(
                Icon::Resume,
                t::chat::reload_title(),
                Vec::new(),
                a,
                cx,
            ))
            .child(div().p(px(SIDE_PADDING)).child(body(&self.card, a)))
            .child(self.footer(a, cx))
    }
}
