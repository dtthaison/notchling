use notchling_locale as t;
use std::cell::Cell;

use gpui::{
    div, point, prelude::*, px, relative, AnyElement, App, BoxShadow, Context, Div, SharedString,
    Stateful,
};
use notchling_kit::labels::Corner;
use notchling_kit::metric::SPINNER_MINI;
use notchling_kit::metric::{RADIUS_CARD, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, button_with, claude_mark, hsla, icon, label, spinner as kit_spinner, KitPart, KitStyled,
};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::conversation_view::{ConversationInput, ConversationState, ConversationView};
use super::markdown::{self, Sel};
use super::row_select::{self, key_of};
use super::steps_view::{hairline, step_group, thinking};
use super::{message_actions, opinions, ChatView};
use crate::chat::activity::Activity;
use crate::chat::rows::Row;

pub(super) const ROW_GAP: f32 = 14.0;
pub(super) const COLUMN_PADDING_VERTICAL: f32 = 20.0;

pub(super) fn chevron(open: bool, points: f32, a: Appearance) -> Div {
    let shape = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    icon(shape, points, Ink::TextFaint, a)
}

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
                t::chat::retry(),
                ButtonKind::Secondary,
                PartState::Rest,
                a,
            )
            .h(px(22.0))
            .px(px(10.0)),
            t::chat::retry_tip().into(),
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
            .child(label(
                t::chat::show_output_button(),
                TypeRole::Meta,
                Ink::AccentText,
                a,
            ))
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
            label(
                &t::chat::activity_shells(shells),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            )
            .flex_shrink_0()
        }))
        .when(stalled, |row| {
            row.child(label(t::chat::no_reply_yet(), TypeRole::Meta, Ink::Needs, a).flex_shrink_0())
        })
        .children(open_output)
        .children(retry);
    row.part_at("Activity Line", Corner::Trailing, a, cx)
}

pub(super) fn latest_pill(
    s: &ConversationState,
    loading: bool,
    a: Appearance,
    cx: &mut Context<ConversationView>,
) -> Option<Div> {
    if !s.latest_pill(loading) {
        return None;
    }
    let content = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(icon(Icon::ArrowDown, 11.0, Ink::Text, a))
        .child(label(t::chat::latest(), TypeRole::Label, Ink::Text, a));
    let pill = button_with("latest", content, ButtonKind::Secondary, PartState::Rest, a)
        .shadow(vec![BoxShadow {
            color: hsla(Ink::Shadow.color(a)),
            offset: point(px(0.0), px(3.0)),
            blur_radius: px(8.0),
            spread_radius: px(0.0),
        }])
        .on_click(cx.listener(|this, _, _, cx| {
            this.s.jump_to_latest();
            cx.notify();
        }));
    Some(
        div()
            .absolute()
            .right(px(20.0))
            .bottom(px(20.0))
            .child(pill)
            .part_at("Latest Button", Corner::Top, a, cx),
    )
}

fn user_row(
    sel: &Sel,
    split: (String, Vec<std::path::PathBuf>),
    images: usize,
    queued: bool,
    sending: bool,
    a: Appearance,
    cx: &App,
) -> Div {
    let id = sel.id();
    let name = crate::chat::markers::bubble_part(queued, sending);
    let note = if sending {
        t::chat::sending_note()
    } else {
        t::chat::queued_note()
    };
    let (words, pictures) = split;
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
                    .child(super::markdown::plain(
                        &format!("{id}-mine"),
                        sel,
                        &words,
                        a,
                    ))
                    .part_at(name, Corner::TopTrailing, a, cx),
            )
        })
        .when(images > 0, |d| {
            d.child(label(
                &t::chat::images(images),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            ))
        })
        .children(status)
}

const SENT_PICTURE: f32 = 120.0;

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

fn row_element(
    input: &ConversationInput,
    s: &ConversationState,
    row: &Row,
    sel: &Sel,
    live: bool,
    a: Appearance,
    cx: &mut Context<ConversationView>,
) -> Div {
    let id = sel.id();
    match row {
        Row::User {
            text,
            images,
            queued,
            sending,
        } => {
            let split = crate::chat::drops::split_images(text, |p| s.markdown.borrow().is_file(p));
            user_row(sel, split, *images, *queued, *sending, a, cx)
        }
        Row::Assistant { text } => {
            let folder = input.session.as_ref().map_or("", |s| s.cwd.as_str());
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
                        sel,
                        &s.markdown.borrow_mut().message(id, text, folder),
                        a,
                        cx,
                    ))
                    .part("Claude's Reply", a, cx),
            )
        }
        Row::Thinking { title, text } => thinking(s, id, title, text, a, cx),
        Row::Tools(tools) => step_group(input, s, sel, tools, live, a, cx),
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
                sel,
                t::chat::output(),
                text,
                false,
                a,
            ))
            .part_at("Command Output", Corner::TopTrailing, a, cx),
    }
}

