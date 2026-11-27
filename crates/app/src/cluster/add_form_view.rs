use std::sync::atomic::Ordering;

use gpui::{div, prelude::*, px, Context, Div, Window};
use notchling_engine::machines::registry::Machine;
use notchling_engine::machines::ssh_command::parse_ssh_command;
use notchling_engine::machines::ssh_test::{run_target_stoppable, Target, TEST_PARALLEL};
use notchling_kit::metric::SPACE;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, icon, label};
use notchling_kit::{Appearance, Icon, IconSize, Ink, TypeRole};
use notchling_locale as t;

use super::add_sheet::{parse_draft, AddView, Parsed};
use super::add_test_run::{may_test, test_signature};
use super::add_view::AddSheet;
use super::address_rows_view::Rows;
use super::addresses_form::text_of;
use super::edit_key::EditResult;
use super::rounds;
use super::ssh_text::{address_test_text, test_ink};
use super::view::{field_box, ClusterSheetView};

fn test_machine(parsed: &Parsed) -> Machine {
    let (user, host) = parsed
        .target
        .split_once('@')
        .unwrap_or(("", parsed.target.as_str()));
    let mut machine = Machine::new(&parsed.name, host, user);
    machine.set_ssh_key(parsed.key.as_deref());
    if !parsed.addresses.is_empty() {
        machine.set_addresses(parsed.addresses.clone());
    }
    machine
}

impl ClusterSheetView {
    pub(super) fn add_form(
        &self,
        mut body: Div,
        shown: &AddView,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(sheet) = self.add.sheet.as_ref() else {
            return body;
        };
        let address_focused = sheet.address.read(cx).is_focused(window);
        let name_focused = sheet.name.read(cx).is_focused(window);
        let running = sheet.run.is_running();
        let can_test = may_test(
            shown.target.is_some(),
            running,
            self.test_lanes,
            TEST_PARALLEL,
        );
        let test = button(
            "add-test",
            if running {
                t::cluster::ssh_testing()
            } else {
                t::cluster::ssh_test()
            },
            ButtonKind::Secondary,
            if can_test {
                PartState::Rest
            } else {
                PartState::Disabled
            },
            a,
        )
        .when(can_test, |b| {
            b.on_click(cx.listener(|this, _, _, cx| this.add_test(cx)))
        });
        body = body
            .child(label(
                t::cluster::add_intro(),
                TypeRole::Body,
                Ink::TextMuted,
                a,
            ))
            .child(label(
                t::cluster::add_address_label(),
                TypeRole::Label,
                Ink::TextFaint,
                a,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(SPACE[1]))
                    .child(div().flex_1().min_w(px(0.0)).child(field_box(
                        &sheet.address,
                        address_focused,
                        a,
                    )))
                    .child(test),
            );
        if let Some(text) = &shown.target {
            body = body.child(label(text, TypeRole::Meta, Ink::TextMuted, a));
        }
        if let Some(problem) = &shown.error {
            body = body.child(label(problem, TypeRole::Meta, Ink::Needs, a));
        }
        body = body
            .child(self.add_test_lines(sheet, a, cx))
            .child(label(
                t::cluster::add_name_label(),
                TypeRole::Label,
                Ink::TextFaint,
                a,
            ))
            .child(field_box(&sheet.name, name_focused, a))
            .child(self.add_more_row(sheet.more, a, cx));
        if sheet.more {
            let jump = parse_ssh_command(sheet.address.read(cx).text())
                .ok()
                .and_then(|t| t.jump);
            body = body.child(self.rows_view_with(Rows::Add, true, jump.as_deref(), window, a, cx));
        }
        body
    }

    fn add_more_row(&self, open: bool, a: Appearance, cx: &mut Context<Self>) -> impl IntoElement {
        let chevron = if open {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };
        div()
            .id("add-more")
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(SPACE[0]))
            .child(icon(chevron, IconSize::Row.points(), Ink::TextFaint, a))
            .child(label(
                t::cluster::add_more(),
                TypeRole::Label,
                Ink::TextFaint,
                a,
            ))
            .on_click(cx.listener(|this, _, _, cx| this.add_toggle_more(cx)))
    }

    pub(super) fn add_toggle_more(&mut self, cx: &mut Context<Self>) {
        if let Some(sheet) = self.add.sheet.as_mut() {
            sheet.more = !sheet.more;
        }
        cx.notify();
    }

    pub(super) fn add_test_reset(&mut self) {
        if let Some(sheet) = self.add.sheet.as_mut() {
            sheet.run.reset();
            sheet.error = None;
        }
    }

    fn add_signature(&self, cx: &Context<Self>) -> String {
        let draft = self.add_draft(cx);
        let jump = parse_ssh_command(&draft.address).ok().and_then(|t| t.jump);
        let machines = self
            .add
            .sheet
            .as_ref()
            .and_then(|s| s.parts.as_ref())
            .map(|p| p.registry.all())
            .unwrap_or_default();
        test_signature(&draft, &machines, jump.as_deref())
    }

    fn add_test_lines(&self, sheet: &AddSheet, a: Appearance, cx: &mut Context<Self>) -> Div {
        let mut lines = div().flex().flex_none().flex_col().gap(px(2.0));
        for row in sheet.run.shown(&self.add_signature(cx)) {
            lines = lines.child(label(
                &t::cluster::ssh_test_address(&row.address, &address_test_text(&row.result)),
                TypeRole::Meta,
                test_ink(&row.result),
                a,
            ));
        }
        lines
    }

    pub(super) fn add_test(&mut self, cx: &mut Context<Self>) {
        let draft = self.add_draft(cx);
        let signature = self.add_signature(cx);
        let full = self.test_lanes >= TEST_PARALLEL;
        let Some(sheet) = self.add.sheet.as_mut() else {
            return;
        };
        if sheet.run.is_running() || full {
            return;
        }
        let Some(parts) = sheet.parts.clone() else {
            return;
        };
        let parsed = match parse_draft(&draft, &sheet.user, &sheet.names) {
            Ok(parsed) => parsed,
            Err(why) => {
                sheet.error = Some(why);
                cx.notify();
                return;
            }
        };
        let machine = test_machine(&parsed);
        let home = std::path::PathBuf::from(&parts.this.home);
        sheet.error = None;
        let round = rounds::next();
        let stop = sheet.run.start(signature, round);
        self.test_lanes += 1;
        cx.spawn(async move |this, cx| {
            let user = machine.user.clone();
            let results = cx
                .background_executor()
                .spawn(async move {
                    let runner = parts.engine.runner().clone();
                    let all = parts.registry.all();
                    let mut results = Vec::new();
                    for (address, target) in parts.engine.router().targets(&machine, &all) {
                        if stop.load(Ordering::SeqCst) {
                            break;
                        }
                        let route = match &target {
                            Target::Route(route) => Some(route.clone()),
                            _ => None,
                        };
                        let result = run_target_stoppable(&*runner, &target, &home, Some(&stop));
                        results.push(EditResult {
                            address: text_of(&address, &user),
                            result,
                            route,
                        });
                    }
                    results
                })
                .await;
            this.update(cx, |view, cx| {
                view.test_lanes = view.test_lanes.saturating_sub(1);
                let now = view.add_signature(cx);
                if let Some(sheet) = view.add.sheet.as_mut() {
                    sheet.run.finish(round, &now, results);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
}
