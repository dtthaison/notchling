use std::sync::Arc;

use gpui::{
    div, prelude::*, px, Context, Div, EventEmitter, Render, SharedString, Stateful, Window,
};
use notchling_kit::metric::{RADIUS_CARD, RADIUS_SM, SIDE_PADDING};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{
    button, card, hsla, icon, label, scrollable, window_appearance, KitPart, KitStyled,
};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;
use serde_json::Value;

use super::profile::{tool_title, Imported};
use super::setup::{
    diff_line, fate_text, key_line, short_path, Action, Change, DiffLine, Group, Outcome,
    SetupModel, Stage,
};
use super::wire;
use crate::actions::Actions;
use crate::mac::gate;
use crate::sheets::header::sheet_header;
use crate::sheets::view::Dismiss;

const DIFF_LINES: usize = 60;
const REPORT_LINES: usize = 8;
const TAG_WIDTH: f32 = 62.0;

pub struct SetupView {
    actions: Option<Arc<Actions>>,
    pub(super) model: SetupModel,
    busy: bool,
}

impl EventEmitter<Dismiss> for SetupView {}

fn change_word(change: Change) -> &'static str {
    match change {
        Change::New => t::costs::change_new(),
        Change::Edit => t::costs::change_edit(),
        Change::Replace => t::costs::change_replace(),
        Change::Remove => t::costs::change_remove(),
        Change::Offer => t::costs::change_offer(),
        Change::Skip => t::costs::change_skip(),
        Change::InStep => t::costs::change_in_step(),
    }
}

fn change_ink(change: Change) -> Ink {
    match change {
        Change::New => Ink::Done,
        Change::Edit | Change::Replace => Ink::AccentText,
        Change::Remove => Ink::Needs,
        Change::Offer | Change::Skip | Change::InStep => Ink::TextMuted,
    }
}

fn checkbox(id: SharedString, on: bool, a: Appearance) -> Stateful<Div> {
    div()
        .id(id)
        .flex_shrink_0()
        .size(px(16.0))
        .rounded(px(4.0))
        .border(px(1.0))
        .border_color(hsla(Ink::Hairline.color(a)))
        .fill(if on { Ink::Accent } else { Ink::Raised }, a)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .when(on, |m| {
            m.child(icon(
                Icon::Check,
                TypeRole::Meta.spec().size,
                Ink::OnAccent,
                a,
            ))
        })
}

fn status_dot(ink: Ink, id: SharedString, tip: &str, a: Appearance) -> Stateful<Div> {
    gate::tip_text(
        div()
            .id(id)
            .flex_shrink_0()
            .size(px(8.0))
            .rounded_full()
            .fill(ink, a),
        SharedString::from(tip.to_owned()),
        a,
    )
}

impl SetupView {
    pub fn live(actions: Arc<Actions>, cx: &mut Context<Self>) -> SetupView {
        let mut view = SetupView {
            actions: Some(actions),
            model: SetupModel::new(&std::env::var("HOME").unwrap_or_default()),
            busy: false,
        };
        view.read(cx);
        view
    }

    pub fn fixed(model: SetupModel) -> SetupView {
        SetupView {
            actions: None,
            model,
            busy: false,
        }
    }

