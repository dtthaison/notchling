use gpui::{div, prelude::*, px, Context, Div, SharedString};
use notchling_kit::labels::Corner;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::typescale::sentence;
use notchling_kit::ui::{button, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;
use serde_json::Value;

use super::ChatView;
use crate::acting::Request;
use crate::chat::composer::can_resume_session;
use crate::mac::gate;

pub(super) fn footer_status(
    view: &ChatView,
    a: Appearance,
    cx: &mut Context<ChatView>,
    fallback: impl FnOnce(&Context<ChatView>) -> Option<Div>,
) -> Option<Div> {
    if !view.selected_resumable() {
        return fallback(cx);
    }
    let plain_exited = view.bring_back_note().is_none()
        && view.tools.error.is_none()
        && view
            .tools
            .agy_note
            .as_ref()
            .filter(|(row, _)| Some(row) == view.selected.as_ref())
            .is_none()
        && view.chrome_note().is_none()
        && view.reload_note().is_none()
        && view.offline_note().is_none()
        && !view.tools.sending
        && view
            .selected
            .as_deref()
            .map_or(0, |id| view.tools.live.waiting(id))
            == 0;
    let line = if plain_exited {
        Some(
            label(
                &sentence(t::chat::session_exited()),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            )
            .elide()
            .part_at("Composer Hint", Corner::Trailing, a, cx),
        )
    } else {
        fallback(cx)
    };
    let acting = view.acting_in(cx).control();
    let live = acting.enabled && !view.tools.sending;
    let b = button(
        "session-resume",
        t::chat::resume(),
        ButtonKind::Secondary,
        if live {
            PartState::Rest
        } else {
            PartState::Disabled
        },
        a,
    )
    .flex_shrink_0();
    let b = match acting.tooltip {
        Some(tip) if !acting.enabled => gate::tip_text(b, SharedString::from(tip), a),
        _ => b,
    };
    let b = if live {
        b.on_click(cx.listener(|this, _, _, cx| this.resume_selected(cx)))
    } else {
        b
    };
    Some(
        div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(8.0))
            .children(line.map(|line| div().min_w(px(0.0)).overflow_hidden().child(line)))
            .child(b.part_at("Resume Button", Corner::Top, a, cx)),
    )
}

impl ChatView {
    pub(super) fn selected_resumable(&self) -> bool {
        self.selected_session().is_some_and(can_resume_session)
    }

    pub(super) fn resume_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected.clone() {
            self.resume_row(id, cx);
        }
    }

    pub(super) fn resume_row(&mut self, id: String, cx: &mut Context<Self>) {
        self.row_menu = None;
        let resumable = self
            .sessions
            .iter()
            .find(|s| s.agent.id == id)
            .is_some_and(can_resume_session);
        if self.tools.sending || !resumable || !self.acting(cx).control().enabled {
            cx.notify();
            return;
        }
        self.spawn_send(
            Request::resume(&id),
            |view, result, cx| match result {
                Ok(reply) => {
                    if let Some(job) = reply
                        .pointer("/result/jobId")
                        .or_else(|| reply.get("jobId"))
                        .and_then(Value::as_str)
                        .filter(|j| !j.is_empty())
                    {
                        view.show_started_job(job, cx);
                    }
                    view.reload_now(cx);
                }
                Err(message) => view.tools.error = Some(message),
            },
            cx,
        );
    }
}
