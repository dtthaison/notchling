use gpui::{div, prelude::*, px, Context, Div};
use notchling_engine::sessions::Agent;
use notchling_kit::labels::Corner;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{icon_button, status_pill, KitPart, KitStyled};
use notchling_kit::{Icon, Ink, TypeRole};
use notchling_locale as t;

use super::cards::{text, A};
use super::layout::NotchState;
use super::model::{ago, status};
use super::view::NotchView;

impl NotchView {
    pub(super) fn header(&self, agent: &Agent, cx: &mut Context<Self>) -> Div {
        let group_status = status(agent.group);
        let sub = t::notch::detail_sub(
            &agent.label,
            &agent.host,
            &ago(agent.since_ms, self.model.now_ms),
        );
        div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                icon_button(
                    "notch-back",
                    Icon::ChevronLeft,
                    ButtonKind::Ghost,
                    PartState::Rest,
                    A,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.model.go(NotchState::List);
                    cx.notify();
                })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .gap(px(1.0))
                    .child(text(agent.name.clone(), TypeRole::Heading, Ink::Text).elide())
                    .child(text(sub, TypeRole::Meta, Ink::TextMuted).elide()),
            )
            .child(status_pill(group_status, group_status.word(), A))
            .children(self.reload_button(agent, cx))
            .child(
                icon_button(
                    "notch-close",
                    Icon::Close,
                    ButtonKind::Ghost,
                    PartState::Rest,
                    A,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.model.go(NotchState::Collapsed);
                    cx.notify();
                }))
                .part_at("Hide Button", Corner::BottomTrailing, A, cx),
            )
            .part_at("Notch Detail Header", Corner::Bottom, A, cx)
    }
}
