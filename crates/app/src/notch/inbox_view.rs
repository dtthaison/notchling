use gpui::{div, prelude::*, px, Context, Div, Stateful};
use notchling_kit::metric::SPACE;
use notchling_kit::ui::{KitPart, KitStyled};
use notchling_kit::{Ink, TypeRole};

use super::cards::{text, A};
use super::inbox::badge_text;
use super::view::NotchView;

impl NotchView {
    pub(super) fn inbox_badge(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let label = badge_text(self.model.inbox_unread)?;
        let badge = div()
            .id("notch-inbox-badge")
            .min_w(px(SPACE[3]))
            .h(px(SPACE[3]))
            .px(px(SPACE[0]))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .fill(Ink::Accent, A)
            .child(text(label, TypeRole::Meta, Ink::OnAccent))
            .on_click(cx.listener(|this, _, _, cx| {
                if this.ns_window.is_some() {
                    crate::mac::shell::open_chat_inbox(cx);
                }
            }))
            .part("Inbox Badge", A, cx);
        Some(badge)
    }
}
