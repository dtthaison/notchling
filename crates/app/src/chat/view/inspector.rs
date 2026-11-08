//! The inspector: tasks, facts, subagents and background shells of the selected session.

use gpui::{div, prelude::*, px, Div, FontWeight};
use notchling_kit::metric::{SIDE_PADDING, TITLE_BAR};
use notchling_kit::ui::{hsla, label, status_pill, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, Status, TypeRole};

use super::ChatView;
use crate::chat::inspector::{inspector, no_writers};

const WIDTH: f32 = 260.0;

fn section(title: &str, a: Appearance, content: Div) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(label(title, TypeRole::Section, Ink::TextFaint, a))
        .child(content)
}

fn empty(text: &str, a: Appearance) -> Div {
    label(text, TypeRole::Meta, Ink::TextMuted, a)
}

fn status_glyph(icon: Icon, status: Status, a: Appearance) -> Div {
    div()
        .w(px(14.0))
        .flex_shrink_0()
        .text_size(px(11.0))
        .font_weight(FontWeight::MEDIUM)
        .ink(Ink::status(status), a)
        .child(icon.glyph())
}

fn two_lines(first: &str, second: &str, a: Appearance) -> Div {
    div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .gap(px(1.0))
        .child(label(first, TypeRole::Body, Ink::Text, a))
        .child(label(second, TypeRole::Meta, Ink::TextMuted, a))
}

pub fn render(view: &ChatView, a: Appearance) -> Div {
    let i = inspector(
        &view.conversation,
        &view.subagents,
        &view.root.home,
        &no_writers,
    );
    let tasks = if i.tasks.is_empty() {
        empty("No tasks", a)
    } else {
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .children(i.tasks.iter().map(|t| {
                div()
                    .flex()
                    .items_start()
                    .gap(px(6.0))
                    .child(status_glyph(t.icon, t.status, a))
                    .child(label(&t.subject, TypeRole::Body, Ink::Text, a))
            }))
    };
    let facts = if i.facts.is_empty() {
        empty("Nothing read yet", a)
    } else {
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .children(i.facts.iter().map(|(name, value)| {
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.0))
                    .child(div().w(px(72.0)).flex_shrink_0().child(label(
                        name,
                        TypeRole::Meta,
                        Ink::TextMuted,
                        a,
                    )))
                    .child(div().flex_1().min_w(px(0.0)).child(label(
                        value,
                        TypeRole::Meta,
                        Ink::Text,
                        a,
                    )))
            }))
    };
    let subagents = if i.subagents.is_empty() {
        empty("None started", a)
    } else {
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .children(i.subagents.iter().map(|s| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(two_lines(&s.description, &s.detail, a))
                    .child(status_pill(s.status, &s.status_word, a))
            }))
    };
    let shells = if i.shells.is_empty() {
        empty("None running", a)
    } else {
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .children(i.shells.iter().map(|s| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(two_lines(&s.title, &s.command, a))
                    .child(status_pill(s.status, &s.status_word, a))
            }))
    };
    div()
        .w(px(WIDTH))
        .h_full()
        .flex_shrink_0()
        .fill(Ink::Sidebar, a)
        .border_l(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .flex()
        .flex_col()
        .child(div().h(px(TITLE_BAR)).flex_shrink_0())
        .child(
            div()
                .id("inspector")
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scroll()
                .px(px(SIDE_PADDING))
                .pb(px(SIDE_PADDING))
                .flex()
                .flex_col()
                .gap(px(16.0))
                .child(section("Tasks", a, tasks))
                .child(section("Facts", a, facts))
                .child(section("Subagents", a, subagents))
                .child(section("Shells", a, shells)),
        )
}
