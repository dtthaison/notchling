use gpui::{div, prelude::*, px, Context, Div};
use notchling_kit::labels::Corner;
use notchling_kit::metric::{SPACE, SPINNER_SMALL};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, hsla, icon, label, spinner, KitPart, KitStyled};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::model::ClusterModel;
use super::move_sheet::{
    Button, MoveSheet, MoveView, Note, Phase, SessionLine, StepLine, StepState,
};
use super::promote::{MoveKind, PromoteSheet};
use super::view::ClusterSheetView;

const MOVE_MAX_HEIGHT: f32 = 640.0;

impl ClusterSheetView {
    pub(super) fn open_move(&mut self, id: &str, cx: &mut Context<Self>) {
        self.menu = None;
        let model = self.feed.snapshot();
        let name = model.row(id).map(|r| r.name.clone()).unwrap_or_default();
        let primary = model
            .primary_row()
            .map(|r| r.name.clone())
            .unwrap_or_default();
        match model.move_kind(id) {
            Some(MoveKind::Handover) => {
                self.move_sheet = Some(MoveSheet::new(id, &name).with_from(&primary));
            }
            Some(MoveKind::Promote) => {
                self.promote_sheet = Some(PromoteSheet::new(id, &name, &primary));
            }
            None => {}
        }
        cx.notify();
    }

    pub(super) fn show_move(&mut self, cx: &mut Context<Self>) {
        let model = self.feed.snapshot();
        let Some(status) = model.move_status().filter(|m| m.running()) else {
            return;
        };
        let name = model
            .row(&status.to)
            .map(|r| r.name.clone())
            .unwrap_or_default();
        let primary = model
            .primary_row()
            .map(|r| r.name.clone())
            .unwrap_or_default();
        self.move_sheet = Some(MoveSheet::running(&status.to, &name).with_from(&primary));
        cx.notify();
    }

