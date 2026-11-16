use std::path::Path;
use std::rc::Rc;

use gpui::{div, prelude::*, px, Context, Div, SharedString, Window};
use notchling_engine::pages::clock::local_offset_secs;
use notchling_kit::metric::SPACE;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, icon, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::model::ClusterModel;
use super::promote::{Ask, CopyRow, PromoteSheet, WaitRow};
use super::view::ClusterSheetView;
use super::wire::Request;
use crate::accounts::flows::Alert;
use crate::accounts::view_rows::{alert_overlay, Handler};
use crate::phone::bridge::now_ms;
use crate::sheets::view::Dismiss;

fn heading(text: &str, a: Appearance) -> Div {
    div().child(label(text, TypeRole::Label, Ink::TextFaint, a))
}

fn note_icon(a: Appearance) -> gpui::AnyElement {
    icon(Icon::Warning, TypeRole::Meta.spec().size, Ink::Needs, a).into_any_element()
}

fn alert_of(ask: Ask) -> Alert {
    Alert {
        title: ask.title,
        message: ask.message,
        button: ask.confirm,
        destructive: false,
    }
}

impl ClusterSheetView {
    pub(super) fn now(&self) -> i64 {
        self.now.unwrap_or_else(now_ms)
    }

    fn button_state(&self) -> PartState {
        if self.busy {
            PartState::Disabled
        } else {
            PartState::Rest
        }
    }

    pub(super) fn failover_lines(&self, model: &ClusterModel, a: Appearance) -> Vec<Div> {
        let offset = local_offset_secs(self.now() as f64 / 1000.0);
        model
            .failover_notes(offset)
            .into_iter()
            .map(|note| {
                let ink = if note.problem {
                    Ink::Needs
                } else {
                    Ink::TextMuted
                };
                div()
                    .flex()
                    .items_start()
                    .gap(px(SPACE[0]))
                    .when(note.problem, |row| row.child(note_icon(a)))
                    .child(div().flex_1().min_w(px(0.0)).child(label(
                        &note.text,
                        TypeRole::Meta,
                        ink,
                        a,
                    )))
            })
            .collect()
    }

