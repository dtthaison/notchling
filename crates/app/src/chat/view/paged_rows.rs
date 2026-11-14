use gpui::{div, prelude::*, px, AnyElement, App, Div, SharedString, Stateful, WeakEntity};
use notchling_kit::metric::{RADIUS_SM, TOOL_ICON};
use notchling_kit::ui::{hsla, icon, label, tool_icon, KitPart, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::inspector::{MARK_BOX, ROW_LINE};
use super::ChatView;
use crate::chat::inspector::SubagentRow;

const INDENT: f32 = 14.0;

fn indented(depth: u8, row: impl IntoElement) -> AnyElement {
    if depth > 1 {
        div()
            .pl(px(INDENT * f32::from(depth - 1)))
            .child(row)
            .into_any_element()
    } else {
        row.into_any_element()
    }
}

fn disclosure(
    s: &SubagentRow,
    a: Appearance,
    chat: &WeakEntity<ChatView>,
    cx: &App,
) -> Stateful<Div> {
    let (chat, group) = (chat.clone(), s.id.clone());
    let chevron = if s.open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    div()
        .id(SharedString::from(format!("subagent-group-{}", s.id)))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(2.0))
        .h(px(ROW_LINE))
        .cursor_pointer()
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            chat.update(cx, |this, cx| this.toggle_group(&group, cx))
                .ok();
        })
        .child(label(
            &s.children.to_string(),
            TypeRole::Meta,
            Ink::TextFaint,
            a,
        ))
        .child(icon(chevron, 10.0, Ink::TextFaint, a))
        .part("Subagent Group", a, cx)
}

pub(super) fn subagent_row(
    s: &SubagentRow,
    a: Appearance,
    chat: &WeakEntity<ChatView>,
    cx: &App,
) -> AnyElement {
    let depth = s.depth;
    let row = plain_row(s, a, chat);
    let row = if s.children > 0 {
        row.child(disclosure(s, a, chat, cx))
    } else {
        row
    };
    indented(depth, row)
}

pub(super) fn group_more_row(
    group: &str,
    next: usize,
    depth: u8,
    chat: &WeakEntity<ChatView>,
    a: Appearance,
) -> AnyElement {
    let (chat, group) = (chat.clone(), group.to_owned());
    let row = div()
        .id(SharedString::from(format!("group-more-{group}")))
        .cursor_pointer()
        .child(label(
            &t::chat::details_load_more(&next.to_string()),
            TypeRole::Meta,
            Ink::AccentText,
            a,
        ))
        .on_click(move |_, _, cx| {
            chat.update(cx, |this, cx| this.load_more_under(&group, cx))
                .ok();
        });
    indented(depth.saturating_add(1), row)
}

fn plain_row(s: &SubagentRow, a: Appearance, chat: &WeakEntity<ChatView>) -> Stateful<Div> {
    let (chat, agent) = (chat.clone(), s.id.clone());
    div()
        .id(SharedString::from(format!("subagent-{}", s.id)))
        .mx(px(-4.0))
        .px(px(4.0))
        .py(px(2.0))
        .rounded(px(RADIUS_SM))
        .cursor_pointer()
        .hover(|style| style.bg(hsla(Ink::Raised.color(a).with_alpha(0.6))))
        .on_click(move |_, _, cx| {
            chat.update(cx, |this, cx| this.open_subagent(&agent, cx))
                .ok();
        })
        .flex()
        .items_start()
        .gap(px(7.0))
        .child(
            div()
                .w(px(MARK_BOX))
                .h(px(ROW_LINE))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(tool_icon(s.tool, TOOL_ICON, a)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    label(&s.description, TypeRole::Meta, Ink::Text, a).line_height(px(ROW_LINE)),
                )
                .child(
                    label(&s.detail, TypeRole::Meta, Ink::TextFaint, a).line_height(px(ROW_LINE)),
                )
                .children(s.doing.as_ref().map(|doing| {
                    label(doing, TypeRole::Meta, Ink::TextMuted, a)
                        .line_height(px(ROW_LINE))
                        .w_full()
                        .elide()
                })),
        )
}

#[derive(Clone, Copy)]
pub(super) enum More {
    Subagents,
    Shells,
}

pub(super) fn more_row(which: More, text: &str, chat: &WeakEntity<ChatView>, a: Appearance) -> Div {
    let chat = chat.clone();
    let key = match which {
        More::Subagents => "subagents-more",
        More::Shells => "shells-more",
    };
    div().child(
        div()
            .id(key)
            .cursor_pointer()
            .child(label(text, TypeRole::Meta, Ink::AccentText, a))
            .on_click(move |_, _, cx| {
                chat.update(cx, |this, cx| match which {
                    More::Subagents => this.load_more_subagents(cx),
                    More::Shells => this.load_more_shells(cx),
                })
                .ok();
            }),
    )
}
