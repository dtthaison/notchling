use std::rc::Rc;

use gpui::{
    div, prelude::*, px, Context, Div, Entity, PathPromptOptions, SharedString, Subscription,
    Window,
};
use notchling_engine::machines::ssh_test::run_target;
use notchling_kit::metric::SPACE;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, hsla, label, KitPart, TextEditor, TextEditorEvent};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;

use super::add_parts::AddParts;
use super::address_rows_view::{RowEditor, Rows};
use super::addresses_form::{addresses_of, drafts_of, text_of};
use super::edit_key::{draft_machine, key_of, save_edit, EditTest};
use super::ssh_text::{address_test_text, key_issue_text, key_note, test_ink};
use super::view::{field_box, ClusterSheetView};
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
    pub key: Entity<TextEditor>,
    pub test: EditTest,
    pub round: u64,
    pub _key_events: Subscription,
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
        let key =
            cx.new(|cx| TextEditor::new(t::cluster::ssh_key_placeholder(), 1, cx).single_line());
        if let Some(saved) = machine.ssh_key() {
            key.update(cx, |editor, cx| editor.set_text(saved, cx));
        }
        let key_events = cx.subscribe(&key, |this, _, event: &TextEditorEvent, cx| {
            if *event == TextEditorEvent::Changed {
                if let Some(sheet) = this.edit.as_mut() {
                    sheet.error = None;
                    sheet.test = EditTest::Idle;
                }
                cx.notify();
            }
        });
        self.edit = Some(EditSheet {
            name: name.to_owned(),
            user: machine.user.clone(),
            rows: Vec::new(),
            error: None,
            parts: parts.clone(),
            key,
            test: EditTest::Idle,
            round: 0,
            _key_events: key_events,
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

    fn edit_home(&self) -> std::path::PathBuf {
        self.edit
            .as_ref()
            .map(|s| std::path::PathBuf::from(&s.parts.this.home))
            .unwrap_or_default()
    }

    pub(super) fn edit_choose_key(&mut self, cx: &mut Context<Self>) {
        let ssh = self.edit_home().join(".ssh");
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t::cluster::ssh_key_picker_prompt().into()),
            directory: Some(ssh),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await {
                if let Some(path) = paths.first() {
                    let text = path.to_string_lossy().into_owned();
                    this.update(cx, |view, cx| view.edit_set_key(&text, cx))
                        .ok();
                }
            }
        })
        .detach();
    }

    pub(super) fn edit_set_key(&mut self, text: &str, cx: &mut Context<Self>) {
        let home = self.edit_home();
        let tidy = notchling_engine::machines::ssh_key::tidy(text, &home);
        if let Some(sheet) = self.edit.as_ref() {
            let key = sheet.key.clone();
            key.update(cx, |editor, cx| editor.set_text(&tidy, cx));
        }
        cx.notify();
    }

    pub(super) fn edit_test(&mut self, cx: &mut Context<Self>) {
        let drafts = self.rows_drafts(Rows::Edit, cx);
        let home = self.edit_home();
        let Some(sheet) = self.edit.as_mut() else {
            return;
        };
        if matches!(sheet.test, EditTest::Running(_)) {
            return;
        }
        let list = match addresses_of(&drafts, &sheet.name, &sheet.user) {
            Ok(list) => list,
            Err(why) => {
                sheet.error = Some(why);
                cx.notify();
                return;
            }
        };
        let text = sheet.key.read(cx).text().to_owned();
        let key = match key_of(&text, &home) {
            Ok((key, _)) => key,
            Err(issue) => {
                sheet.error = Some(key_issue_text(issue).to_owned());
                cx.notify();
                return;
            }
        };
        let all = sheet.parts.registry.all();
        let Some(machine) = all.iter().find(|m| m.name == sheet.name).cloned() else {
            sheet.error = Some(t::cluster::address_edit_gone(&sheet.name));
            cx.notify();
            return;
        };
        let draft = draft_machine(&machine, list, key.as_deref());
        sheet.error = None;
        sheet.round += 1;
        let round = sheet.round;
        sheet.test = EditTest::Running(round);
        let parts = sheet.parts.clone();
        cx.spawn(async move |this, cx| {
            let user = draft.user.clone();
            let results = cx
                .background_executor()
                .spawn(async move {
                    let runner = parts.engine.runner().clone();
                    parts
                        .engine
                        .router()
                        .targets(&draft, &all)
                        .into_iter()
                        .map(|(address, target)| {
                            let result = run_target(&*runner, &target, &home);
                            (text_of(&address, &user), result)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |view, cx| {
                if let Some(sheet) = view.edit.as_mut() {
                    if sheet.test == EditTest::Running(round) {
                        sheet.test = EditTest::Done(round, results);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn edit_save(&mut self, cx: &mut Context<Self>) {
        let drafts = self.rows_drafts(Rows::Edit, cx);
        let home = self.edit_home();
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
        let text = sheet.key.read(cx).text().to_owned();
        let key = match key_of(&text, &home) {
            Ok((key, _)) => key,
            Err(issue) => {
                sheet.error = Some(key_issue_text(issue).to_owned());
                cx.notify();
                return;
            }
        };
        match save_edit(&sheet.parts, &sheet.name, list, key.as_deref()) {
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
            .child(self.rows_view(Rows::Edit, window, a, cx))
            .child(self.edit_key_block(sheet, window, a, cx));
        if let Some(error) = &sheet.error {
            inner = inner.child(label(error, TypeRole::Meta, Ink::Needs, a));
        }
        let body = div()
            .id("edit-addresses-body")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .child(inner);
        let running = matches!(sheet.test, EditTest::Running(_));
        let buttons = div()
            .pt(px(SPACE[1]))
            .flex()
            .gap(px(SPACE[1]))
            .child(
                button(
                    "edit-test",
                    if running {
                        t::cluster::ssh_testing()
                    } else {
                        t::cluster::ssh_test()
                    },
                    ButtonKind::Secondary,
                    if running {
                        PartState::Disabled
                    } else {
                        PartState::Rest
                    },
                    a,
                )
                .on_click(cx.listener(|this, _, _, cx| this.edit_test(cx))),
            )
            .child(div().flex_1())
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
                .py(px(SHEET_MARGIN / 2.0))
                .bg(hsla(Ink::Shadow.color(a)))
                .child(
                    card(a)
                        .w(px(SHEET_WIDTH))
                        .max_h_full()
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
                        .child(Self::edit_test_lines(sheet, a))
                        .child(buttons)
                        .part_at("Edit Addresses Sheet", Corner::TopTrailing, a, cx),
                ),
        )
    }

    fn edit_key_block(
        &self,
        sheet: &EditSheet,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let text = sheet.key.read(cx).text().to_owned();
        let home = std::path::PathBuf::from(&sheet.parts.this.home);
        let issue = if text.trim().is_empty() {
            None
        } else {
            key_of(&text, &home).err()
        };
        let focused = sheet.key.read(cx).is_focused(window);
        let mut block = div()
            .flex()
            .flex_col()
            .gap(px(SPACE[0]))
            .child(label(
                t::cluster::ssh_key_label(),
                TypeRole::Label,
                Ink::Text,
                a,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(SPACE[0]))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .child(field_box(&sheet.key, focused, a)),
                    )
                    .child(
                        button(
                            "edit-key-choose",
                            t::cluster::ssh_key_choose(),
                            ButtonKind::Secondary,
                            PartState::Rest,
                            a,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.edit_choose_key(cx))),
                    )
                    .child(
                        button(
                            "edit-key-clear",
                            t::cluster::ssh_key_clear(),
                            ButtonKind::Ghost,
                            if text.trim().is_empty() {
                                PartState::Disabled
                            } else {
                                PartState::Rest
                            },
                            a,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.edit_set_key("", cx))),
                    ),
            );
        match key_note(Some(text.trim()).filter(|k| !k.is_empty()), issue) {
            Some((note, ink)) => block = block.child(label(&note, TypeRole::Meta, ink, a)),
            None => {
                block = block.child(label(
                    t::cluster::ssh_key_note(),
                    TypeRole::Meta,
                    Ink::TextMuted,
                    a,
                ))
            }
        }
        block
    }

    fn edit_test_lines(sheet: &EditSheet, a: Appearance) -> Div {
        let mut lines = div().flex().flex_none().flex_col().gap(px(2.0));
        if let EditTest::Done(_, results) = &sheet.test {
            for (address, result) in results {
                lines = lines.child(label(
                    &t::cluster::ssh_test_address(address, &address_test_text(result)),
                    TypeRole::Meta,
                    test_ink(result),
                    a,
                ));
            }
        }
        lines
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