    pub(super) fn promote_running_line(
        &self,
        model: &ClusterModel,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let status = model.promote_status().filter(|_| model.promote_running())?;
        if self.promote_sheet.is_some() {
            return None;
        }
        let name = model.row(&status.node).map(|r| r.name.clone())?;
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .child(div().flex_1().min_w(px(0.0)).child(label(
                    &t::cluster::move_running(&name),
                    TypeRole::Meta,
                    Ink::TextMuted,
                    a,
                )))
                .child(
                    button(
                        "cluster-promote-show",
                        t::cluster::move_show(),
                        ButtonKind::Secondary,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.show_promote(cx))),
                ),
        )
    }

    fn show_promote(&mut self, cx: &mut Context<Self>) {
        let model = self.feed.snapshot();
        let Some(status) = model.promote_status().filter(|_| model.promote_running()) else {
            return;
        };
        let name = model
            .row(&status.node)
            .map(|r| r.name.clone())
            .unwrap_or_default();
        let primary = model
            .primary_row()
            .map(|r| r.name.clone())
            .unwrap_or_default();
        self.promote_sheet = Some(PromoteSheet::running(&status.node, &name, &primary));
        cx.notify();
    }

    pub(super) fn start_promote(&mut self, cx: &mut Context<Self>) {
        let model = self.feed.snapshot();
        let Some(sheet) = self.promote_sheet.as_mut() else {
            return;
        };
        let Some(request) = sheet.start(&model) else {
            cx.notify();
            return;
        };
        let feed = self.feed.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let result = feed.send(&request);
                    feed.refresh();
                    result
                })
                .await;
            this.update(cx, |view, cx| {
                if let Some(sheet) = view.promote_sheet.as_mut() {
                    sheet.answered(result);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn run_ask(
        &mut self,
        request: Option<Request>,
        failed: fn(&str) -> String,
        cx: &mut Context<Self>,
    ) {
        match request {
            Some(request) => self.run(request, failed, cx),
            None => cx.notify(),
        }
    }

    pub(super) fn take_back_alert(
        &self,
        model: &ClusterModel,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if !self.taking_back {
            return None;
        }
        let on: Handler<Self, bool> = Rc::new(|this, yes, cx| {
            this.taking_back = false;
            if yes {
                let request = this.feed.snapshot().take_back_request();
                this.run_ask(request, t::cluster::take_back_failed, cx);
            } else {
                cx.notify();
            }
        });
        Some(alert_overlay(
            &alert_of(model.take_back_ask()),
            self.busy,
            a,
            cx,
            on,
        ))
    }

    pub(super) fn continue_alert(
        &self,
        model: &ClusterModel,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let session = self.continuing.as_ref()?;
        let ask = model.continue_ask(session, &self.names)?;
        let on: Handler<Self, bool> = Rc::new(|this, yes, cx| {
            let Some(session) = this.continuing.take() else {
                return;
            };
            if yes {
                let request = this.feed.snapshot().continue_request(&session);
                this.run_ask(request, t::cluster::continue_failed, cx);
            } else {
                cx.notify();
            }
        });
        Some(alert_overlay(&alert_of(ask), self.busy, a, cx, on))
    }

    pub(super) fn open_take_back(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        if self.feed.snapshot().offers_take_back() {
            self.taking_back = true;
        }
        cx.notify();
    }

    fn open_copy(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !Path::new(path).is_file() {
            self.note = Some((t::cluster::copy_gone().to_owned(), true));
            cx.notify();
            return;
        }
        if crate::chat::view::open_file_in_chat(Path::new(path), window, cx) {
            cx.emit(Dismiss);
        }
    }

    fn hide_copy(&mut self, session: &str, node: &str, cx: &mut Context<Self>) {
        let request = self.feed.snapshot().hide_side_copy(session, node);
        self.run_ask(request, t::cluster::copy_hide_failed, cx);
    }

    fn wait_row(&self, row: &WaitRow, a: Appearance, cx: &mut Context<Self>) -> Div {
        let (wait, cont) = (row.session.clone(), row.session.clone());
        card(a)
            .p(px(SPACE[2]))
            .flex()
            .flex_col()
            .gap(px(SPACE[1]))
            .child(label(&row.text, TypeRole::Body, Ink::Text, a))
            .child(
                div()
                    .flex()
                    .gap(px(SPACE[1]))
                    .child(
                        button(
                            SharedString::from(format!("waiting-wait-{}", row.session)),
                            row.wait,
                            ButtonKind::Secondary,
                            PartState::Rest,
                            a,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.waited.insert(wait.clone());
                            cx.notify();
                        })),
                    )
                    .child(
                        button(
                            SharedString::from(format!("waiting-continue-{}", row.session)),
                            row.continue_here,
                            ButtonKind::Primary,
                            self.button_state(),
                            a,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.continuing = Some(cont.clone());
                            cx.notify();
                        })),
                    ),
            )
            .part("Waiting Session", a, cx)
    }

    pub(super) fn waiting_block(
        &self,
        model: &ClusterModel,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let rows: Vec<WaitRow> = model
            .waiting_rows(&self.names)
            .into_iter()
            .filter(|r| !self.waited.contains(&r.session))
            .collect();
        if rows.is_empty() {
            return None;
        }
        let mut block = div()
            .flex()
            .flex_col()
            .gap(px(SPACE[1]))
            .child(heading(t::cluster::waiting_title(), a));
        for row in &rows {
            block = block.child(self.wait_row(row, a, cx));
        }
        Some(block)
    }

    fn copy_row(&self, index: usize, row: &CopyRow, a: Appearance, cx: &mut Context<Self>) -> Div {
        let (session, node) = (row.session.clone(), row.node.clone());
        let mut buttons = div().flex().items_center().gap(px(SPACE[0]));
        if let Some(path) = row.open.clone() {
            buttons = buttons.child(
                button(
                    SharedString::from(format!("copy-open-{index}-{}-{}", row.session, row.node)),
                    t::cluster::copy_open(),
                    ButtonKind::Secondary,
                    PartState::Rest,
                    a,
                )
                .on_click(
                    cx.listener(move |this, _, window, cx| this.open_copy(&path, window, cx)),
                ),
            );
        }
        buttons = buttons.child(
            button(
                SharedString::from(format!("copy-hide-{index}-{}-{}", row.session, row.node)),
                t::cluster::copy_hide(),
                ButtonKind::Ghost,
                self.button_state(),
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.hide_copy(&session, &node, cx))),
        );
        div()
            .flex()
            .items_center()
            .gap(px(SPACE[1]))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .child(label(&row.title, TypeRole::Body, Ink::Text, a).elide())
                    .child(label(&row.line, TypeRole::Meta, Ink::TextMuted, a)),
            )
            .child(buttons)
    }

    pub(super) fn copies_block(
        &self,
        model: &ClusterModel,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let rows = model.side_copy_rows(&self.names, self.now());
        if rows.is_empty() {
            return None;
        }
        let mut list = div().flex().flex_col().gap(px(SPACE[1]));
        for (index, row) in rows.iter().enumerate() {
            list = list.child(self.copy_row(index, row, a, cx));
        }
        Some(
            card(a)
                .p(px(SPACE[2]))
                .flex()
                .flex_col()
                .gap(px(SPACE[1]))
                .child(label(
                    t::cluster::copies_title(),
                    TypeRole::Heading,
                    Ink::Text,
                    a,
                ))
                .child(label(
                    t::cluster::copies_note(),
                    TypeRole::Meta,
                    Ink::TextMuted,
                    a,
                ))
                .child(list)
                .part("Side Copies", a, cx),
        )
    }
}
