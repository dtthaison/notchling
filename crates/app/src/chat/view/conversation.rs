//! The conversation column: the rows of `chat::rows`, drawn with the kit the way the Swift app's
//! `ChatParts` draws them. A run of tool calls is one chip ("Ran 4 commands, read 1 file", the
//! kinds' icons, a red failure count, a spinner while one runs, a chevron) that opens to a line
//! per call, each opening to its command, output or diff; a thought is one faint line that opens
//! to its text; a reply is Markdown; a turn ends in "Worked for 48s · Opus 5.5" between hairlines.
//! What is open is `ChatView::expanded`; the newest group of a working session is always open.

use std::path::Path;
use std::time::Duration;

use gpui::{
    div, prelude::*, px, relative, Animation, AnimationExt, Context, Div, FontWeight, SharedString,
    Stateful,
};
use notchling_engine::sessions::agent::AgentGroup;
use notchling_engine::transcript::model::{DiffKind, DiffLine};
use notchling_kit::labels::Corner;
use notchling_kit::metric::{RADIUS_CARD, RADIUS_MD, RADIUS_SM, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, claude_mark, hsla, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::{markdown, opinions, ChatView};
use crate::chat::data::Tool;
use crate::chat::rows::{rows, Row, ToolRow};
use crate::chat::steps::{self, Detail};

/// Thinking, tools and their buttons sit a little in from the reply's left edge.
const INDENT: f32 = 12.0;
/// The chevron that opens and closes a row: right when folded, down when open.
const CHEVRON_CLOSED: &str = "›";
const CHEVRON_OPEN: &str = "⌄";

fn glyph(icon: Icon, size: f32, ink: Ink, a: Appearance) -> Div {
    div()
        .flex_shrink_0()
        .text_size(px(size))
        .font_weight(FontWeight::MEDIUM)
        .ink(ink, a)
        .child(icon.glyph())
}

fn chevron(open: bool, a: Appearance) -> Div {
    div()
        .flex_shrink_0()
        .w(px(10.0))
        .flex()
        .justify_center()
        .text_size(px(12.0))
        .font_weight(FontWeight::BOLD)
        .ink(Ink::TextFaint, a)
        .child(if open { CHEVRON_OPEN } else { CHEVRON_CLOSED })
}

/// A small spinner: the working glyph, breathing.
fn spinner(key: &str, a: Appearance) -> impl IntoElement {
    glyph(Icon::Working, 11.0, Ink::Working, a).with_animation(
        SharedString::from(format!("spin-{key}")),
        Animation::new(Duration::from_millis(1000)).repeat(),
        |d, t| d.opacity(0.3 + 0.7 * (1.0 - (2.0 * t - 1.0).abs())),
    )
}

fn hairline(a: Appearance) -> Div {
    div().flex_1().h(px(1.0)).fill(Ink::Hairline, a)
}

fn diff_line(line: &DiffLine, a: Appearance) -> Div {
    if line.kind == DiffKind::Hunk {
        return div().px(px(10.0)).py(px(3.0)).child(label(
            &line.text,
            TypeRole::Meta,
            Ink::TextFaint,
            a,
        ));
    }
    let (ink, sign) = match line.kind {
        DiffKind::Add => (Ink::Done, "+"),
        DiffKind::Remove => (Ink::Expired, "−"),
        _ => (Ink::TextFaint, " "),
    };
    let wash = matches!(line.kind, DiffKind::Add | DiffKind::Remove);
    div()
        .w_full()
        .flex()
        .py(px(1.0))
        .kit_type(TypeRole::Mono)
        .when(wash, |d| d.bg(hsla(ink.color(a).with_alpha(0.12))))
        .child(
            div()
                .w(px(40.0))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .ink(Ink::TextFaint, a)
                .child(line.number.map(|n| n.to_string()).unwrap_or_default()),
        )
        .child(
            div()
                .w(px(20.0))
                .flex_shrink_0()
                .flex()
                .justify_center()
                .font_weight(FontWeight::BOLD)
                .ink(ink, a)
                .child(sign),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .pr(px(10.0))
                .ink(Ink::Text, a)
                .child(if line.text.is_empty() {
                    " ".to_owned()
                } else {
                    line.text.clone()
                }),
        )
}

/// A file change (`DiffView`): the file and its folder, Show in Finder, then the lines.
fn diff_view(step: &ToolRow, a: Appearance, cx: &mut Context<ChatView>) -> Div {
    let path = Path::new(&step.input);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| step.input.clone());
    let folder = path
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let reveal = step.input.clone();
    div()
        .w_full()
        .rounded(px(RADIUS_MD))
        .fill(Ink::Card, a)
        .border(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(28.0))
                .pl(px(10.0))
                .pr(px(4.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .border_b(px(1.0))
                .border_color(hsla(Ink::Hairline.color(a)))
                .child(glyph(Icon::File, 10.0, Ink::Text, a))
                .child(label(&name, TypeRole::Label, Ink::Text, a))
                .child(
                    label(&folder, TypeRole::Meta, Ink::TextFaint, a)
                        .flex_1()
                        .min_w(px(0.0))
                        .elide(),
                )
                .child(
                    button(
                        SharedString::from(format!("reveal-{}", step.id)),
                        "Show in Finder",
                        ButtonKind::Ghost,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.reveal_path(Path::new(&reveal));
                    })),
                ),
        )
        .child(
            div()
                .py(px(4.0))
                .flex()
                .flex_col()
                .children(step.diff.iter().map(|line| diff_line(line, a))),
        )
}

