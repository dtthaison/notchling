//! The conversation column: the rows of `chat::rows`, drawn with the kit the way the Swift app's
//! `ChatParts` draws them. A run of tool calls is one chip ("Ran 4 commands, read 1 file", the
//! kinds' icons, a red failure count, a spinner while one runs, a chevron) that opens to a line
//! per call, each opening to its command, output or diff; a thought is one faint line that opens
//! to its text; a reply is Markdown; a turn ends in "Worked for 48s · Opus 5.5" between hairlines.
//! What is open is `ChatView::expanded`; the newest group of a working session is always open.

use gpui::{
    div, point, prelude::*, px, relative, AnyElement, BoxShadow, Context, Div, SharedString,
    Stateful,
};
use notchling_engine::sessions::agent::AgentGroup;
use notchling_kit::labels::Corner;
use notchling_kit::metric::SPINNER_MINI;
use notchling_kit::metric::{RADIUS_CARD, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, button_with, claude_mark, hsla, icon, label, spinner as kit_spinner, KitPart, KitStyled,
};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::steps_view::{hairline, step_group, thinking};
use super::{markdown, message_actions, opinions, ChatView};
use crate::chat::activity::Activity;
use crate::chat::data::Tool;
use crate::chat::rows::{rows, with_outgoing, Row};

/// Rows sit this far apart (the Swift app's conversation stack).
const ROW_GAP: f32 = 14.0;
/// Above the first row and below the last.
const COLUMN_PADDING_VERTICAL: f32 = 20.0;

/// The chevron that opens and closes a row: right when folded, down when open. `points` is the
/// Swift app's symbol size (9 on steps, 8 on a thought).
pub(super) fn chevron(open: bool, points: f32, a: Appearance) -> Div {
    let shape = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    icon(shape, points, Ink::TextFaint, a)
}

