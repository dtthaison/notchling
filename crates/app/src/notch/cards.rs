//! The notch's smaller parts over GPUI: text in a role, the host tag, the conversation's lines and
//! the read-only question and permission cards, with their drawn heights (the model's
//! `card_height`, which beats the estimate as the Swift app's measured height does).

use gpui::{div, prelude::*, px, Div, SharedString};
use notchling_engine::transcript::snapshot::{ChatLine, LineKind, PendingQuestion};
use notchling_kit::metric::{RADIUS_MD, RADIUS_SM};
use notchling_kit::part::PartState;
use notchling_kit::ui::{hsla, option_row, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::requests::HookRequest;

/// The notch is dark in both appearances: it continues the camera housing.
pub const A: Appearance = Appearance::Dark;
/// Every line of text is this many times its font size tall.
pub const LINE: f32 = 1.3;

/// A role's line height in points.
pub fn line(role: TypeRole) -> f32 {
    role.spec().size * LINE
}

/// Text in a type role and an ink.
pub fn text(content: impl Into<SharedString>, role: TypeRole, ink: Ink) -> Div {
    div()
        .kit_type(role)
        .ink(ink, A)
        .line_height(px(line(role)))
        .child(role.display(&content.into()))
}

/// Where a session runs, in a small chip.
pub fn tag(host: &str) -> Div {
    text(host.to_owned(), TypeRole::Meta, Ink::TextMuted)
        .flex_shrink_0()
        .px(px(7.0))
        .py(px(2.0))
        .rounded(px(RADIUS_SM))
        .fill(Ink::Raised, A)
}

/// One conversation entry, laid out like Claude Code's terminal: "❯" prompts on a shaded bar,
/// Claude's text after a "⏺", and dim tool summaries indented beneath it.
pub fn bubble(entry: &ChatLine) -> Div {
    let (mark, mark_ink, ink) = match entry.kind {
        LineKind::User => ("❯", Ink::TextMuted, Ink::Text),
        LineKind::Assistant => ("⏺", Ink::Text, Ink::Text),
        LineKind::Tools => ("⎿", Ink::TextFaint, Ink::TextMuted),
    };
    let row = div()
        .flex()
        .items_start()
        .gap(px(6.0))
        .child(text(mark, TypeRole::Mono, mark_ink).flex_shrink_0())
        .child(
            text(entry.text.clone(), TypeRole::Mono, ink)
                .flex_1()
                .min_w(px(0.0)),
        );
    match entry.kind {
        LineKind::User => row
            .px(px(6.0))
            .py(px(5.0))
            .rounded(px(4.0))
            .fill(Ink::Raised, A),
        LineKind::Assistant => row.px(px(6.0)),
        LineKind::Tools => row.pl(px(18.0)).pr(px(6.0)),
    }
}

/// A card's header: icon and title in an ink, a step count, the session's name when it peeks.
fn header(icon: Icon, title: &str, ink: Ink, step: Option<String>, agent: Option<&str>) -> Div {
    div()
        .h(px(HEADER))
        .flex()
        .items_center()
        .gap(px(7.0))
        .child(text(icon.glyph(), TypeRole::Heading, ink))
        .child(text(title.to_owned(), TypeRole::Heading, ink))
        .children(step.map(|s| text(s, TypeRole::Meta, Ink::TextMuted)))
        .child(div().flex_1())
        .children(
            agent.map(|name| text(name.to_owned(), TypeRole::Meta, Ink::TextMuted).truncate()),
        )
}

/// Says why nothing here can be clicked.
fn read_only_line(host: &str) -> Div {
    let place = if host == "Background" || host.is_empty() {
        "the session's terminal".to_owned()
    } else {
        host.to_owned()
    };
    text(
        format!("Read only here. Answer in {place}."),
        TypeRole::Meta,
        Ink::TextMuted,
    )
}

const HEADER: f32 = 22.0;
const GAP: f32 = 8.0;
/// The kit's option row: 10 padding around a 22 badge, or the title over its detail.
const OPTION_PADDING: f32 = 20.0;
const OPTION_BADGE: f32 = 22.0;
/// Roughly how many characters of a 13 pt line fit across a card in the panel width.
const CHARS_PER_LINE: usize = 52;
/// And of the 12 pt monospaced summary.
const MONO_CHARS_PER_LINE: usize = 48;
/// The permission summary shows at most this many lines (`lineLimit(10)`).
const SUMMARY_LINES: usize = 10;

fn wrapped_lines(chars: usize, per_line: usize) -> f32 {
    (chars / per_line + 1) as f32
}

/// The kit's option row always draws its detail line, empty or not.
fn option_height() -> f32 {
    let text = line(TypeRole::Heading) + 2.0 + line(TypeRole::Meta);
    text.max(OPTION_BADGE) + OPTION_PADDING
}

/// "Claude asks": the first question with its options, disabled; the rest wait their turn.
pub fn question_card(q: &PendingQuestion, host: &str, agent: Option<&str>) -> Div {
    let Some(item) = q.items.first() else {
        return div();
    };
    let step = (q.items.len() > 1).then(|| format!("1 of {}", q.items.len()));
    let options = item.options.iter().enumerate().map(|(i, title)| {
        let detail = item.details.get(i).map(String::as_str).unwrap_or("");
        option_row(
            SharedString::from(format!("option-{i}")),
            i + 1,
            title,
            detail,
            false,
            PartState::Disabled,
            A,
        )
        .line_height(px(line(TypeRole::Heading)))
    });
    div()
        .flex()
        .flex_col()
        .gap(px(GAP))
        .child(header(
            Icon::Question,
            "Claude asks",
            Ink::Working,
            step,
            agent,
        ))
        .child(text(item.question.clone(), TypeRole::Heading, Ink::Text))
        .children(options)
        .child(read_only_line(host))
}

/// The question card's drawn height.
pub fn question_height(q: &PendingQuestion) -> f32 {
    let Some(item) = q.items.first() else {
        return 0.0;
    };
    let question = wrapped_lines(item.question.chars().count(), CHARS_PER_LINE).min(3.0)
        * line(TypeRole::Heading);
    let options = option_height() * item.options.len() as f32;
    // Header, question, the options and the last line, with a gap between each.
    let gaps = 2 + item.options.len();
    HEADER + question + options + line(TypeRole::Meta) + GAP * gaps as f32
}

/// "Permission needed": the tool and what it would run, with Allow, Always allow and Deny disabled.
pub fn permission_card(r: &HookRequest, host: &str, agent: Option<&str>) -> Div {
    let mut titles = vec![("Allow", "This time only")];
    if r.can_always_allow {
        titles.push(("Always allow this", "Don't ask again for this"));
    }
    titles.push(("Deny", "Claude tries something else"));
    let options = titles.into_iter().enumerate().map(|(i, (title, detail))| {
        option_row(
            SharedString::from(format!("permission-{i}")),
            i + 1,
            title,
            detail,
            false,
            PartState::Disabled,
            A,
        )
        .line_height(px(line(TypeRole::Heading)))
    });
    div()
        .flex()
        .flex_col()
        .gap(px(GAP))
        .child(header(
            Icon::Permission,
            "Permission needed",
            Ink::AccentText,
            None,
            agent,
        ))
        .child(text(
            format!("Claude wants to use {}", r.tool_name),
            TypeRole::Heading,
            Ink::Text,
        ))
        .child(
            text(r.summary.clone(), TypeRole::Mono, Ink::Text)
                .line_clamp(SUMMARY_LINES)
                .p(px(8.0))
                .rounded(px(RADIUS_MD))
                .fill(Ink::Raised, A),
        )
        .children(options)
        .child(read_only_line(host))
}

/// The permission card's drawn height.
pub fn permission_height(r: &HookRequest) -> f32 {
    let lines = wrapped_lines(r.summary.chars().count(), MONO_CHARS_PER_LINE)
        .min(SUMMARY_LINES as f32)
        + r.summary.matches('\n').count() as f32;
    let summary = lines.min(SUMMARY_LINES as f32) * line(TypeRole::Mono) + 16.0;
    let count = if r.can_always_allow { 3 } else { 2 };
    HEADER
        + line(TypeRole::Heading)
        + summary
        + option_height() * count as f32
        + line(TypeRole::Meta)
        + GAP * (3 + count) as f32
}

/// "Waiting on you": what a session needs, when no card says it.
pub fn needs_box(needs: &str) -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(8.0))
        .p(px(10.0))
        .rounded(px(RADIUS_MD))
        .bg(hsla(Ink::Needs.color(A).with_alpha(0.15)))
        .child(text(Icon::Needs.glyph(), TypeRole::Body, Ink::Needs))
        .child(
            text(needs.to_owned(), TypeRole::Body, Ink::Text)
                .flex_1()
                .min_w(px(0.0)),
        )
}
