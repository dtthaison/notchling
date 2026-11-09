//! The conversation column: the rows of `chat::rows`, drawn with the kit.

use gpui::{div, prelude::*, px, relative, Context, Div, FontWeight, Stateful};
use notchling_engine::transcript::model::{DiffKind, DiffLine};
use notchling_kit::metric::{RADIUS_CARD, RADIUS_SM, SIDE_PADDING};
use notchling_kit::ui::{claude_mark, hsla, label, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::{opinions, ChatView};
use crate::chat::data::Tool;
use crate::chat::rows::{rows, Row, ToolRow};

/// Thinking, tools and their buttons sit a little in from the reply's left edge.
const INDENT: f32 = 12.0;

fn glyph(icon: Icon, ink: Ink, a: Appearance) -> Div {
    div()
        .flex_shrink_0()
        .text_size(px(11.0))
        .font_weight(FontWeight::MEDIUM)
        .ink(ink, a)
        .child(icon.glyph())
}

fn mono_box(text: &str, ink: Ink, a: Appearance) -> Div {
    div()
        .w_full()
        .p(px(8.0))
        .rounded(px(RADIUS_SM))
        .fill(Ink::Raised, a)
        .kit_type(TypeRole::Mono)
        .ink(ink, a)
        .child(text.to_owned())
}

fn diff_line(line: &DiffLine, a: Appearance) -> Div {
    let (ink, sign) = match line.kind {
        DiffKind::Add => (Ink::Done, "+"),
        DiffKind::Remove => (Ink::Expired, "−"),
        DiffKind::Context => (Ink::TextMuted, " "),
        DiffKind::Hunk => (Ink::TextFaint, "@"),
    };
    let wash = matches!(line.kind, DiffKind::Add | DiffKind::Remove);
    div()
        .w_full()
        .flex()
        .gap(px(8.0))
        .px(px(8.0))
        .kit_type(TypeRole::Mono)
        .ink(ink, a)
        .when(wash, |d| d.bg(hsla(ink.color(a).with_alpha(0.12))))
        .child(
            div()
                .w(px(28.0))
                .flex_shrink_0()
                .ink(Ink::TextFaint, a)
                .child(line.number.map(|n| n.to_string()).unwrap_or_default()),
        )
        .child(div().w(px(10.0)).flex_shrink_0().child(sign))
        .child(div().flex_1().min_w(px(0.0)).child(line.text.clone()))
}

fn tool(row: &ToolRow, a: Appearance) -> Div {
    let title_ink = if row.is_error {
        Ink::Expired
    } else {
        Ink::Text
    };
    let mut element = div().flex().flex_col().gap(px(4.0)).child(
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .h(px(22.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .rounded_full()
                    .fill(Ink::Raised, a)
                    .child(glyph(row.icon, Ink::TextMuted, a))
                    .child(label(&row.title, TypeRole::Label, title_ink, a)),
            )
            .when(!row.finished, |d| {
                d.child(label("running", TypeRole::Meta, Ink::Working, a))
            })
            .when(row.added + row.removed > 0, |d| {
                d.child(label(
                    &format!("+{} −{}", row.added, row.removed),
                    TypeRole::Meta,
                    Ink::TextMuted,
                    a,
                ))
            }),
    );
    if !row.input.is_empty() {
        element = element.child(
            div()
                .kit_type(TypeRole::Mono)
                .ink(Ink::TextMuted, a)
                .line_clamp(3)
                .child(row.input.clone()),
        );
    }
    if let Some(output) = &row.output {
        let ink = if row.is_error {
            Ink::Expired
        } else {
            Ink::TextFaint
        };
        let text = if output.more > 0 {
            format!("{}\n… +{} lines", output.text, output.more)
        } else {
            output.text.clone()
        };
        element = element.child(mono_box(&text, ink, a));
    }
    if !row.diff.is_empty() {
        element = element.child(
            div()
                .w_full()
                .py(px(4.0))
                .rounded(px(RADIUS_SM))
                .fill(Ink::Raised, a)
                .flex()
                .flex_col()
                .children(row.diff.iter().map(|line| diff_line(line, a))),
        );
    }
    element
}

fn row_element(row: &Row, a: Appearance) -> Div {
    match row {
        Row::User {
            text,
            images,
            queued,
        } => div()
            .w_full()
            .flex()
            .flex_col()
            .items_end()
            .gap(px(2.0))
            .when(*queued, |d| {
                d.child(label("Queued", TypeRole::Meta, Ink::TextFaint, a))
            })
            .child(
                div()
                    .max_w(relative(0.75))
                    .px(px(12.0))
                    .py(px(8.0))
                    .rounded(px(RADIUS_CARD))
                    .fill(Ink::AccentWash, a)
                    .kit_type(TypeRole::Body)
                    .ink(Ink::Text, a)
                    .when(*queued, |d| d.opacity(0.6))
                    .child(text.clone()),
            )
            .when(*images > 0, |d| {
                d.child(label(
                    &format!("{images} image{}", if *images == 1 { "" } else { "s" }),
                    TypeRole::Meta,
                    Ink::TextFaint,
                    a,
                ))
            }),
        // No mark before each reply: the bubble alone says who wrote it.
        Row::Assistant { text } => div().w_full().flex().items_start().child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .p(px(12.0))
                .rounded(px(RADIUS_CARD))
                .fill(Ink::Card, a)
                .border(px(1.0))
                .border_color(hsla(Ink::Hairline.color(a)))
                .kit_type(TypeRole::Body)
                .ink(Ink::Text, a)
                .child(text.clone()),
        ),
        Row::Thinking { title, text } => div()
            .pl(px(INDENT))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(label(title, TypeRole::Meta, Ink::TextMuted, a))
            .when(!text.is_empty(), |d| {
                d.child(
                    div()
                        .kit_type(TypeRole::Meta)
                        .ink(Ink::TextFaint, a)
                        .italic()
                        .child(text.clone()),
                )
            }),
        Row::Tools(tools) => div()
            .pl(px(INDENT))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .children(tools.iter().map(|t| tool(t, a))),
        Row::TurnEnd { text } => div().w_full().flex().justify_center().child(label(
            text,
            TypeRole::Meta,
            Ink::TextFaint,
            a,
        )),
        Row::Notice { text, icon } => div()
            .w_full()
            .flex()
            .justify_center()
            .items_center()
            .gap(px(6.0))
            .child(glyph(*icon, Ink::TextMuted, a))
            .child(label(text, TypeRole::Meta, Ink::TextMuted, a)),
        Row::CommandOutput { text } => {
            div()
                .pl(px(INDENT))
                .child(mono_box(text, Ink::TextMuted, a))
        }
    }
}

/// A row with its Second Opinion buttons and the cards of the opinions asked under it, when the
/// item has something to review.
fn row_with_opinions(
    view: &ChatView,
    i: usize,
    row: &Row,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Div {
    let element = row_element(row, a);
    let Some(item) = view.conversation.items.get(i) else {
        return element;
    };
    let subjects = opinions::subjects(item);
    if subjects.is_empty() {
        return element;
    }
    let anchors: Vec<String> = subjects.iter().map(|(anchor, _)| anchor.clone()).collect();
    let buttons = div().pl(px(INDENT)).flex().gap(px(4.0)).children(
        subjects
            .into_iter()
            .map(|(anchor, subject)| opinions::opinion_button(&anchor, subject, a, cx)),
    );
    let cards = view
        .selected
        .as_deref()
        .and_then(|session| opinions::cards(session, &anchors, a, cx))
        .map(|cards| cards.pl(px(INDENT)));
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(element)
        .child(buttons)
        .children(cards)
}

pub fn render(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Stateful<Div> {
    let rows = rows(&view.conversation);
    let column = div()
        .id("conversation")
        .flex_1()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .track_scroll(&view.conversation_scroll)
        .px(px(SIDE_PADDING))
        .py(px(12.0))
        .flex()
        .flex_col()
        .gap(px(12.0));
    if view.loading {
        // The picked session's conversation is on its way; the previous one's would mislead.
        return column;
    }
    if view.selected_session().is_none() {
        return column.child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .child(claude_mark(40.0, a))
                .child(label("No sessions yet", TypeRole::Heading, Ink::Text, a)),
        );
    }
    if rows.is_empty() {
        let note = match view.selected_session().map(|s| s.tool) {
            // Loaded through the daemon, which holds the session open only while acting is on.
            Some(Tool::Antigravity) if !view.tools.agy.as_ref().is_some_and(|c| c.open) => {
                match view.acting(cx) {
                    crate::acting::Acting::On => "Loading from Antigravity…",
                    _ => "Antigravity sessions load through the daemon while acting is on.",
                }
            }
            _ => "No transcript yet for this session",
        };
        return column.child(label(note, TypeRole::Meta, Ink::TextMuted, a));
    }
    column.children(
        rows.iter()
            .enumerate()
            .map(|(i, row)| row_with_opinions(view, i, row, a, cx)),
    )
}
