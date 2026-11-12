//! The notch's Antigravity permission card (`AgyPermissionCard`): the shared card from
//! `tools::view` with Antigravity's own options, answered through `agy.permission` by the row's
//! id, the rpc id and the option picked. The daemon resolves the held prompt for Chat and the notch
//! alike, so answering in one clears the other.

use gpui::{div, prelude::*, px, Context, Div};
use notchling_kit::metric::OPTION_BADGE;
use notchling_kit::TypeRole;

use super::answering::{status_height, GAP, HEADER};
use super::cards::{line, A};
use super::requests::HookRequest;
use super::view::NotchView;
use crate::chat::prompt_flow::Sending;
use crate::tools::agy_permission::{AgyPermission, PREVIEW_LINES};
use crate::tools::requests::agy_permission;
use crate::tools::view::agy_card;

/// The kit's option row: 10 padding around the title over its detail, or the 22 badge.
fn option_height() -> f32 {
    let text = line(TypeRole::Heading) + 2.0 + line(TypeRole::Meta);
    text.max(OPTION_BADGE) + 20.0
}

/// The card's drawn height: header, the title line, the preview box, the options, the status.
pub fn agy_permission_height(agy: &AgyPermission, status: Option<&str>) -> f32 {
    let lines = agy.preview(PREVIEW_LINES).len() as f32;
    let preview = lines * (line(TypeRole::Mono) + 1.0) + 16.0;
    HEADER
        + GAP
        + line(TypeRole::Heading)
        + GAP
        + preview
        + GAP
        + (option_height() + GAP) * agy.options.len() as f32
        + status_height(status)
}

impl NotchView {
    /// The card for a row whose held prompt is Antigravity's.
    pub(super) fn agy_permission_card(
        &self,
        request: &HookRequest,
        agy: &AgyPermission,
        agent: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Div {
        let gate = self.gate();
        let (session, rpc) = (request.session_id.clone(), agy.rpc_id.clone());
        let prompt = request.id.clone();
        let card = agy_card(agy, agent, gate, A, cx, move |this, option, cx| {
            let request = agy_permission(&session, &rpc, Some(option));
            this.send_for(Some(prompt.clone()), request, Sending::Answer, cx);
        });
        div()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(card)
            .children(self.card_status(gate))
    }
}
