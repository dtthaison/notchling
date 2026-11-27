use gpui::{div, prelude::*, px, Context, Div};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, label};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_local_api::ops;
use notchling_locale as t;
use serde_json::{json, Value};

use super::SettingsView;
use crate::acting::Request;
use crate::sheets::sidecar_rows::{self, Shown, CHANNEL_KEY};

#[derive(Default)]
pub struct SidecarUi {
    shown: Option<Shown>,
    busy: bool,
    confirming: bool,
    result: Option<(String, bool)>,
}

impl SettingsView {
    pub(super) fn sidecar_shown(&mut self, shown: Option<Shown>) {
        self.sidecar.shown = shown;
    }

    pub fn ask_sidecar(&mut self, cx: &mut Context<Self>) {
        let actions = self.acts.actions.clone();
        cx.spawn(async move |this, cx| {
            let reply = cx
                .background_executor()
                .spawn(async move {
                    actions.read(&Request {
                        op: ops::AGY_SIDECAR_STATUS,
                        args: json!({}),
                    })
                })
                .await;
            this.update(cx, |view, cx| {
                view.sidecar.shown = Shown::from_reply(&reply);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn set_up_sidecar(&mut self, action: &'static str, cx: &mut Context<Self>) {
        if self.sidecar.busy {
            return;
        }
        self.sidecar.busy = true;
        self.sidecar.confirming = false;
        self.sidecar.result = None;
        let actions = self.acts.actions.clone();
        cx.spawn(async move |this, cx| {
            let reply: Result<Value, String> = cx
                .background_executor()
                .spawn(async move {
                    actions.read(&Request {
                        op: ops::AGY_SIDECAR_SETUP,
                        args: json!({"action": action, "confirm": action == "turn_on"}),
                    })
                })
                .await;
            this.update(cx, |view, cx| {
                view.sidecar.busy = false;
                view.sidecar.result = Some(sidecar_rows::outcome(&reply));
                if let Some(shown) = Shown::from_reply(&reply) {
                    view.sidecar.shown = Some(shown);
                } else {
                    view.ask_sidecar(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn sidecar_section(&self, a: Appearance, cx: &mut Context<Self>) -> Option<Div> {
        let chosen = sidecar_rows::is_chosen(self.values.get(CHANNEL_KEY));
        if !sidecar_rows::visible(chosen, self.sidecar.shown.as_ref()) {
            return None;
        }
        let shown = self.sidecar.shown;
        let state = shown.map_or(sidecar_rows::checking(), |s| s.word());
        let mut buttons = div().flex().items_center().gap(px(6.0));
        if self.sidecar.confirming {
            buttons = buttons
                .child(
                    button(
                        "sidecar-confirm",
                        t::settings::sidecar_confirm(),
                        ButtonKind::Primary,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.set_up_sidecar("turn_on", cx))),
                )
                .child(
                    button(
                        "sidecar-cancel",
                        t::settings::sidecar_cancel(),
                        ButtonKind::Ghost,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sidecar.confirming = false;
                        cx.notify();
                    })),
                );
        } else if !self.sidecar.busy {
            if shown.is_some_and(|s| s.can_turn_on()) {
                buttons = buttons.child(
                    button(
                        "sidecar-turn-on",
                        t::settings::sidecar_turn_on(),
                        ButtonKind::Primary,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.sidecar.confirming = true;
                        this.sidecar.result = None;
                        cx.notify();
                    })),
                );
            }
            if shown.is_some_and(|s| s.can_turn_off()) {
                buttons = buttons.child(
                    button(
                        "sidecar-turn-off",
                        t::settings::sidecar_turn_off(),
                        ButtonKind::Ghost,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.set_up_sidecar("turn_off", cx))),
                );
            }
        }
        let mut body = div()
            .px(px(14.0))
            .py(px(8.0))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .min_h(px(24.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        label(
                            t::settings::antigravity_sidecar(),
                            TypeRole::Body,
                            Ink::Text,
                            a,
                        )
                        .flex_1(),
                    )
                    .child(label(state, TypeRole::Label, Ink::TextMuted, a))
                    .child(buttons),
            );
        if self.sidecar.confirming {
            body = body.child(label(
                t::settings::sidecar_confirm_note(),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            ));
        }
        if let Some((text, failed)) = &self.sidecar.result {
            let ink = if *failed { Ink::Needs } else { Ink::TextFaint };
            body = body.child(label(text, TypeRole::Meta, ink, a));
        }
        body = body.child(label(
            t::settings::sidecar_note(),
            TypeRole::Meta,
            Ink::TextFaint,
            a,
        ));
        Some(card(a).flex().flex_col().child(body))
    }
}
