use gpui::{div, prelude::*, px, App, Div};
use notchling_kit::metric::RADIUS_SM;
use notchling_kit::ui::{card, hsla, label, status_pill, KitPart, KitStyled};
use notchling_kit::{sentence, Appearance, Ink, Status, TypeRole};
use notchling_locale as t;

use super::hook_rows::{HookLine, HookState, HooksShown};

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

fn state(line: &HookLine, a: Appearance) -> Option<Div> {
    let word = line.state.word()?;
    Some(match line.state {
        HookState::Installed => status_pill(Status::Done, word, a),
        HookState::Missing => status_pill(Status::Needs, word, a),
        _ => tag(word, a),
    })
}

fn command(text: &str, a: Appearance) -> Div {
    div()
        .px(px(8.0))
        .py(px(3.0))
        .rounded(px(RADIUS_SM))
        .fill(Ink::Raised, a)
        .child(label(text, TypeRole::Mono, Ink::TextMuted, a).elide())
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
                .children(state(line, a)),
        )
        .child(label(
            &sentence(line.what),
            TypeRole::Meta,
            Ink::TextFaint,
            a,
        ))
        .children(line.command.as_deref().map(|c| command(c, a)))
}

pub fn hooks_section(shown: &HooksShown, a: Appearance, cx: &App) -> Div {
    let lines = shown.lines();
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(label(
            t::settings::hooks(),
            TypeRole::Section,
            Ink::TextFaint,
            a,
        ))
        .children(
            shown
                .status()
                .map(|s| label(&sentence(&s), TypeRole::Meta, Ink::Needs, a)),
        )
        .child(
            card(a)
                .flex()
                .flex_col()
                .children(lines.iter().enumerate().map(|(i, l)| row(l, i == 0, a))),
        )
        .child(label(
            t::settings::hooks_pass_note(),
            TypeRole::Meta,
            Ink::TextFaint,
            a,
        ))
        .child(label(
            t::settings::hooks_footer(),
            TypeRole::Meta,
            Ink::TextFaint,
            a,
        ))
        .part("Agents Hooks", a, cx)
}
