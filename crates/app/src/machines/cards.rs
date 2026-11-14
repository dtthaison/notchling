use std::rc::Rc;

use gpui::{div, prelude::*, px, App, Context, Div, SharedString};
use notchling_engine::machines::forms::abilities_line;
use notchling_engine::machines::registry::{AdminAccess, Machine};
use notchling_engine::machines::step::{CheckResult, SetupStep, StepFix, StepId};
use notchling_engine::machines::steps_repo::offers_replace;
use notchling_engine::versions::{Mark, Tool};
use notchling_kit::metric::RADIUS_SM;
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, icon, label, KitPart, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};

use super::pieces::{LogTail, Tone};
use super::versions_model::{
    place_rows, remove_note, replace_label, replace_tip, PlaceFlag, VersionRow,
};
use super::view::{
    busy_line, marked, mono_box, state_icon, state_mark, text_line, Confirming, MachinesView,
    CARD_PADDING,
};
use crate::accounts::view_rows::{segmented, Handler};
use crate::mac::gate;

fn ink_of(tone: Tone) -> Ink {
    match tone {
        Tone::Good => Ink::Done,
        Tone::Warn => Ink::Needs,
        Tone::Bad => Ink::Expired,
        Tone::Neutral => Ink::TextFaint,
    }
}

fn state(busy: bool) -> PartState {
    if busy {
        PartState::Disabled
    } else {
        PartState::Rest
    }
}

fn acting_state(busy: bool, off: Option<&str>) -> PartState {
    state(busy || off.is_some())
}

fn off_tip(
    element: gpui::Stateful<Div>,
    off: Option<&'static str>,
    a: Appearance,
) -> gpui::Stateful<Div> {
    match off {
        Some(why) => gate::tip_text(element, SharedString::from(why), a),
        None => element,
    }
}

impl MachinesView {
    pub(super) fn machine_card(
        &self,
        machine: &Machine,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let shown = self.model.engine().overlaid(machine);
        let busy = self.model.engine().running(&machine.name).is_some();
        let mut body = card(a)
            .w_full()
            .p(px(CARD_PADDING))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(self.card_header(machine, busy, a, cx));
        if shown.sd_worker().is_some() {
            body = body.child(self.chip_line(machine, a));
        }
        body = body.child(label(
            &abilities_line(shown.abilities.as_ref(), shown.os),
            TypeRole::Meta,
            Ink::TextMuted,
            a,
        ));
        if machine.admin() == AdminAccess::RootSsh {
            body = body.child(marked(
                Icon::Warning,
                &format!("Admin steps run as root over SSH on {}.", machine.host),
                TypeRole::Label,
                Ink::AccentText,
                a,
            ));
        }
        let results = self.model.engine().results(&machine.name);
        let mut steps = div().flex().flex_col().gap(px(8.0));
        for step in self.model.engine().steps(machine) {
            steps = steps.child(self.step_row(machine, &step, results.get(&step.id), busy, a, cx));
        }
        body = body.child(steps);
        let rows = self.model.secret_rows(&shown);
        if !rows.is_empty() {
            body = body.child(self.secrets(machine, rows, busy, a, cx));
        }
        if let Some(id) = self.model.engine().running(&machine.name) {
            let title = self
                .model
                .engine()
                .steps(machine)
                .into_iter()
                .find(|s| s.id == id)
                .map_or(id.raw().to_owned(), |s| s.title);
            body = body.child(busy_line(
                SharedString::from(format!("running-{}", machine.name)),
                &format!("Running {title}…"),
                TypeRole::Label,
                Ink::TextMuted,
                a,
            ));
        }
        if let Some(line) = self.model.outcomes.get(&machine.name).filter(|_| !busy) {
            let (mark, ink) = if line.ok {
                (Icon::Done, Ink::Done)
            } else if line.guidance {
                (Icon::Info, Ink::AccentText)
            } else {
                (Icon::Fail, Ink::Expired)
            };
            body = body.child(marked(mark, &line.text, TypeRole::Label, ink, a));
        }
        body.children(self.log(machine, a, cx))
    }

