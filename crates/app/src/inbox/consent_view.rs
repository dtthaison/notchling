use gpui::{div, prelude::*, px, Div, ElementId, SharedString, Stateful};
use notchling_kit::metric::{RADIUS_SM, SPACE};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, hsla, label, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;

use super::consent_card::{ConsentCard, Untrusted};
use super::model::Row;
use super::rows::{chips, head};

const LABEL_WIDTH: f32 = 84.0;

fn header(row: &Row, a: Appearance) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(head(row, true, a))
        .children(chips(row, a))
}

fn facts(card: &ConsentCard, a: Appearance) -> Div {
    let mut out = div().flex().flex_col().gap(px(2.0));
    for fact in &card.facts {
        out = out.child(label(fact, TypeRole::Body, Ink::Text, a));
    }
    out
}

fn details(card: &ConsentCard, a: Appearance) -> Div {
    let mut out = div().flex().flex_col().gap(px(2.0));
    for detail in &card.details {
        out = out.child(
            div()
                .flex()
                .items_start()
                .gap(px(SPACE[1]))
                .child(div().w(px(LABEL_WIDTH)).flex_shrink_0().child(label(
                    &detail.label,
                    TypeRole::Meta,
                    Ink::TextFaint,
                    a,
                )))
                .child(div().flex_1().min_w(px(0.0)).child(label(
                    &detail.value,
                    TypeRole::Meta,
                    Ink::TextMuted,
                    a,
                ))),
        );
    }
    out
}

fn untrusted(words: &Untrusted, a: Appearance) -> Div {
    let mut head =
        div()
            .flex()
            .flex_col()
            .child(label(&words.label, TypeRole::Label, Ink::TextMuted, a));
    if let Some(note) = &words.note {
        head = head.child(label(note, TypeRole::Meta, Ink::TextFaint, a));
    }
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[0]))
        .px(px(SPACE[1]))
        .py(px(SPACE[1]))
        .rounded(px(RADIUS_SM))
        .border(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .fill(Ink::Raised, a)
        .child(head)
        .child(label(&words.text, TypeRole::Body, Ink::Text, a))
}

fn expiry(card: &ConsentCard, a: Appearance) -> Div {
    let ink = if card.ended {
        Ink::Needs
    } else {
        Ink::TextFaint
    };
    label(&card.expiry, TypeRole::Meta, ink, a)
}

pub struct Buttons {
    pub allow: Stateful<Div>,
    pub deny: Stateful<Div>,
}

pub fn buttons(card: &ConsentCard, busy: bool, a: Appearance) -> Buttons {
    let key =
        |name: &str| ElementId::from(SharedString::from(format!("consent-{name}-{}", card.id)));
    let allow_state = if card.allow.enabled() && !busy {
        PartState::Rest
    } else {
        PartState::Disabled
    };
    let deny_state = if card.answerable && !busy {
        PartState::Rest
    } else {
        PartState::Disabled
    };
    Buttons {
        allow: button(
            key("allow"),
            t::consent::allow(),
            ButtonKind::Secondary,
            allow_state,
            a,
        ),
        deny: button(
            key("deny"),
            t::consent::deny(),
            ButtonKind::Secondary,
            deny_state,
            a,
        ),
    }
}

pub fn card_body(row: &Row, card: &ConsentCard, a: Appearance) -> Div {
    let mut body = div()
        .flex()
        .flex_col()
        .gap(px(SPACE[1]))
        .child(header(row, a))
        .children(card.asked_again.as_ref().map(|note| {
            label(
                &t::consent::asked_again(note),
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            )
        }))
        .child(facts(card, a))
        .child(details(card, a));
    if let Some(value) = &card.value {
        body = body.child(untrusted(value, a));
    }
    if let Some(says) = &card.says {
        body = body.child(untrusted(says, a));
    }
    body = body.child(expiry(card, a));
    if let Some(blocked) = &card.blocked {
        body = body.child(label(blocked, TypeRole::Meta, Ink::Needs, a));
    }
    body
}
