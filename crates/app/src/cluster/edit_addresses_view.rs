use std::rc::Rc;

use gpui::{div, prelude::*, px, Context, Div, SharedString, Window};
use notchling_kit::metric::SPACE;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, hsla, label, KitPart};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;

use super::add_parts::AddParts;
use super::address_rows_view::{RowEditor, Rows};
use super::addresses_form::{addresses_of, drafts_of};
use super::machine_add::set_addresses;
use super::view::ClusterSheetView;
use crate::accounts::flows::Alert;
use crate::accounts::view_rows::{alert_overlay, Handler};
use notchling_kit::labels::Corner;

const SHEET_WIDTH: f32 = 560.0;
const SHEET_MARGIN: f32 = 60.0;

pub(super) struct EditSheet {
    pub name: String,
    pub user: String,
    pub rows: Vec<RowEditor>,
    pub error: Option<String>,
    pub parts: AddParts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Switching {
    pub machine: String,
    pub wanted: String,
    pub active: String,
}

impl ClusterSheetView {
    pub(super) fn open_edit_addresses(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.edit.is_some() || self.add.sheet.is_some() {
            return;
        }
        let Some(parts) = self.feed.add_source().parts() else {
            return;
        };
        let Some(machine) = parts.registry.all().into_iter().find(|m| m.name == name) else {
            self.note = Some((t::cluster::address_edit_gone(name), true));
            cx.notify();
            return;
        };
        self.via_names = parts
            .names()
            .into_iter()
            .filter(|n| n != name)
            .collect::<Vec<_>>();
        self.edit = Some(EditSheet {
            name: name.to_owned(),
            user: machine.user.clone(),
            rows: Vec::new(),
            error: None,
            parts: parts.clone(),
        });
        let mut rows = Vec::new();
        for draft in drafts_of(&machine.address_list(), &machine.user) {
            rows.push(self.row_new(Rows::Edit, &draft.text, draft.meta, cx));
        }
        if let Some(sheet) = self.edit.as_mut() {
            sheet.rows = rows;
        }
        self.load_tailnets(&parts, cx);
        cx.notify();
    }

    pub(super) fn load_tailnets(&mut self, parts: &AddParts, cx: &mut Context<Self>) {
        let Some(tailscale) = parts.engine.router().tailscale().cloned() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let list = cx
                .background_executor()
                .spawn(async move { tailscale.networks().unwrap_or_default() })
                .await;
            this.update(cx, |view, cx| {
                view.tailnets = list.into_iter().map(|n| n.tailnet).collect();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn edit_cancel(&mut self, cx: &mut Context<Self>) {
        self.edit = None;
        cx.notify();
    }

    pub(super) fn edit_save(&mut self, cx: &mut Context<Self>) {
        let drafts = self.rows_drafts(Rows::Edit, cx);
        let Some(sheet) = self.edit.as_mut() else {
            return;
        };
        let list = match addresses_of(&drafts, &sheet.name, &sheet.user) {
            Ok(list) => list,
            Err(why) => {
                sheet.error = Some(why);
                cx.notify();
                return;
            }
        };
        match set_addresses(&sheet.parts, &sheet.name, list) {
            Ok(()) => {
                let name = sheet.name.clone();
                self.edit = None;
                self.add.forget_list();
                self.check_machine(&name, cx);
            }
            Err(why) => sheet.error = Some(why),
        }
        cx.notify();
    }

    pub(super) fn edit_overlay(
        &self,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let sheet = self.edit.as_ref()?;
        let mut inner = div()
            .flex()
            .flex_col()
            .gap(px(SPACE[1]))
            .child(self.rows_view(Rows::Edit, window, a, cx));
        if let Some(error) = &sheet.error {
            inner = inner.child(label(error, TypeRole::Meta, Ink::Needs, a));
        }
        let body = div()
            .id("edit-addresses-body")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .child(inner);
        let buttons = div()
            .pt(px(SPACE[1]))
            .flex()
            .justify_end()
            .gap(px(SPACE[1]))
            .child(
                button(
                    "edit-addresses-cancel",
                    t::cluster::add_cancel(),
                    ButtonKind::Ghost,
                    PartState::Rest,
                    a,
                )
                .on_click(cx.listener(|this, _, _, cx| this.edit_cancel(cx))),
            )
            .child(
                button(
                    "edit-addresses-save",
                    t::cluster::address_edit_save(),
                    ButtonKind::Primary,
                    PartState::Rest,
                    a,
                )
                .on_click(cx.listener(|this, _, _, cx| this.edit_save(cx))),
            );
        Some(
            div()
                .absolute()
                .size_full()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(hsla(Ink::Shadow.color(a)))
                .child(
                    card(a)
                        .w(px(SHEET_WIDTH))
                        .max_h(px(super::HEIGHT - SHEET_MARGIN))
                        .p(px(SPACE[3]))
                        .flex()
                        .flex_col()
                        .gap(px(SPACE[1]))
                        .child(label(
                            &t::cluster::address_edit_title(&sheet.name),
                            TypeRole::Heading,
                            Ink::Text,
                            a,
                        ))
                        .child(body)
                        .child(buttons)
                        .part_at("Edit Addresses Sheet", Corner::TopTrailing, a, cx),
                ),
        )
    }

    pub(super) fn ask_switch(
        &mut self,
        machine: &str,
        wanted: &str,
        active: &str,
        cx: &mut Context<Self>,
    ) {
        self.switching = Some(Switching {
            machine: machine.to_owned(),
            wanted: wanted.to_owned(),
            active: active.to_owned(),
        });
        cx.notify();
    }

    pub(super) fn switch_alert(&self, a: Appearance, cx: &mut Context<Self>) -> Option<Div> {
        let ask = self.switching.as_ref()?;
        let alert = Alert {
            title: t::cluster::machine_switch_title(&ask.wanted),
            message: t::cluster::machine_switch_note(&ask.active),
            button: t::cluster::machine_switch_confirm().to_owned(),
            destructive: false,
        };
        let on: Handler<Self, bool> = Rc::new(|this, yes, cx| {
            let Some(ask) = this.switching.take() else {
                return;
            };
            if yes {
                this.switch_network(&ask, cx);
            } else {
                cx.notify();
            }
        });
        Some(alert_overlay(&alert, false, a, cx, on))
    }

    fn switch_network(&mut self, ask: &Switching, cx: &mut Context<Self>) {
        if self.busy || !self.feed.is_live() {
            return;
        }
        self.busy = true;
        self.note = None;
        let (feed, source) = (self.feed.clone(), self.feed.add_source());
        let (machine, request) = (
            ask.machine.clone(),
            super::wire::switch_network(&ask.wanted),
        );
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let sent = feed.send(&request);
                    if sent.is_ok() {
                        source.forget_routes();
                        source.check(&machine);
                    }
                    sent
                })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                if let Err(why) = result {
                    view.note = Some((t::cluster::machine_switch_failed(&why), true));
                }
                view.add.forget_list();
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
}

pub(super) fn switch_button_id(name: &str) -> SharedString {
    SharedString::from(format!("machine-switch-{name}"))
}
