//! The conversation column: the rows of `chat::rows`, drawn with the kit the way the Swift app's
//! `ChatParts` draws them. A run of tool calls is one chip ("Ran 4 commands, read 1 file", the
//! kinds' icons, a red failure count, a spinner while one runs, a chevron) that opens to a line
//! per call, each opening to its command, output or diff; a thought is one faint line that opens
//! to its text; a reply is Markdown; a turn ends in "Worked for 48s · Opus 5.5" between hairlines.
//! What is open is `ChatView::expanded`; the newest group of a working session is always open.

use std::path::Path;

use gpui::{
    div, point, prelude::*, px, relative, BoxShadow, Context, Div, FontWeight, SharedString,
    Stateful,
};
use notchling_engine::sessions::agent::AgentGroup;
use notchling_engine::transcript::model::{DiffKind, DiffLine};
use notchling_kit::labels::Corner;
use notchling_kit::metric::{RADIUS_CARD, RADIUS_MD, RADIUS_SM, SIDE_PADDING};
use notchling_kit::metric::{SPINNER_MINI as MINI, SPINNER_SMALL as SMALL};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, button_with, claude_mark, hsla, icon, label, spinner as kit_spinner, KitPart, KitStyled,
};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::{markdown, opinions, ChatView};
use crate::chat::activity::Activity;
use crate::chat::data::Tool;
use crate::chat::rows::{rows, Row, ToolRow};
use crate::chat::steps::{self, Detail};

/// Rows sit this far apart (the Swift app's conversation stack).
const ROW_GAP: f32 = 14.0;
/// Above the first row and below the last.
const COLUMN_PADDING_VERTICAL: f32 = 20.0;

/// The chevron that opens and closes a row: right when folded, down when open. `points` is the
/// Swift app's symbol size (9 on steps, 8 on a thought).
fn chevron(open: bool, points: f32, a: Appearance) -> Div {
    let shape = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    icon(shape, points, Ink::TextFaint, a)
}

/// The mini spinner on a step group or step that is still running (Swift `controlSize(.mini)`).
fn spinner(key: &str, a: Appearance) -> impl IntoElement {
    kit_spinner(
        SharedString::from(format!("spin-{key}")),
        MINI,
        Ink::Working,
        a,
    )
}

/// The live line after the last row (the Swift app's live row): a spinner, what the session is
/// doing, "(50s · ↓ 2.9k tokens)" faint after it, and any background command still running. The
/// view redraws it once a second while it shows (`ChatView::tick`).
fn activity_row(activity: &Activity, a: Appearance, cx: &Context<ChatView>) -> Div {
    div()
        .w_full()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(kit_spinner("spin-activity", SMALL, Ink::Working, a))
        .child(
            label(&activity.text, TypeRole::Meta, Ink::TextMuted, a)
                .min_w(px(0.0))
                .elide(),
        )
        .children(activity.stats.as_ref().map(|stats| {
            label(&format!("({stats})"), TypeRole::Meta, Ink::TextFaint, a).flex_shrink_0()
        }))
        .children(activity.shells.as_ref().map(|shells| {
            label(&format!("· {shells}"), TypeRole::Meta, Ink::TextFaint, a).flex_shrink_0()
        }))
        .part_at("Activity Line", Corner::Trailing, a, cx)
}

/// The "↓ Latest" pill over the conversation's bottom right, while it is scrolled up (the Swift
/// button: a secondary kit button with an arrow, 20 pt from the corner). It sits in the
/// conversation, above the reply box. A click goes to the newest message and follows again.
pub fn latest_pill(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    if !view.latest_pill() {
        return None;
    }
    let content = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(icon(Icon::ArrowDown, 11.0, Ink::Text, a))
        .child(label("Latest", TypeRole::Label, Ink::Text, a));
    let pill = button_with("latest", content, ButtonKind::Secondary, PartState::Rest, a)
        .shadow(vec![BoxShadow {
            color: hsla(Ink::Shadow.color(a)),
            offset: point(px(0.0), px(3.0)),
            blur_radius: px(8.0),
            spread_radius: px(0.0),
        }])
        .on_click(cx.listener(|this, _, _, cx| this.jump_to_latest(cx)));
    Some(
        div()
            .absolute()
            .right(px(20.0))
            .bottom(px(20.0))
            .child(pill)
            .part_at("Latest Button", Corner::Top, a, cx),
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
                .child(icon(Icon::File, 10.0, Ink::Text, a))
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
                .child(icon(step.icon, 11.0, icon_ink, a)),
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
            d.child(icon(Icon::Failed, 11.0, Ink::Expired, a))
        })
        .when(has_detail, |d| d.child(chevron(open, 9.0, a)))
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
                    .map(|(n, kind)| {
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
                            .child(icon(kind, 9.0, Ink::TextMuted, a))
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
        .child(chevron(open, 9.0, a))
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
                .child(icon(Icon::Thinking, 10.0, Ink::TextFaint, a))
                .child(label(title, TypeRole::Label, Ink::TextFaint, a))
                .when(has_text, |d| d.child(chevron(open, 8.0, a)))
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
                .px(px(14.0))
                .py(px(10.0))
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
        // No mark before each reply: the bubble alone says who wrote it. Its edge is the column's
        // left edge, which the thoughts, the steps and the reply box share.
        Row::Assistant { text } => div().w_full().flex().items_start().child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .px(px(14.0))
                .py(px(12.0))
                .rounded(px(RADIUS_CARD))
                .fill(Ink::Card, a)
                .border(px(1.0))
                .border_color(hsla(Ink::Hairline.color(a)))
                .child(markdown::render(id, text, a))
                .part("Claude's Reply", a, cx),
        ),
        Row::Thinking { title, text } => thinking(view, id, title, text, a, cx),
        Row::Tools(tools) => step_group(view, id, tools, live, a, cx),
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
                    .child(icon(Icon::Clock, 9.0, Ink::TextFaint, a))
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
            .child(notchling_kit::ui::icon(*icon, 10.0, Ink::TextMuted, a))
            .child(label(text, TypeRole::Meta, Ink::TextMuted, a))
            .part_at("Notice", Corner::Leading, a, cx),
        Row::CommandOutput { text } => div()
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
    let buttons = div().flex().gap(px(4.0)).children(
        subjects
            .into_iter()
            .map(|(anchor, subject)| opinions::opinion_button(&anchor, subject, a, cx)),
    );
    let cards = view
        .selected
        .as_deref()
        .and_then(|session| opinions::cards(session, &anchors, a, cx));
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
        .py(px(COLUMN_PADDING_VERTICAL))
        .flex()
        .flex_col()
        .gap(px(ROW_GAP));
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
        return column
            .child(label(note, TypeRole::Meta, Ink::TextMuted, a))
            .children(
                view.activity_line()
                    .map(|activity| activity_row(&activity, a, cx)),
            );
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
    let column = column.children(
        rows.iter()
            .enumerate()
            .map(|(i, row)| row_with_opinions(view, i, row, last_tools == Some(i), a, cx)),
    );
    column.children(
        view.activity_line()
            .map(|activity| activity_row(&activity, a, cx)),
    )
}
