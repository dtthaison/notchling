//! The slim header over a subagent's transcript, which Chat shows read-only in place of the
//! conversation: the subagent's description, the detail line its Details Panel row shows, and a
//! Back button (Esc and ⌘[ do the same). There is no reply box under it.

use gpui::{div, prelude::*, px, Context, Div};
use notchling_engine::transcript::model::Conversation;
use notchling_kit::labels::Corner;
use notchling_kit::metric::SIDE_PADDING;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button_with, hsla, icon, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::ChatView;
use crate::chat::subagent_view::header_text;

/// The header, while a subagent is open.
pub fn header(view: &ChatView, a: Appearance, cx: &mut Context<ChatView>) -> Option<Div> {
    let open = view.open_subagent.as_ref()?;
    let (title, detail) = header_text(&open.agent);
    let back = button_with(
        "subagent-back",
        div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(icon(Icon::ChevronLeft, 10.0, Ink::Text, a))
            .child(label("Back", TypeRole::Label, Ink::Text, a)),
        ButtonKind::Ghost,
        PartState::Rest,
        a,
    )
    .flex_shrink_0()
    .on_click(cx.listener(|this, _, _, cx| {
        this.close_subagent(cx);
    }));
    Some(
        div()
            .w_full()
            .h(px(36.0))
            .flex_shrink_0()
            .px(px(SIDE_PADDING))
            .flex()
            .items_center()
            .gap(px(10.0))
            .border_b(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(back)
            .child(
                label(&title, TypeRole::Label, Ink::Text, a)
                    .flex_shrink()
                    .min_w(px(0.0))
                    .elide(),
            )
            .child(label(&detail, TypeRole::Meta, Ink::TextFaint, a).flex_shrink_0())
            .part_at("Subagent Header", Corner::Trailing, a, cx),
    )
}

impl ChatView {
    /// What the conversation column draws: the open subagent's transcript, else the session's.
    pub(super) fn shown_conversation(&self) -> &Conversation {
        self.open_subagent
            .as_ref()
            .and_then(|o| o.conversation.as_ref())
            .unwrap_or(&self.conversation)
    }

    /// Shows a subagent of the selected session read-only in place of its conversation, read now.
    pub(super) fn open_subagent(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(agent) = crate::chat::subagent_view::find(&self.subagents, id) else {
            return;
        };
        self.open_subagent = Some(crate::chat::subagent_view::Open::new(agent));
        self.scroll_to_end_now(cx);
        self.reload_now(cx);
        cx.notify();
    }

    /// Back to the session's conversation.
    pub(super) fn close_subagent(&mut self, cx: &mut Context<Self>) {
        if self.open_subagent.take().is_some() {
            // The scroll position was the subagent's; the session's conversation opens at its end.
            self.scroll_to_end_now(cx);
            cx.notify();
        }
    }
}