fn row_with_opinions(
    input: &ConversationInput,
    s: &ConversationState,
    i: usize,
    row: &Row,
    live: bool,
    a: Appearance,
    cx: &mut Context<ConversationView>,
) -> AnyElement {
    super::pane::ROWS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let item = input.shown.items.get(i);
    let key = key_of(&input.shown.items, i, row);
    let next = Cell::new(0);
    let sel = Sel::Row {
        row: &key,
        next: &next,
        state: &s.sel,
        run: input.can_run,
    };
    let element = row_element(input, s, row, &sel, live, a, cx);
    let subjects = item
        .filter(|_| input.subagent.is_none())
        .map(opinions::subjects)
        .unwrap_or_default();
    let anchors: Vec<String> = subjects.iter().map(|(anchor, _)| anchor.clone()).collect();
    let buttons = (!subjects.is_empty()).then(|| {
        let of = subjects.len();
        div()
            .min_w(px(0.0))
            .flex()
            .flex_wrap()
            .justify_end()
            .gap(px(4.0))
            .children(subjects.into_iter().map(|(anchor, subject)| {
                opinions::opinion_button(&anchor, subject, of, a, &input.chat, cx)
            }))
    });
    let element = message_actions::with_actions(s, row, item, element, buttons, a, cx);
    if anchors.is_empty() {
        return element;
    }
    let cards = input
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

pub(super) fn live_group_of(input: &ConversationInput) -> Option<usize> {
    let working = match &input.subagent {
        Some(sub) => sub.running,
        None => input.held_working || input.session.as_ref().is_some_and(|s| s.working),
    };
    crate::chat::activity::live_group(&input.shown, working)
}

pub(super) fn render_row(
    input: &ConversationInput,
    s: &ConversationState,
    i: usize,
    row: &Row,
    live: bool,
    a: Appearance,
    cx: &mut Context<ConversationView>,
) -> AnyElement {
    row_with_opinions(input, s, i, row, live, a, cx)
}

pub(super) fn empty_column(
    input: &ConversationInput,
    s: &ConversationState,
    a: Appearance,
    cx: &mut Context<ConversationView>,
) -> Option<Stateful<Div>> {
    let shown = &*input.shown;
    let empty = shown.items.is_empty()
        && shown.queued.is_empty()
        && input.held.is_none()
        && input.outgoing.is_empty();
    let state = if input.subagent.as_ref().is_some_and(|sub| !sub.loaded) {
        Empty::Loading
    } else if input.loading {
        Empty::Blank
    } else if input.session.is_none() {
        Empty::NoSessions
    } else if empty {
        Empty::Note
    } else {
        return None;
    };
    let column = div()
        .id("conversation")
        .flex_1()
        .min_h(px(0.0))
        .px(px(SIDE_PADDING))
        .pb(px(COLUMN_PADDING_VERTICAL))
        .pt(px(if input.tasks_inset {
            super::tasks::INSET + COLUMN_PADDING_VERTICAL
        } else {
            COLUMN_PADDING_VERTICAL
        }))
        .flex()
        .flex_col()
        .gap(px(ROW_GAP))
        .child(row_select::listener(s.sel.clone(), cx.entity().downgrade()));
    Some(match state {
        Empty::Loading => {
            column.child(label(t::chat::loading(), TypeRole::Meta, Ink::TextMuted, a))
        }
        Empty::Blank => column,
        Empty::NoSessions => column.child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .child(claude_mark(40.0, a))
                .child(label(
                    t::chat::no_sessions_yet(),
                    TypeRole::Heading,
                    Ink::Text,
                    a,
                )),
        ),
        Empty::Note => column.child(label(
            input.empty_note.unwrap_or(t::chat::no_transcript()),
            TypeRole::Meta,
            Ink::TextMuted,
            a,
        )),
    })
}

enum Empty {
    Loading,
    Blank,
    NoSessions,
    Note,
}