/// The live line after the last row (the Swift app's live row): a spinner, what the session is
/// doing, "(50s · ↓ 2.9k tokens)" faint after it, and any background command still running. Its
/// spinner's frames draw it again, so its clock moves with them.
/// The activity line, just above the reply box: one line, its words cut with an ellipsis before
/// the counts beside them. A prompt with nothing back for a while (`stalled`) gets "No reply
/// yet" and Retry, which stops the stuck turn and sends the prompt again. With a background
/// command still running (`shell`: its name and output file), the line is a link to its output.
pub(super) fn activity_row(
    activity: &Activity,
    stalled: bool,
    shell: Option<(String, String)>,
    a: Appearance,
    cx: &mut Context<ChatView>,
) -> Div {
    let retry = stalled.then(|| {
        crate::mac::gate::tip_text(
            button(
                "activity-retry",
                "Retry",
                ButtonKind::Secondary,
                PartState::Rest,
                a,
            )
            .h(px(22.0))
            .px(px(10.0)),
            "Nothing came back: stop this turn and send the message again".into(),
            a,
        )
        .flex_shrink_0()
        .on_click(cx.listener(|this, _, _, cx| this.retry_stalled(cx)))
    });
    let open_output = shell.map(|(name, path)| {
        div()
            .id("activity-output")
            .flex_shrink_0()
            .cursor_pointer()
            .child(label("Show Output", TypeRole::Meta, Ink::AccentText, a))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_shell_output(&name, &path, cx);
            }))
    });
    let row = div()
        .w_full()
        .min_w(px(0.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(kit_spinner("spin-activity", SPINNER_MINI, Ink::Working, a).flex_shrink_0())
        .child(
            label(&activity.text, TypeRole::Meta, Ink::TextMuted, a)
                .flex_shrink()
                .min_w(px(0.0))
                .elide(),
        )
        .children(activity.stats.as_ref().map(|stats| {
            label(&format!("({stats})"), TypeRole::Meta, Ink::TextFaint, a).flex_shrink_0()
        }))
        .children(activity.shells.as_ref().map(|shells| {
            label(&format!("· {shells}"), TypeRole::Meta, Ink::TextFaint, a).flex_shrink_0()
        }))
        .when(stalled, |row| {
            row.child(label("· No reply yet", TypeRole::Meta, Ink::Needs, a).flex_shrink_0())
        })
        .children(open_output)
        .children(retry);
    row.part_at("Activity Line", Corner::Trailing, a, cx)
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

/// Your message: `split` is its words and the pictures it sent (`drops::split_images`).
fn user_row(
    id: &str,
    split: (String, Vec<std::path::PathBuf>),
    images: usize,
    queued: bool,
    sending: bool,
    a: Appearance,
    cx: &Context<ChatView>,
) -> Div {
    // The Swift names: a sent message is "Your Message", one waiting for the turn "Queued Bubble".
    let name = match (queued, sending) {
        (_, true) => "Sending Bubble",
        (true, false) => "Queued Bubble",
        (false, false) => "Your Message",
    };
    let note = if sending { "Sending…" } else { "Queued" };
    let (words, pictures) = split;
    // Under the bubble where its delivered mark, time and Copy go once it's in (same gray, same
    // row height), so nothing moves when it lands.
    let status = queued.then(|| {
        div()
            .h(px(super::message_actions::ROW_HEIGHT))
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(icon(Icon::Clock, 13.0, Ink::TextFaint, a))
            .child(label(note, TypeRole::Meta, Ink::TextFaint, a).px(px(4.0)))
    });
    div()
        .w_full()
        .flex()
        .flex_col()
        .items_end()
        .gap(px(2.0))
        .when(!pictures.is_empty(), |d| {
            d.child(
                div()
                    .max_w(relative(0.75))
                    .flex()
                    .flex_wrap()
                    .justify_end()
                    .gap(px(6.0))
                    .when(queued, |d| d.opacity(0.6))
                    .children(pictures.iter().map(|p| sent_picture(id, p, a))),
            )
        })
        .when(!words.is_empty() || pictures.is_empty(), |d| {
            d.child(
                div()
                    .max_w(relative(0.75))
                    .px(px(14.0))
                    .py(px(10.0))
                    .rounded(px(RADIUS_CARD))
                    .fill(Ink::AccentWash, a)
                    .kit_type(TypeRole::Body)
                    .ink(Ink::Text, a)
                    .when(queued, |d| d.opacity(0.6))
                    .child(super::markdown::plain(&format!("{id}-mine"), &words, a))
                    .part_at(name, Corner::TopTrailing, a, cx),
            )
        })
        .when(images > 0, |d| {
            d.child(label(
                &format!("{images} image{}", if images == 1 { "" } else { "s" }),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            ))
        })
        .children(status)
}

/// How tall a picture you sent shows over your words.
const SENT_PICTURE: f32 = 120.0;

/// A picture you sent, over your bubble; a click opens it.
fn sent_picture(id: &str, path: &std::path::Path, a: Appearance) -> impl IntoElement {
    let text = path.to_string_lossy().into_owned();
    let (w, h) = super::markdown::image_shape(&text).unwrap_or((4, 3));
    let width = (SENT_PICTURE * w as f32 / h.max(1) as f32).clamp(60.0, 320.0);
    let to = text.clone();
    div()
        .id(SharedString::from(format!("sent-pic-{id}-{text}")))
        .w(px(width))
        .h(px(SENT_PICTURE))
        .rounded(px(RADIUS_CARD))
        .border(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .overflow_hidden()
        .cursor_pointer()
        .on_click(move |_, window, cx| super::page_panel::follow_link(&to, window, cx))
        .child(
            gpui::img(path.to_path_buf())
                .size_full()
                .object_fit(gpui::ObjectFit::Cover),
        )
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
            sending,
        } => {
            // The pictures you sent show as pictures, their paths taken out of the words.
            let split =
                crate::chat::drops::split_images(text, |p| view.markdown.borrow().is_file(p));
            user_row(id, split, *images, *queued, *sending, a, cx)
        }
        // No mark before each reply: the bubble alone says who wrote it. Its edge is the column's
        // left edge, which the thoughts, the steps and the reply box share.
        Row::Assistant { text } => {
            let folder = view
                .selected_session()
                .map(|s| s.agent.cwd.as_str())
                .unwrap_or("");
            div().w_full().flex().items_start().child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .px(px(14.0))
                    .py(px(12.0))
                    .rounded(px(RADIUS_CARD))
                    .fill(Ink::Card, a)
                    .border(px(1.0))
                    .border_color(hsla(Ink::Hairline.color(a)))
                    .child(markdown::render(
                        id,
                        &view.markdown.borrow_mut().message(id, text, folder),
                        a,
                    ))
                    .part("Claude's Reply", a, cx),
            )
        }
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
) -> AnyElement {
    let item = view.shown_conversation().items.get(i);
    let id = item.map(|item| item.id.as_str()).unwrap_or("queued");
    let element = row_element(view, row, id, live, a, cx);
    // A subagent's transcript is only read: no second opinions on it.
    let subjects = item
        .filter(|_| view.open_subagent.is_none())
        .map(opinions::subjects)
        .unwrap_or_default();
    let anchors: Vec<String> = subjects.iter().map(|(anchor, _)| anchor.clone()).collect();
    // The Second Opinion buttons show on hover, at the right end of the message's row of actions.
    let buttons = (!subjects.is_empty()).then(|| {
        let of = subjects.len();
        div()
            .min_w(px(0.0))
            .flex()
            .flex_wrap()
            .justify_end()
            .gap(px(4.0))
            .children(
                subjects
                    .into_iter()
                    .map(|(anchor, subject)| opinions::opinion_button(&anchor, subject, of, a, cx)),
            )
    });
    // Your messages and Claude's replies get their row of actions beneath.
    let element = message_actions::with_actions(view, row, item, element, buttons, a, cx);
    if anchors.is_empty() {
        return element;
    }
    let cards = view
        .selected
        .as_deref()
        .and_then(|session| opinions::cards(session, &anchors, a, cx));
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(element)
        .children(cards)
        .into_any_element()
}

pub fn render(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Stateful<Div> {
    let shown = view.shown_conversation();
    // What was typed to the session isn't waiting in a subagent's transcript.
    let outgoing = if view.open_subagent.is_some() {
        Vec::new()
    } else {
        view.outgoing_texts()
    };
    let rows = with_outgoing(rows(shown), &outgoing);
    let column = div()
        .id("conversation")
        .flex_1()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .track_scroll(&view.conversation_scroll)
        .px(px(SIDE_PADDING))
        .pb(px(COLUMN_PADDING_VERTICAL))
        // The Tasks bar sits over the top, so the first row starts under it.
        .pt(px(if super::tasks::shown(view) {
            super::tasks::INSET + COLUMN_PADDING_VERTICAL
        } else {
            COLUMN_PADDING_VERTICAL
        }))
        .flex()
        .flex_col()
        .gap(px(ROW_GAP))
        // First, so each frame's texts are noted afresh before the rows place theirs.
        .child(super::select::listener());
    if let Some(open) = &view.open_subagent {
        if open.conversation.is_none() {
            return column.child(label("Loading…", TypeRole::Meta, Ink::TextMuted, a));
        }
    }
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
            _ if view.open_subagent.is_some() => "No transcript yet for this subagent",
            _ => "No transcript yet for this session",
        };
        return column.child(label(note, TypeRole::Meta, Ink::TextMuted, a));
    }
    // The newest step group of a working session stays open with its running call spinning (the
    // Swift app's `liveTools`): the session works, and the last item is that group.
    let working = match &view.open_subagent {
        Some(open) => open.agent.is_running(),
        None => {
            view.selected_held_working()
                || view
                    .selected_session()
                    .is_some_and(|s| s.agent.group == AgentGroup::Working)
        }
    };
    let last_tools = crate::chat::activity::live_group(shown, working);
    column.children(
        rows.iter()
            .enumerate()
            .map(|(i, row)| row_with_opinions(view, i, row, last_tools == Some(i), a, cx)),
    )
}