/// What opens under a step: its diff, or its command or input and its output.
fn step_detail(step: &ToolRow, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let mut detail = div().pl(px(24.0)).flex().flex_col().gap(px(6.0));
    match steps::detail(step) {
        Detail::None => return None,
        Detail::Diff => detail = detail.child(diff_view(step, a, cx)),
        Detail::Text {
            input_title,
            has_output,
        } => {
            if let Some(title) = input_title {
                detail = detail.child(markdown::mono_box(
                    &format!("{}-in", step.id),
                    title,
                    &step.input,
                    false,
                    a,
                ));
            }
            if has_output {
                if let Some(output) = &step.output {
                    let text = if output.more > 0 {
                        format!("{}\n… +{} lines", output.text, output.more)
                    } else {
                        output.text.clone()
                    };
                    let title = if step.is_error { "Error" } else { "Output" };
                    detail = detail.child(markdown::mono_box(
                        &format!("{}-out", step.id),
                        title,
                        &text,
                        step.is_error,
                        a,
                    ));
                }
            }
        }
    }
    Some(detail)
}

/// One call (`ToolCallRow`): icon, title, diff counts, its state, a chevron when something opens.
fn step_row(
    view: &ChatView,
    step: &ToolRow,
    live: bool,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Div {
    let has_detail = steps::detail(step) != Detail::None;
    let open = has_detail && view.is_open(&step.id);
    let running = live && !step.finished;
    let icon_ink = if step.is_error {
        Ink::Expired
    } else {
        Ink::TextMuted
    };
    let id = step.id.clone();
    let line = div()
        .id(SharedString::from(format!("step-{}", step.id)))
        .w_full()
        .min_h(px(28.0))
        .px(px(8.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .rounded(px(RADIUS_SM))
        .when(has_detail, |d| {
            d.cursor_pointer()
                .hover(|s| s.bg(hsla(Ink::Raised.color(a).with_alpha(0.6))))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(&id, cx)))
        })
        .child(
            div()
                .w(px(16.0))
                .flex_shrink_0()
                .flex()
                .justify_center()
                .child(glyph(step.icon, 11.0, icon_ink, a)),
        )
        .child(
            label(&step.title, TypeRole::Body, Ink::Text, a)
                .flex_1()
                .min_w(px(0.0))
                .elide(),
        )
        .when(step.added + step.removed > 0, |d| {
            d.child(label(
                &format!("+{}", step.added),
                TypeRole::Mono,
                Ink::Done,
                a,
            ))
            .child(label(
                &format!("−{}", step.removed),
                TypeRole::Mono,
                Ink::Expired,
                a,
            ))
        })
        .when(running, |d| d.child(spinner(&step.id, a)))
        .when(!running && step.is_error, |d| {
            d.child(glyph(Icon::Needs, 11.0, Ink::Expired, a))
        })
        .when(has_detail, |d| d.child(chevron(open, a)))
        .part_at("Step Row", Corner::Trailing, a, cx);
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(line)
        .when(open, |d| d.children(step_detail(step, a, cx)))
}

/// The chip over a run of calls (`ToolStepView`): the kinds' icons stacked, the summary, how many
/// failed, a spinner while live, the chevron. Click to open.
fn step_chip(
    id: &str,
    steps: &[ToolRow],
    open: bool,
    live: bool,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Stateful<Div> {
    let icons =
        div()
            .flex()
            .items_center()
            .children(
                steps::icons(steps)
                    .into_iter()
                    .enumerate()
                    .map(|(n, icon)| {
                        div()
                            .when(n > 0, |d| d.ml(px(-4.0)))
                            .size(px(20.0))
                            .rounded_full()
                            .fill(Ink::Card, a)
                            .border(px(1.0))
                            .border_color(hsla(Ink::Hairline.color(a)))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(9.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .ink(Ink::TextMuted, a)
                            .child(icon.glyph())
                    }),
            );
    let key = id.to_owned();
    div()
        .id(SharedString::from(format!("chip-{id}")))
        .h(px(28.0))
        .pl(px(4.0))
        .pr(px(10.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .rounded_full()
        .bg(hsla(Ink::Raised.color(a).with_alpha(0.7)))
        .cursor_pointer()
        .hover(|s| s.bg(hsla(Ink::Raised.color(a))))
        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&key, cx)))
        .child(icons)
        .child(label(
            &steps::summary(steps),
            TypeRole::Label,
            Ink::TextMuted,
            a,
        ))
        .children(steps::failed_text(steps).map(|t| label(&t, TypeRole::Label, Ink::Expired, a)))
        .when(live, |d| d.child(spinner(id, a)))
        .child(chevron(open, a))
}

/// A run of calls: the chip, and when open each call on its own line beside a hairline.
fn step_group(
    view: &ChatView,
    id: &str,
    steps: &[ToolRow],
    live: bool,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Div {
    let open = live || view.is_open(id);
    div()
        .flex()
        .flex_col()
        .items_start()
        .child(step_chip(id, steps, open, live, a, cx))
        .when(open, |d| {
            d.child(
                div()
                    .w_full()
                    .mt(px(8.0))
                    .pl(px(12.0))
                    .border_l(px(1.0))
                    .border_color(hsla(Ink::Hairline.color(a)))
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .children(steps.iter().map(|step| step_row(view, step, live, a, cx))),
            )
        })
        .part_at("Tool Steps", Corner::TopTrailing, a, cx)
}

/// "Thought for 16s" (`ThinkingRow`): faint, with the text under it when opened. Claude Code keeps
/// the text only sometimes; without it there is nothing to open.
fn thinking(
    view: &ChatView,
    id: &str,
    title: &str,
    text: &str,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Div {
    let has_text = !text.is_empty();
    let open = has_text && view.is_open(id);
    let key = id.to_owned();
    div()
        .flex()
        .flex_col()
        .items_start()
        .gap(px(6.0))
        .child(
            div()
                .id(SharedString::from(format!("think-{id}")))
                .flex()
                .items_center()
                .gap(px(6.0))
                .when(has_text, |d| {
                    d.cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle(&key, cx)))
                })
                .child(glyph(Icon::Thinking, 10.0, Ink::TextFaint, a))
                .child(label(title, TypeRole::Label, Ink::TextFaint, a))
                .when(has_text, |d| d.child(chevron(open, a)))
                .part_at("Thinking Row", Corner::Trailing, a, cx),
        )
        .when(open, |d| {
            d.child(
                div()
                    .w_full()
                    .flex()
                    .items_start()
                    .gap(px(10.0))
                    .child(div().flex_shrink_0().w(px(2.0)).fill(Ink::Hairline, a))
                    .child(
                        label(text, TypeRole::Meta, Ink::TextMuted, a)
                            .flex_1()
                            .min_w(px(0.0)),
                    ),
            )
        })
}

fn user_row(text: &str, images: usize, queued: bool, a: Appearance, cx: &Context<ChatView>) -> Div {
    // The Swift names: a sent message is "Your Message", one waiting for the turn "Queued Bubble".
    let name = if queued {
        "Queued Bubble"
    } else {
        "Your Message"
    };
    div()
        .w_full()
        .flex()
        .flex_col()
        .items_end()
        .gap(px(2.0))
        .when(queued, |d| {
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
                .when(queued, |d| d.opacity(0.6))
                .child(text.to_owned())
                .part_at(name, Corner::TopTrailing, a, cx),
        )
        .when(images > 0, |d| {
            d.child(label(
                &format!("{images} image{}", if images == 1 { "" } else { "s" }),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            ))
        })
}

/// A row, drawn. `id` is the item's (the queued bubbles at the end have none), `live` whether this
/// is the newest step group of a working session.
fn row_element(
    view: &ChatView,
    row: &Row,
    id: &str,
    live: bool,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Div {
    match row {
        Row::User {
            text,
            images,
            queued,
        } => user_row(text, *images, *queued, a, cx),
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
                .child(markdown::render(id, text, a))
                .part("Claude's Reply", a, cx),
        ),
        Row::Thinking { title, text } => div()
            .pl(px(INDENT))
            .child(thinking(view, id, title, text, a, cx)),
        Row::Tools(tools) => div()
            .pl(px(INDENT))
            .child(step_group(view, id, tools, live, a, cx)),
        Row::TurnEnd { text } => div()
            .w_full()
            .py(px(2.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(hairline(a))
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .child(glyph(Icon::Clock, 9.0, Ink::TextFaint, a))
                    .child(label(text, TypeRole::Meta, Ink::TextFaint, a)),
            )
            .child(hairline(a))
            .part_at("Turn Footer", Corner::Trailing, a, cx),
        Row::Notice { text, icon } => div()
            .w_full()
            .flex()
            .justify_center()
            .items_center()
            .gap(px(6.0))
            .child(glyph(*icon, 11.0, Ink::TextMuted, a))
            .child(label(text, TypeRole::Meta, Ink::TextMuted, a))
            .part_at("Notice", Corner::Leading, a, cx),
        Row::CommandOutput { text } => div()
            .pl(px(INDENT))
            .child(markdown::mono_box(
                &format!("{id}-cmd"),
                "Output",
                text,
                false,
                a,
            ))
            .part_at("Command Output", Corner::TopTrailing, a, cx),
    }
}

/// A row with its Second Opinion buttons and the cards of the opinions asked under it, when the
/// item has something to review.
fn row_with_opinions(
    view: &ChatView,
    i: usize,
    row: &Row,
    live: bool,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Div {
    let item = view.conversation.items.get(i);
    let id = item.map(|item| item.id.as_str()).unwrap_or("queued");
    let element = row_element(view, row, id, live, a, cx);
    let Some(item) = item else {
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
    // The newest step group of a working session stays open with its running call spinning (the
    // Swift app's `liveTools`): the session works, and the last item is that group.
    let working = view
        .selected_session()
        .is_some_and(|s| s.agent.group == AgentGroup::Working);
    let last_tools = view.conversation.items.len().checked_sub(1).filter(|_| {
        working
            && matches!(
                rows.get(view.conversation.items.len() - 1),
                Some(Row::Tools(_))
            )
    });
    column.children(
        rows.iter()
            .enumerate()
            .map(|(i, row)| row_with_opinions(view, i, row, last_tools == Some(i), a, cx)),
    )
}