    fn read(&mut self, cx: &mut Context<Self>) {
        let Some(actions) = self.actions.clone() else {
            return;
        };
        let consent = self.model.consent_list();
        self.busy = true;
        cx.spawn(async move |this, cx| {
            let (plan, versions) = cx
                .background_executor()
                .spawn(async move {
                    let plan = actions
                        .read_full(&wire::plan(&consent))
                        .map_err(|f| f.message);
                    (plan, actions.read(&wire::versions()).ok())
                })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                view.model.show_plan(plan, versions);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn toggle_offer(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.busy || self.model.stage != Stage::Review {
            return;
        }
        if self.model.toggle_offer(id) {
            self.read(cx);
        }
    }

    fn toggle_diff(&mut self, id: &str, cx: &mut Context<Self>) {
        self.model.toggle_diff(id);
        cx.notify();
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        let Some(actions) = self.actions.clone() else {
            return;
        };
        if self.busy || !self.model.can_apply() {
            return;
        }
        let consent = self.model.consent_list();
        self.model.stage = Stage::Applying;
        self.model.message = None;
        self.busy = true;
        cx.spawn(async move |this, cx| {
            let reply = cx
                .background_executor()
                .spawn(async move {
                    actions
                        .read_full(&wire::apply(&consent))
                        .map_err(|f| (f.message, f.data))
                })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                view.model.finish_apply(reply);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn undo(&mut self, tool: usize, cx: &mut Context<Self>) {
        let Some(actions) = self.actions.clone() else {
            return;
        };
        if self.busy {
            return;
        }
        self.busy = true;
        cx.spawn(async move |this, cx| {
            let reply: Result<Value, String> = cx
                .background_executor()
                .spawn(async move {
                    actions
                        .read_full(&wire::undo(super::profile::TOOLS[tool]))
                        .map_err(|f| f.message)
                })
                .await;
            this.update(cx, |view, cx| {
                view.busy = false;
                view.model.finish_undo(tool, reply);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn diff_block(&self, action: &Action, a: Appearance) -> Div {
        let mut lines: Vec<(String, DiffLine)> = action
            .diff
            .lines()
            .map(|l| (l.to_owned(), diff_line(l)))
            .collect();
        if lines.is_empty() {
            lines = action
                .keys
                .iter()
                .map(|k| (key_line(k), DiffLine::Plain))
                .collect();
        }
        let more = lines.len().saturating_sub(DIFF_LINES);
        let mut block = div()
            .mt(px(6.0))
            .p(px(8.0))
            .rounded(px(RADIUS_SM))
            .fill(Ink::Raised, a)
            .flex()
            .flex_col();
        for (text, kind) in lines.into_iter().take(DIFF_LINES) {
            let ink = match kind {
                DiffLine::Add => Ink::Done,
                DiffLine::Del => Ink::Expired,
                DiffLine::Hunk | DiffLine::Header => Ink::TextMuted,
                DiffLine::Plain => Ink::Text,
            };
            block = block.child(label(&text, TypeRole::Mono, ink, a).min_h(px(14.0)));
        }
        if more > 0 {
            block = block.child(label(
                &t::costs::diff_more(more as u64),
                TypeRole::Meta,
                Ink::TextFaint,
                a,
            ));
        }
        block
    }

    fn action_row(&self, action: &Action, a: Appearance, cx: &mut Context<Self>) -> Div {
        let offer = action.change == Change::Offer || !action.needs.is_empty();
        let ticked = self.model.ticked(action);
        let mut top = div().flex().items_center().gap(px(10.0));
        if offer {
            let id = action.id.clone();
            top = top.child(
                checkbox(
                    SharedString::from(format!("costs-offer-{}", action.id)),
                    ticked,
                    a,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_offer(&id, cx))),
            );
        }
        let change = if offer && ticked && action.change == Change::Offer {
            Change::Edit
        } else {
            action.change
        };
        top = top
            .child(
                label(change_word(change), TypeRole::Label, change_ink(change), a)
                    .w(px(TAG_WIDTH))
                    .flex_shrink_0(),
            )
            .child(
                label(&action.path, TypeRole::Mono, Ink::Text, a)
                    .elide()
                    .flex_1(),
            );
        if action.has_detail() {
            let id = action.id.clone();
            let open = self.model.is_open(&action.id);
            top = top.child(
                button(
                    SharedString::from(format!("costs-diff-{}", action.id)),
                    if open {
                        t::costs::diff_hide()
                    } else {
                        t::costs::diff_show()
                    },
                    ButtonKind::Ghost,
                    PartState::Rest,
                    a,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_diff(&id, cx))),
            );
        }
        let mut row = div()
            .px(px(14.0))
            .py(px(8.0))
            .flex()
            .flex_col()
            .border_t(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(top);
        let inset = if offer { 26.0 } else { 0.0 } + TAG_WIDTH + 10.0;
        for note in &action.explain() {
            row = row.child(
                label(note, TypeRole::Meta, Ink::TextMuted, a)
                    .pl(px(inset))
                    .pt(px(2.0)),
            );
        }
        if self.model.is_open(&action.id) && action.has_detail() {
            row = row.child(div().pl(px(inset)).child(self.diff_block(action, a)));
        }
        row
    }

    fn group_view(&self, group: &Group, a: Appearance, cx: &mut Context<Self>) -> Div {
        let version = group
            .version
            .clone()
            .unwrap_or_else(|| t::costs::tool_unknown_version().to_owned());
        let (ink, status) = if group.found {
            (Ink::Done, version)
        } else {
            (Ink::TextFaint, t::costs::tool_skipped().to_owned())
        };
        let header = div()
            .px(px(14.0))
            .py(px(8.0))
            .flex()
            .items_center()
            .gap(px(10.0))
            .rounded_t(px(RADIUS_CARD))
            .fill(Ink::Raised, a)
            .child(status_dot(
                ink,
                SharedString::from(format!("costs-tool-dot-{}", group.tool)),
                if group.found {
                    t::costs::tool_found()
                } else {
                    t::costs::tool_missing()
                },
                a,
            ))
            .child(label(tool_title(group.tool), TypeRole::Heading, Ink::Text, a).flex_1())
            .child(label(&status, TypeRole::Meta, Ink::TextMuted, a));
        let mut body = card(a).flex().flex_col().overflow_hidden().child(header);
        if group.found {
            for action in &group.actions {
                body = body.child(self.action_row(action, a, cx));
            }
            let note = if group.actions.is_empty() {
                Some(t::costs::group_nothing().to_owned())
            } else if group.in_step > 0 {
                Some(t::costs::group_in_step(group.in_step as u64))
            } else {
                None
            };
            if let Some(note) = note {
                body = body.child(
                    div()
                        .px(px(14.0))
                        .py(px(6.0))
                        .border_t(px(1.0))
                        .border_color(hsla(Ink::Hairline.color(a)))
                        .child(label(&note, TypeRole::Meta, Ink::TextFaint, a)),
                );
            }
        }
        body
    }

    fn report_block(&self, import: &Imported, a: Appearance) -> Div {
        let mut rows = card(a).flex().flex_col();
        for (i, line) in import.lines.iter().take(REPORT_LINES).enumerate() {
            rows = rows.child(
                div()
                    .px(px(14.0))
                    .py(px(5.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .when(i > 0, |r| {
                        r.border_t(px(1.0))
                            .border_color(hsla(Ink::Hairline.color(a)))
                    })
                    .child(
                        label(&line.number.to_string(), TypeRole::Meta, Ink::TextFaint, a)
                            .w(px(24.0))
                            .flex_shrink_0(),
                    )
                    .child(
                        label(&line.text, TypeRole::Mono, Ink::Text, a)
                            .elide()
                            .flex_1(),
                    )
                    .child(
                        label(&fate_text(&line.fate), TypeRole::Meta, Ink::TextMuted, a)
                            .flex_shrink_0(),
                    ),
            );
        }
        let more = import.lines.len().saturating_sub(REPORT_LINES);
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .children((!import.source.is_empty()).then(|| {
                label(
                    &short_path(&import.source, &self.model.home),
                    TypeRole::Mono,
                    Ink::TextMuted,
                    a,
                )
            }))
            .child(rows)
            .children((more > 0).then(|| {
                label(
                    &t::costs::report_more(more as u64),
                    TypeRole::Meta,
                    Ink::TextFaint,
                    a,
                )
            }))
    }

    fn report_section(&self, a: Appearance) -> Option<Div> {
        let plan = self.model.plan.as_ref().filter(|p| !p.imports.is_empty())?;
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(label(
                    t::costs::report_title(),
                    TypeRole::Section,
                    Ink::TextFaint,
                    a,
                ))
                .children(plan.imports.iter().map(|i| self.report_block(i, a))),
        )
    }

    fn written_section(&self, a: Appearance) -> Div {
        let mut rows = card(a).flex().flex_col();
        for (i, w) in self.model.written.iter().enumerate() {
            let ink = if w.done { Ink::Done } else { Ink::Needs };
            let mut detail = change_word(w.change).to_owned();
            if let Some(backup) = &w.backup {
                detail = format!("{detail} · {}", t::costs::written_backup(backup));
            }
            if let Some(error) = &w.error {
                detail = format!("{detail} · {error}");
            }
            rows = rows.child(
                div()
                    .px(px(14.0))
                    .py(px(7.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .when(i > 0, |r| {
                        r.border_t(px(1.0))
                            .border_color(hsla(Ink::Hairline.color(a)))
                    })
                    .child(status_dot(
                        ink,
                        SharedString::from(format!("costs-written-{i}")),
                        if w.done {
                            t::costs::written_done()
                        } else {
                            t::costs::written_failed()
                        },
                        a,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .flex_col()
                            .child(label(&w.path, TypeRole::Mono, Ink::Text, a).elide())
                            .child(label(&detail, TypeRole::Meta, Ink::TextMuted, a)),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(label(
                t::costs::written_title(),
                TypeRole::Section,
                Ink::TextFaint,
                a,
            ))
            .child(rows)
    }

    fn undo_section(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let mut tools = self.model.undoable();
        tools.extend(self.model.undone.iter().map(|u| u.tool));
        tools.sort_unstable();
        tools.dedup();
        let mut rows = card(a).flex().flex_col();
        for (i, tool) in tools.into_iter().enumerate() {
            let undone = self.model.undone_for(tool);
            let mut row = div()
                .px(px(14.0))
                .py(px(8.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .when(i > 0, |r| {
                    r.border_t(px(1.0))
                        .border_color(hsla(Ink::Hairline.color(a)))
                });
            let head = div()
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(label(tool_title(tool), TypeRole::Body, Ink::Text, a).flex_1());
            let head = match undone {
                None => head.child(
                    button(
                        SharedString::from(format!("costs-undo-{tool}")),
                        t::costs::undo(),
                        ButtonKind::Secondary,
                        if self.busy {
                            PartState::Disabled
                        } else {
                            PartState::Rest
                        },
                        a,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.undo(tool, cx))),
                ),
                Some(u) => {
                    let conflicts = u.conflicts().len();
                    let text = if conflicts == 0 {
                        t::costs::undone_clean().to_owned()
                    } else {
                        t::costs::undone_conflicts(conflicts as u64)
                    };
                    let ink = if conflicts == 0 {
                        Ink::Done
                    } else {
                        Ink::Needs
                    };
                    head.child(label(&text, TypeRole::Meta, ink, a))
                }
            };
            row = row.child(head);
            if let Some(u) = undone {
                for r in &u.rows {
                    let (ink, word) = match r.outcome {
                        Outcome::Conflict => (Ink::Needs, t::costs::outcome_conflict()),
                        Outcome::Failed => (Ink::Needs, t::costs::outcome_failed()),
                        Outcome::Restored => (Ink::Done, t::costs::outcome_restored()),
                        Outcome::Removed => (Ink::Done, t::costs::outcome_removed()),
                        Outcome::Gone => (Ink::TextFaint, t::costs::outcome_gone()),
                    };
                    let mut line = word.to_owned();
                    if let Some(d) = &r.detail {
                        line = format!("{line} · {d}");
                    }
                    row = row.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(status_dot(
                                ink,
                                SharedString::from(format!("costs-undo-dot-{tool}-{}", r.path)),
                                word,
                                a,
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .flex()
                                    .flex_col()
                                    .child(label(&r.path, TypeRole::Mono, Ink::Text, a).elide())
                                    .child(label(&line, TypeRole::Meta, Ink::TextMuted, a)),
                            ),
                    );
                }
            }
            rows = rows.child(row);
        }
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(label(
                t::costs::undo_title(),
                TypeRole::Section,
                Ink::TextFaint,
                a,
            ))
            .child(rows)
    }

    fn footer(&self, a: Appearance, cx: &mut Context<Self>) -> Div {
        let (hint, ink) = match self.model.stage {
            Stage::Reading => (t::costs::setup_reading().to_owned(), Ink::TextMuted),
            Stage::Applying => (t::costs::setup_applying().to_owned(), Ink::TextMuted),
            Stage::Done => (
                t::costs::setup_done(self.model.written.iter().filter(|w| w.done).count() as u64),
                Ink::TextMuted,
            ),
            Stage::Review if self.model.plan.is_none() => {
                (t::costs::setup_failed_footer().to_owned(), Ink::Needs)
            }
            Stage::Review if !self.model.install_on() => {
                (t::costs::setup_install_off().to_owned(), Ink::Needs)
            }
            Stage::Review if self.model.writes() == 0 => {
                (t::costs::setup_no_changes().to_owned(), Ink::TextMuted)
            }
            Stage::Review => (t::costs::setup_nothing_written().to_owned(), Ink::TextMuted),
        };
        let enabled = self.model.can_apply() && !self.busy;
        let apply = button(
            "costs-apply",
            t::costs::apply(),
            ButtonKind::Primary,
            if enabled {
                PartState::Rest
            } else {
                PartState::Disabled
            },
            a,
        );
        let apply = if enabled {
            apply.on_click(cx.listener(|this, _, _, cx| this.apply(cx)))
        } else if self.model.stage == Stage::Review && !self.model.install_on() {
            gate::tip_text(apply, SharedString::from(t::costs::setup_install_off()), a)
        } else {
            apply
        };
        div()
            .flex_shrink_0()
            .px(px(SIDE_PADDING))
            .py(px(10.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .fill(Ink::Card, a)
            .border_t(px(1.0))
            .border_color(hsla(Ink::Hairline.color(a)))
            .child(label(&hint, TypeRole::Meta, ink, a).flex_1())
            .children(
                (self.model.stage != Stage::Done
                    && (self.model.stage != Stage::Review || self.model.plan.is_some()))
                .then_some(apply),
            )
    }
}

impl Render for SetupView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let a = window_appearance(window);
        let mut body = div()
            .id("costs-setup-body")
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .p(px(SIDE_PADDING))
            .flex()
            .flex_col()
            .gap(px(14.0));
        let groups = self.model.plan.as_ref().map(|p| p.groups.clone());
        match (self.model.stage, groups) {
            (Stage::Reading, _) | (_, None) => {
                body = body.child(label(
                    if self.model.message.is_some() {
                        t::costs::setup_unreadable()
                    } else {
                        t::costs::setup_reading()
                    },
                    TypeRole::Body,
                    Ink::TextMuted,
                    a,
                ));
            }
            (stage, Some(groups)) => {
                if stage == Stage::Done {
                    body = body
                        .child(self.written_section(a))
                        .child(self.undo_section(a, cx));
                } else {
                    for group in &groups {
                        let title =
                            label(tool_title(group.tool), TypeRole::Section, Ink::TextFaint, a);
                        body = body.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(6.0))
                                .child(title)
                                .child(self.group_view(group, a, cx)),
                        );
                    }
                    body = body.children(self.report_section(a));
                }
            }
        }
        if let Some(message) = self.model.message.as_deref().filter(|m| !m.is_empty()) {
            body = body.child(label(message, TypeRole::Meta, Ink::Needs, a));
        }
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .rounded(px(RADIUS_CARD))
            .overflow_hidden()
            .fill(Ink::Window, a)
            .child(sheet_header(
                Icon::Settings,
                t::costs::setup_sheet_title(),
                Vec::new(),
                a,
                cx,
            ))
            .child(scrollable(body))
            .child(self.footer(a, cx))
            .part("Cost Setup", a, cx)
    }
}