    fn card_header(
        &self,
        machine: &Machine,
        busy: bool,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let name = machine.name.clone();
        let admin: Vec<(String, String)> = vec![
            ("sudo".into(), format!("sudo as {}", machine.user)),
            ("root-ssh".into(), "root over SSH".into()),
        ];
        let picked = match machine.admin() {
            AdminAccess::Sudo => "sudo",
            AdminAccess::RootSsh => "root-ssh",
        };
        let for_admin = name.clone();
        let pick: Handler<Self, String> = Rc::new(move |this, key: String, cx| {
            let access = if key == "root-ssh" {
                AdminAccess::RootSsh
            } else {
                AdminAccess::Sudo
            };
            this.model.set_admin(&for_admin, access);
            cx.notify();
        });
        let (for_check, for_all, for_remove) = (name.clone(), name.clone(), name);
        let off = self.model.off_reason();
        let live = !busy && off.is_none();
        let check = button(
            SharedString::from(format!("check-{}", machine.name)),
            "Check",
            ButtonKind::Secondary,
            acting_state(busy, off),
            a,
        )
        .when(live, |b| {
            b.on_click(cx.listener(move |this, _, _, cx| this.check(&for_check, cx)))
        });
        let check = off_tip(check, off, a);
        let all = button(
            SharedString::from(format!("set-up-all-{}", machine.name)),
            "Set up all",
            ButtonKind::Primary,
            acting_state(busy, off),
            a,
        )
        .when(live, |b| {
            b.on_click(cx.listener(move |this, _, window, cx| {
                this.model.request_set_up_all(&for_all);
                window.focus(&this.focus);
                cx.notify();
            }))
        });
        let all = off_tip(all, off, a);
        let remove = button(
            SharedString::from(format!("remove-{}", machine.name)),
            "Remove…",
            ButtonKind::Ghost,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.confirming = Some(Confirming::Remove(for_remove.clone()));
            cx.notify();
        }));
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(label(&machine.name, TypeRole::Title, Ink::Text, a))
            .child(label(
                &format!("{}@{}", machine.user, machine.host),
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            ))
            .child(div().flex_1())
            .child(label("Admin:", TypeRole::Meta, Ink::TextMuted, a))
            .child(segmented(
                &format!("admin-{}", machine.name),
                &admin,
                picked,
                a,
                cx,
                pick,
            ))
            .child(check)
            .child(all)
            .child(remove)
    }

    fn chip_line(&self, machine: &Machine, a: Appearance) -> Div {
        let chip = self.model.chip(machine);
        let mut row = div()
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(8.0))
            .child(
                div()
                    .size(px(8.0))
                    .rounded_full()
                    .fill(ink_of(chip.tone), a),
            )
            .child(label(
                &chip.word,
                TypeRole::Label,
                if chip.tone == Tone::Bad {
                    Ink::Expired
                } else {
                    Ink::TextMuted
                },
                a,
            ));
        for detail in &chip.details {
            row = row.child(label(
                &format!("· {detail}"),
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            ));
        }
        if let Some(error) = &chip.error {
            row = row.child(label(error, TypeRole::Meta, Ink::Expired, a).elide());
        }
        row
    }

    fn step_row(
        &self,
        machine: &Machine,
        step: &SetupStep,
        result: Option<&CheckResult>,
        busy: bool,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let (mark, ink) = state_mark(result.map(|r| &r.state));
        let text = result.map_or("checking…".to_owned(), |r| r.state.text().to_owned());
        let mut column = div().flex().flex_col().gap(px(4.0)).child(
            div()
                .flex()
                .items_start()
                .gap(px(8.0))
                .child(
                    state_icon(
                        SharedString::from(format!("check-{}-{}", machine.name, step.title)),
                        mark,
                        ink,
                        TypeRole::Body,
                        a,
                    )
                    .w(px(16.0))
                    .flex_shrink_0(),
                )
                .child(label(&step.title, TypeRole::Body, Ink::Text, a).flex_shrink_0())
                .child(
                    label(&text, TypeRole::Meta, Ink::TextMuted, a)
                        .flex_1()
                        .min_w(px(0.0))
                        .line_clamp(2),
                )
                .children(self.step_action(machine, step, result, busy, a, cx)),
        );
        let versions = (step.id == StepId::TOOL_VERSIONS)
            .then(|| self.model.version_rows(&machine.name))
            .flatten();
        if let Some(rows) = versions {
            let drift = self.model.drift_lines(&machine.name);
            column = column.child(self.version_rows(rows, drift, a, cx));
        } else if let (Some(result), true) = (result, step.id.self_update_tool().is_some()) {
            for row in place_rows(result) {
                let (mark, ink) = match row.flag {
                    PlaceFlag::Off => (Icon::Check, Ink::Done),
                    PlaceFlag::On => (Icon::Close, Ink::Expired),
                    PlaceFlag::CantRead => (Icon::Warning, Ink::AccentText),
                };
                column = column.child(
                    div()
                        .pl(px(26.0))
                        .flex()
                        .gap(px(6.0))
                        .child(icon(mark, TypeRole::Meta.spec().size, ink, a))
                        .child(label(&row.name, TypeRole::Meta, Ink::Text, a))
                        .child(
                            label(&row.detail, TypeRole::Meta, Ink::TextMuted, a)
                                .flex_1()
                                .min_w(px(0.0))
                                .line_clamp(2),
                        ),
                );
            }
        } else if let Some(result) = result {
            for row in &result.rows {
                column = column.child(
                    div()
                        .pl(px(26.0))
                        .flex()
                        .gap(px(6.0))
                        .child(icon(
                            if row.ok { Icon::Check } else { Icon::Close },
                            TypeRole::Meta.spec().size,
                            if row.ok { Ink::Done } else { Ink::Expired },
                            a,
                        ))
                        .child(label(&row.name, TypeRole::Meta, Ink::Text, a))
                        .child(
                            label(&row.detail, TypeRole::Meta, Ink::TextMuted, a)
                                .flex_1()
                                .min_w(px(0.0))
                                .line_clamp(2),
                        ),
                );
            }
        }
        if self.how_to.get(&machine.name) == Some(&step.id) {
            if let StepFix::HowTo(how) = &step.fix {
                column = column.child(
                    div()
                        .ml(px(26.0))
                        .p(px(10.0))
                        .rounded(px(RADIUS_SM))
                        .fill(Ink::Sidebar, a)
                        .child(text_line(how, TypeRole::Meta, Ink::Text, a)),
                );
            }
        }
        column
    }

    fn version_rows(
        &self,
        rows: Vec<VersionRow>,
        drift: Vec<(Tool, String)>,
        a: Appearance,
        cx: &App,
    ) -> Div {
        let mut column = div().flex().flex_col().gap(px(4.0));
        for row in rows {
            let (mark, ink) = match row.mark {
                Mark::Same => (Icon::Check, Ink::Done),
                Mark::Behind | Mark::Ahead => (Icon::Warning, Ink::AccentText),
                Mark::Held | Mark::CantPin | Mark::NotInstalled | Mark::Unknown => {
                    (Icon::Close, Ink::TextMuted)
                }
            };
            column = column.child(
                div()
                    .pl(px(26.0))
                    .flex()
                    .gap(px(6.0))
                    .child(icon(mark, TypeRole::Meta.spec().size, ink, a))
                    .child(label(row.tool.title(), TypeRole::Meta, Ink::Text, a).flex_shrink_0())
                    .child(
                        label(&row.text, TypeRole::Meta, Ink::TextMuted, a)
                            .flex_1()
                            .min_w(px(0.0))
                            .line_clamp(2),
                    )
                    .child(label(row.mark_text, TypeRole::Meta, ink, a).flex_shrink_0()),
            );
            if let Some((_, line)) = drift.iter().find(|(tool, _)| *tool == row.tool) {
                column = column.child(div().pl(px(26.0 + 22.0)).child(label(
                    line,
                    TypeRole::Meta,
                    Ink::AccentText,
                    a,
                )));
            }
        }
        column.part("Tool Versions", a, cx)
    }

    fn step_action(
        &self,
        machine: &Machine,
        step: &SetupStep,
        result: Option<&CheckResult>,
        busy: bool,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::Stateful<Div>> {
        let mut actions = Vec::new();
        let off = self.model.off_reason();
        let live = !busy && off.is_none();
        let failed = result.is_some_and(|r| r.state.is_fail());
        let (name, id) = (machine.name.clone(), step.id.clone());
        if step.replace.is_some() && offers_replace(result) {
            let (n, i) = (name.clone(), id.clone());
            let replace = button(
                SharedString::from(format!("replace-{}-{}", machine.name, step.id.raw())),
                replace_label(&step.id),
                ButtonKind::Ghost,
                acting_state(busy, off),
                a,
            )
            .when(live, |b| {
                b.on_click(cx.listener(move |this, _, window, cx| {
                    this.model.request_replace_key(&i, &n);
                    window.focus(&this.focus);
                    cx.notify();
                }))
            });
            actions.push(gate::tip_text(
                replace,
                off.unwrap_or(replace_tip(&step.id)).into(),
                a,
            ));
        }
        match &step.fix {
            StepFix::Run(_) if failed => {
                let set_up = button(
                    SharedString::from(format!("set-up-{}-{}", machine.name, step.id.raw())),
                    "Set up",
                    ButtonKind::Secondary,
                    acting_state(busy, off),
                    a,
                )
                .when(live, |b| {
                    b.on_click(cx.listener(move |this, _, window, cx| {
                        this.model.request_set_up(&id, &name);
                        window.focus(&this.focus);
                        cx.notify();
                    }))
                });
                actions.push(off_tip(set_up, off, a));
            }
            StepFix::HowTo(_) if failed => {
                let open = self.how_to.get(&machine.name) == Some(&step.id);
                let how = button(
                    SharedString::from(format!("how-to-{}-{}", machine.name, step.id.raw())),
                    if open { "Hide" } else { "How to…" },
                    ButtonKind::Ghost,
                    PartState::Rest,
                    a,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.how_to.get(&name) == Some(&id) {
                        this.how_to.remove(&name);
                    } else {
                        this.how_to.insert(name.clone(), id.clone());
                    }
                    cx.notify();
                }));
                actions.push(how);
            }
            _ => {}
        }
        actions
    }

    fn secrets(
        &self,
        machine: &Machine,
        rows: Vec<super::pieces::SecretRow>,
        busy: bool,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut column = div().flex().flex_col().gap(px(6.0)).child(label(
            "Secrets",
            TypeRole::Section,
            Ink::TextMuted,
            a,
        ));
        for row in rows {
            let usage = row.usage.clone();
            let off = self.model.off_reason();
            let can_send = row.can_send && !busy && off.is_none();
            let send = button(
                SharedString::from(format!("send-{}-{}", machine.name, row.id())),
                "Send again",
                ButtonKind::Ghost,
                state(!can_send),
                a,
            )
            .when(can_send, |b| {
                b.on_click(cx.listener(move |this, _, window, cx| {
                    this.model.request_send_again(&usage);
                    window.focus(&this.focus);
                    cx.notify();
                }))
            });
            let send = off_tip(send, off, a);
            let name = row.usage.name.clone();
            let renew = button(
                SharedString::from(format!("renew-{}-{}", machine.name, row.id())),
                "Renew…",
                ButtonKind::Ghost,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.renew(&name, cx);
                cx.notify();
            }));
            column = column.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        label(&row.text, TypeRole::Meta, Ink::Text, a)
                            .flex_1()
                            .min_w(px(0.0)),
                    )
                    .child(send)
                    .child(renew),
            );
        }
        column
    }

    fn log(&self, machine: &Machine, a: Appearance, cx: &mut Context<Self>) -> Option<Div> {
        let lines = self.model.engine().log(&machine.name);
        if lines.is_empty() {
            return None;
        }
        let open = self.log_open.contains(&machine.name)
            || self.model.engine().running(&machine.name).is_some();
        let name = machine.name.clone();
        let toggle = div()
            .id(SharedString::from(format!("log-toggle-{}", machine.name)))
            .flex()
            .items_center()
            .gap(px(6.0))
            .cursor_pointer()
            .child(icon(
                if open {
                    Icon::ChevronDown
                } else {
                    Icon::ChevronRight
                },
                TypeRole::Meta.spec().size,
                Ink::TextMuted,
                a,
            ))
            .child(label(
                &format!("Log ({} lines)", lines.len()),
                TypeRole::Label,
                Ink::TextMuted,
                a,
            ))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.log_open.remove(&name) {
                    this.log_open.insert(name.clone());
                }
                cx.notify();
            }));
        let mut column = div().flex().flex_col().gap(px(6.0)).child(toggle);
        if open {
            let tail = LogTail::make(&lines);
            if let Some(note) = &tail.note {
                column = column.child(label(note, TypeRole::Meta, Ink::TextMuted, a));
            }
            column = column.child(mono_box(
                format!("log-{}", machine.name),
                &tail.shown.join("\n"),
                220.0,
                a,
            ));
        }
        Some(column)
    }

    pub(super) fn confirm_overlay(&self, a: Appearance, cx: &mut Context<Self>) -> Option<Div> {
        use crate::accounts::flows::Alert;
        use crate::accounts::view_rows::alert_overlay;
        let confirming = self.confirming.clone()?;
        let alert = match &confirming {
            Confirming::Remove(name) => Alert {
                title: format!("Remove {name} from Notchling?"),
                message: remove_note(
                    &self
                        .model
                        .engine()
                        .self_update_undo_plan(name)
                        .into_iter()
                        .map(|(_, title, _)| title)
                        .collect::<Vec<_>>(),
                ),
                button: "Remove".into(),
                destructive: true,
            },
            Confirming::Close(item) => Alert {
                title: format!("Close {} #{} as not planned?", item.repo, item.issue),
                message: String::new(),
                button: "Close the issue".into(),
                destructive: true,
            },
            Confirming::Drop(item) => Alert {
                title: format!("Drop {} #{}?", item.repo, item.issue),
                message: "This closes the issue as not planned.".into(),
                button: "Drop it".into(),
                destructive: true,
            },
        };
        let on: Handler<Self, bool> = Rc::new(move |this, yes: bool, cx| {
            this.confirming = None;
            if yes {
                match &confirming {
                    Confirming::Remove(name) => this.model.remove(name),
                    Confirming::Close(item) => {
                        let item = item.clone();
                        let i = item.clone();
                        this.perform(&item, Some("Closed"), move |acts| acts.close(&i), cx);
                    }
                    Confirming::Drop(item) => {
                        let item = item.clone();
                        let i = item.clone();
                        this.perform(&item, Some("Dropped"), move |acts| acts.drop_item(&i), cx);
                    }
                }
            }
            cx.notify();
        });
        Some(alert_overlay(&alert, false, a, cx, on))
    }
}
