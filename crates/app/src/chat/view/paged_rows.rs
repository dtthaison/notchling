use gpui::{div, prelude::*, px, Div, SharedString, Stateful, WeakEntity};
use notchling_kit::metric::{RADIUS_SM, TOOL_ICON};
use notchling_kit::ui::{hsla, label, tool_icon, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};

use super::inspector::{MARK_BOX, ROW_LINE};
use super::ChatView;
use crate::chat::inspector::SubagentRow;

pub(super) fn subagent_row(
    s: &SubagentRow,
    a: Appearance,
    chat: &WeakEntity<ChatView>,
) -> Stateful<Div> {
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
