use gpui::{div, prelude::*, px, Context, Div};
use notchling_engine::machines::addresses::Route;
use notchling_engine::machines::host_trust::{scan, trust};
use notchling_kit::metric::SPACE;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, label};
use notchling_kit::{Appearance, TypeRole};
use notchling_locale as t;

use super::add_parts::AddParts;
use super::rounds;
use super::ssh_text::trust_lines;
use super::trust::{TrustAsk, TrustFlow, TrustWhere};
use super::view::ClusterSheetView;

impl ClusterSheetView {
    fn trust_parts(&self, at: &TrustWhere) -> Option<AddParts> {
        match at {
            TrustWhere::Tailscale(_) => self.tailscale.parts.clone(),
            TrustWhere::Edit(_) => self.edit.as_ref().map(|s| s.parts.clone()),
        }
    }

    pub(super) fn trust_begin(&mut self, at: TrustWhere, route: Route, cx: &mut Context<Self>) {
        let Some(parts) = self.trust_parts(&at) else {
            return;
        };
        let ask = TrustAsk::begin(at, route.clone(), rounds::next());
        let (round, needs_scan) = (ask.round, ask.needs_scan());
        self.trust = Some(ask);
        cx.notify();
        if !needs_scan {
            return;
        }
        let runner = parts.engine.runner().clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { scan(&*runner, &route) })
                .await;
            this.update(cx, |view, cx| {
                if let Some(ask) = view.trust.as_mut() {
                    ask.scanned(round, result);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn trust_confirm(&mut self, cx: &mut Context<Self>) {
        let Some(ask) = self.trust.as_mut() else {
            return;
        };
        let Some(keys) = ask.confirm() else {
            return;
        };
        let (round, at) = (ask.round, ask.at.clone());
        let Some(parts) = self.trust_parts(&at) else {
            return;
        };
        let home = std::path::PathBuf::from(&parts.this.home);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { trust(&home, &keys) })
                .await;
            this.update(cx, |view, cx| {
                let saved = view
                    .trust
                    .as_mut()
                    .is_some_and(|ask| ask.saved(round, result));
                if saved {
                    view.trust = None;
                    match at {
                        TrustWhere::Tailscale(host) => {
                            if let Some(model) = view.tailscale.model.as_mut() {
                                model.request_test(&host);
                            }
                            view.tailscale_start_lanes(cx);
                        }
                        TrustWhere::Edit(_) => view.edit_test(cx),
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn trust_cancel(&mut self, cx: &mut Context<Self>) {
        self.trust = None;
        cx.notify();
    }

    pub(super) fn trust_block(
        &self,
        at: &TrustWhere,
        indent: f32,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let ask = self.trust.as_ref().filter(|k| k.is_at(at))?;
        let block = div()
            .pl(px(indent))
            .flex()
            .flex_col()
            .gap(px(SPACE[0]))
            .child(
                div().flex().flex_col().gap(px(2.0)).children(
                    trust_lines(&ask.flow)
                        .into_iter()
                        .map(|(text, ink)| label(&text, TypeRole::Meta, ink, a)),
                ),
            );
        let mut buttons = div().flex().gap(px(SPACE[0]));
        match &ask.flow {
            TrustFlow::Review(_) => {
                buttons = buttons
                    .child(
                        button(
                            "trust-confirm",
                            t::cluster::ssh_trust_confirm(),
                            ButtonKind::Primary,
                            PartState::Rest,
                            a,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.trust_confirm(cx))),
                    )
                    .child(
                        button(
                            "trust-cancel",
                            t::cluster::add_cancel(),
                            ButtonKind::Ghost,
                            PartState::Rest,
                            a,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.trust_cancel(cx))),
                    );
            }
            TrustFlow::Scanning | TrustFlow::Failed(_) => {
                buttons = buttons.child(
                    button(
                        "trust-cancel",
                        if matches!(ask.flow, TrustFlow::Scanning) {
                            t::cluster::add_cancel()
                        } else {
                            t::cluster::add_close()
                        },
                        ButtonKind::Ghost,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.trust_cancel(cx))),
                );
            }
            TrustFlow::Saving(_) => {}
        }
        Some(block.child(buttons))
    }
}
