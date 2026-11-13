use gpui::{div, prelude::*, px, Div};
use notchling_kit::metric::RADIUS_SM;
use notchling_kit::ui::{card, hsla, label, KitStyled};
use notchling_kit::{sentence, Appearance, Ink, TypeRole};

use super::hook_rows::{HookLine, HooksShown, FOOTER, TITLE};

fn tag(text: &str, a: Appearance) -> Div {
    div()
        .px(px(8.0))
        .h(px(20.0))
        .flex()
        .items_center()
        .rounded(px(RADIUS_SM))
        .fill(Ink::Raised, a)
        .child(label(text, TypeRole::Label, Ink::TextMuted, a))
}

fn row(line: &HookLine, first: bool, a: Appearance) -> Div {
    div()
        .px(px(14.0))
        .py(px(8.0))
        .flex()
        .flex_col()
        .gap(px(4.0))
        .when(!first, |r| {
            r.border_t(px(1.0))
                .border_color(hsla(Ink::Hairline.color(a)))
        })
        .child(
            div()
                .min_h(px(24.0))
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(label(&line.title, TypeRole::Body, Ink::Text, a).flex_1())
                .children(line.tag.map(|t| tag(t, a))),
        )
        .children(
            line.note
                .map(|n| label(&sentence(n), TypeRole::Meta, Ink::TextFaint, a)),
        )
}

pub fn hooks_section(shown: &HooksShown, a: Appearance) -> Div {
    let lines = shown.lines();
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(label(TITLE, TypeRole::Section, Ink::TextFaint, a))
        .child(
            card(a)
                .flex()
                .flex_col()
                .children(lines.iter().enumerate().map(|(i, l)| row(l, i == 0, a))),
        )
        .child(label(FOOTER, TypeRole::Meta, Ink::TextFaint, a))
}