    pub(super) fn move_running_line(
        &self,
        model: &ClusterModel,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let status = model.move_status().filter(|m| m.running())?;
        let name = model
            .row(&status.to)
            .map(|r| r.name.clone())
            .unwrap_or_default();
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
                        "cluster-move-show",
                        t::cluster::move_show(),
                        ButtonKind::Secondary,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.show_move(cx))),
                ),
        )
    }

    fn close_move(&mut self, cx: &mut Context<Self>) {
        self.move_sheet = None;
        self.promote_sheet = None;
        cx.notify();
    }

    fn start_move(&mut self, cx: &mut Context<Self>) {
        if self.promote_sheet.is_some() {
            self.start_promote(cx);
            return;
        }
        let Some(sheet) = self.move_sheet.as_mut() else {
            return;
        };
        if sheet.phase != Phase::Confirm {
            return;
        }
        let Some(request) = self.feed.snapshot().move_home(&sheet.to) else {
            sheet.phase = Phase::Refused(t::cluster::move_unavailable().to_owned());
            cx.notify();
            return;
        };
        sheet.phase = Phase::Starting;
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
                if let Some(sheet) = view.move_sheet.as_mut() {
                    sheet.phase = match result {
                        Ok(_) => Phase::Running,
                        Err(error) => Phase::Refused(error),
                    };
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn move_now(&mut self, cx: &mut Context<Self>) {
        let Some(request) = self.feed.snapshot().move_now() else {
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
                if let Some(sheet) = view.move_sheet.as_mut() {
                    sheet.failure = result.err();
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn move_button(&self, which: Button, a: Appearance, cx: &mut Context<Self>) -> Div {
        let (id, title, kind) = match which {
            Button::Start => ("move-start", t::cluster::move_start(), ButtonKind::Primary),
            Button::Anyway => (
                "move-anyway",
                t::cluster::move_anyway(),
                ButtonKind::Primary,
            ),
            Button::Cancel => ("move-cancel", t::cluster::move_cancel(), ButtonKind::Ghost),
            Button::MoveNow => ("move-now", t::cluster::move_now(), ButtonKind::Secondary),
            Button::Close => ("move-close", t::cluster::move_close(), ButtonKind::Ghost),
        };
        let clickable = button(id, title, kind, PartState::Rest, a).on_click(cx.listener(
            move |this, _, _, cx| match which {
                Button::Start | Button::Anyway => this.start_move(cx),
                Button::MoveNow => this.move_now(cx),
                Button::Cancel | Button::Close => this.close_move(cx),
            },
        ));
        div().child(clickable)
    }

    pub(super) fn move_overlay(
        &self,
        model: &ClusterModel,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let (view, waiting) = match (&self.promote_sheet, &self.move_sheet) {
            (Some(sheet), _) => (sheet.view(model), sheet.waiting()),
            (None, Some(sheet)) => (
                sheet.view(model.move_status(), &self.names),
                sheet.phase == Phase::Starting,
            ),
            (None, None) => return None,
        };
        let mut buttons = div()
            .pt(px(SPACE[1]))
            .flex()
            .justify_end()
            .gap(px(SPACE[1]));
        for which in &view.buttons {
            buttons = buttons.child(self.move_button(*which, a, cx));
        }
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
                        .w(px(460.0))
                        .max_h(px(MOVE_MAX_HEIGHT))
                        .p(px(SPACE[3]))
                        .flex()
                        .flex_col()
                        .gap(px(SPACE[1]))
                        .child(label(&view.title, TypeRole::Heading, Ink::Text, a))
                        .child(
                            div()
                                .id("move-body")
                                .flex_1()
                                .min_h(px(0.0))
                                .overflow_y_scroll()
                                .flex()
                                .flex_col()
                                .gap(px(SPACE[1]))
                                .children(
                                    view.intro
                                        .as_deref()
                                        .map(|text| label(text, TypeRole::Body, Ink::TextMuted, a)),
                                )
                                .children(waiting.then(|| {
                                    spinner("move-starting", SPINNER_SMALL, Ink::TextMuted, a)
                                }))
                                .children(session_block(t::cluster::move_moving(), &view.moving, a))
                                .children(session_block(
                                    t::cluster::move_staying(),
                                    &view.staying,
                                    a,
                                ))
                                .children(steps_block(&view, a))
                                .children(view.notes.iter().map(|note| note_line(note, a))),
                        )
                        .child(buttons)
                        .part_at("Move Sheet", Corner::TopTrailing, a, cx),
                ),
        )
    }
}

fn session_block(title: &str, lines: &[SessionLine], a: Appearance) -> Option<Div> {
    if lines.is_empty() {
        return None;
    }
    let mut block = div().flex().flex_col().gap(px(SPACE[0] / 2.0)).child(label(
        title,
        TypeRole::Label,
        Ink::TextFaint,
        a,
    ));
    for line in lines {
        let mut row = div()
            .flex()
            .flex_col()
            .child(label(&line.name, TypeRole::Body, Ink::Text, a).elide());
        if let Some(note) = &line.note {
            row = row.child(label(note, TypeRole::Meta, Ink::TextMuted, a));
        }
        block = block.child(row);
    }
    Some(block)
}

fn steps_block(view: &MoveView, a: Appearance) -> Option<Div> {
    if view.steps.is_empty() {
        return None;
    }
    let mut block = div().flex().flex_col().gap(px(SPACE[0]));
    for step in &view.steps {
        block = block.child(step_line(step, a));
    }
    Some(block)
}

fn step_line(step: &StepLine, a: Appearance) -> Div {
    let (glyph, ink) = match step.state {
        StepState::Done => (Icon::Check, Ink::Done),
        StepState::Problem => (Icon::Warning, Ink::Needs),
        StepState::Current => (Icon::Working, Ink::Working),
        StepState::Waiting => (Icon::Clock, Ink::TextFaint),
    };
    let text_ink = if step.state == StepState::Waiting {
        Ink::TextFaint
    } else {
        Ink::Text
    };
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .child(icon(glyph, TypeRole::Body.spec().size, ink, a))
        .child(label(&step.text, TypeRole::Body, text_ink, a))
}

fn note_line(note: &Note, a: Appearance) -> Div {
    let ink = if note.problem {
        Ink::Needs
    } else {
        Ink::TextMuted
    };
    div()
        .flex()
        .items_start()
        .gap(px(SPACE[0]))
        .when(note.problem, |row| {
            row.child(icon(
                Icon::Warning,
                TypeRole::Meta.spec().size,
                Ink::Needs,
                a,
            ))
        })
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .child(label(&note.text, TypeRole::Meta, ink, a)),
        )
}
