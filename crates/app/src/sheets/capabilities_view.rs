use gpui::{div, prelude::*, px, App, Div};
use notchling_engine::contract::Tool;
use notchling_kit::metric::RADIUS_SM;
use notchling_kit::ui::{card, hsla, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;

use super::capabilities::{parts, CapabilityRow};

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

fn row(row: &CapabilityRow, first: bool, a: Appearance) -> Div {
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
                .child(label(row.capability, TypeRole::Body, Ink::Text, a).flex_1())
                .child(tag(row.state.word(), a)),
        )
        .child(label(row.line, TypeRole::Meta, Ink::TextFaint, a))
}

fn block(tool: Tool, rows: &[CapabilityRow], a: Appearance) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(label(tool.title(), TypeRole::Body, Ink::Text, a))
        .child(
            card(a)
                .flex()
                .flex_col()
                .children(rows.iter().enumerate().map(|(i, r)| row(r, i == 0, a))),
        )
}

pub fn capabilities_section(a: Appearance, cx: &App) -> Div {
    let mut section = div().flex().flex_col().gap(px(10.0)).child(label(
        t::settings::capabilities_title(),
        TypeRole::Section,
        Ink::TextFaint,
        a,
    ));
    for (tool, rows) in parts() {
        if !rows.is_empty() {
            section = section.child(block(tool, &rows, a));
        }
    }
    section
        .child(label(
            t::settings::capabilities_footer(),
            TypeRole::Meta,
            Ink::TextFaint,
            a,
        ))
        .part("What Notchling Can Do", a, cx)
}
