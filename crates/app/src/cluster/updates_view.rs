use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use gpui::{div, prelude::*, px, Context, Div, FocusHandle, KeyDownEvent, SharedString, Window};
use notchling_engine::machines::engine_plan::{AdminAnswer, ShownPlan};
use notchling_engine::machines::node_updates::UpdateRow;
use notchling_engine::machines::step::FixOutcome;
use notchling_kit::labels::Corner;
use notchling_kit::metric::{SPACE, SPINNER_SMALL};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, field, hsla, icon, label, spinner, KitPart};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::add_sheet::AddPassword;
use super::model::ClusterModel;
use super::updates::{banner, commands, execute, lines, plan, run_button, Kind, NotNow};
use super::view::ClusterSheetView;
use crate::machines::runs::{AdminAsks, PendingAdmin};
use crate::machines::view::mono_lines;
use crate::notch::answer::{type_key, Typed};

const SHEET_WIDTH: f32 = 560.0;
const SHEET_MAX_HEIGHT: f32 = super::HEIGHT - 60.0;
const EVERY_SECS: u64 = 5;
const ASK_POLL: Duration = Duration::from_millis(250);

#[derive(Default)]
pub(super) struct UpdateUi {
    hidden: NotNow,
    pub(super) open: bool,
    anyway: BTreeSet<String>,
    plan: Option<ShownPlan>,
    commands: bool,
    running: bool,
    pending: Option<PendingAdmin>,
    password: AddPassword,
    outcome: Option<FixOutcome>,
    asks: AdminAsks,
    focus: Option<FocusHandle>,
    seen: Option<(u64, Instant, Option<String>, Vec<UpdateRow>)>,
}

impl UpdateUi {
    fn offer(
        &mut self,
        feed: &super::feed::ClusterFeed,
        model: &ClusterModel,
    ) -> (Option<String>, Vec<UpdateRow>) {
        let generation = feed.generation();
        if let Some((seen, at, build, rows)) = &self.seen {
            if *seen == generation && at.elapsed().as_secs() < EVERY_SECS {
                return (build.clone(), rows.clone());
            }
        }
        let (build, rows) = feed.add_source().updates(model);
        self.seen = Some((generation, Instant::now(), build.clone(), rows.clone()));
        (build, rows)
    }

    fn password_target(&self) -> String {
        self.pending
            .as_ref()
            .map(|p| p.step.title.clone())
            .unwrap_or_default()
    }
}

impl ClusterSheetView {
    pub(super) fn update_banner_row(
        &mut self,
        model: &ClusterModel,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let (build, rows) = self.update.offer(&self.feed, model);
        if self.update.hidden.hides(build.as_deref()) && !self.update.open {
            return None;
        }
        let shown = banner(&rows)?;
        let open = button(
            "cluster-update-open",
            &shown.button,
            ButtonKind::Secondary,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(|this, _, _, cx| this.open_update(cx)));
        let later = button(
            "cluster-update-not-now",
            t::cluster::update_not_now(),
            ButtonKind::Ghost,
            PartState::Rest,
            a,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            let build = this.update.seen.as_ref().and_then(|s| s.2.clone());
            this.update.hidden.hide(build.as_deref());
            cx.notify();
        }));
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .child(icon(
                    Icon::Info,
                    TypeRole::Meta.spec().size,
                    Ink::AccentText,
                    a,
                ))
                .child(div().flex_1().min_w(px(0.0)).child(label(
                    &shown.text,
                    TypeRole::Meta,
                    Ink::AccentText,
                    a,
                )))
                .child(open)
                .child(later)
                .part("Update Machines", a, cx),
        )
    }

    pub(super) fn preview_update(&mut self, cx: &mut Context<Self>) {
        self.open_update(cx);
        self.update.commands = true;
    }

    pub(super) fn open_update(&mut self, cx: &mut Context<Self>) {
        if self.update.open {
            return;
        }
        self.update.open = true;
        self.update.anyway.clear();
        self.update.outcome = None;
        self.update.commands = false;
        self.update.running = false;
        self.update.pending = None;
        self.update.password.clear();
        self.update.focus = Some(cx.focus_handle());
        self.update.seen = None;
        let feed = self.feed.clone();
        if feed.is_live() {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .spawn(async move { feed.refresh() })
                    .await;
                this.update(cx, |view, cx| {
                    view.update.seen = None;
                    view.replan();
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        self.replan();
        cx.notify();
    }

    fn rows(&mut self) -> Vec<UpdateRow> {
        let model = self.feed.snapshot();
        self.update.offer(&self.feed, &model).1
    }

    fn replan(&mut self) {
        let rows = self.rows();
        let parts = self.feed.add_source().parts();
        if let (Some(old), Some(parts)) = (self.update.plan.take(), parts.as_ref()) {
            parts.engine.cancel(old.id);
        }
        self.update.plan = parts.and_then(|p| plan(&p.engine, &rows, &self.update.anyway).ok());
    }

    fn close_update(&mut self, cx: &mut Context<Self>) {
        if self.update.running {
            self.update.open = false;
            cx.notify();
            return;
        }
        if let (Some(plan), Some(parts)) = (self.update.plan.take(), self.feed.add_source().parts())
        {
            parts.engine.cancel(plan.id);
        }
        self.update.open = false;
        self.update.password.clear();
        cx.notify();
    }

    fn update_anyway(&mut self, machine: &str, cx: &mut Context<Self>) {
        if self.update.running {
            return;
        }
        self.update.anyway.insert(machine.to_owned());
        self.replan();
        cx.notify();
    }

    fn start_update(&mut self, cx: &mut Context<Self>) {
        if self.update.running {
            return;
        }
        let Some(plan) = self.update.plan.take() else {
            return;
        };
        let Some(parts) = self.feed.add_source().parts() else {
            return;
        };
        self.update.running = true;
        self.update.outcome = None;
        let asks = self.update.asks.clone();
        let engine = parts.engine.clone();
        let feed = self.feed.clone();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let outcome = execute(&engine, &plan, &asks);
                    feed.refresh();
                    outcome
                })
                .await;
            this.update(cx, |view, cx| {
                view.update.running = false;
                view.update.pending = None;
                view.update.outcome = Some(outcome);
                view.update.seen = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
        let asks = self.update.asks.clone();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(ASK_POLL).await;
            let again = this.update(cx, |view, cx| {
                if !view.update.running {
                    return false;
                }
                if view.update.pending.is_none() {
                    if let Some(pending) = asks.take() {
                        view.update.pending = Some(pending);
                        cx.notify();
                    }
                }
                true
            });
            if !again.unwrap_or(false) {
                break;
            }
        })
        .detach();
        cx.notify();
    }

    fn answer_update(&mut self, go: bool, cx: &mut Context<Self>) {
        let Some(pending) = self.update.pending.take() else {
            return;
        };
        let answer = match (go, self.update.password.take()) {
            (true, Some(password)) => AdminAnswer::Password(password),
            _ => AdminAnswer::None,
        };
        pending.answer(answer);
        cx.notify();
    }

    fn update_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let k = &event.keystroke;
        let modified = k.modifiers.platform || k.modifiers.control;
        let paste = k.modifiers.platform && !k.modifiers.control && k.key == "v";
        let pasted = paste
            .then(|| cx.read_from_clipboard().and_then(|c| c.text()))
            .flatten()
            .map(|t| t.chars().filter(|c| !c.is_control()).collect::<String>());
        let typed = match pasted {
            Some(text) => {
                self.update.password.field().push_str(&text);
                Typed::Changed
            }
            None => type_key(
                self.update.password.field(),
                &k.key,
                k.key_char.as_deref(),
                modified,
            ),
        };
        match typed {
            Typed::Ignored => return,
            Typed::Changed => {}
            Typed::Submit => self.answer_update(true, cx),
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn update_overlay(
        &mut self,
        window: &Window,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if !self.update.open {
            return None;
        }
        let rows = self.rows();
        let shown = lines(&rows, &self.update.anyway);
        let mut body = div().flex().flex_col().gap(px(SPACE[1])).child(label(
            t::cluster::update_intro(),
            TypeRole::Body,
            Ink::TextMuted,
            a,
        ));
        for line in &shown {
            let ink = match line.kind {
                Kind::Ready => Ink::Done,
                Kind::Chosen => Ink::AccentText,
                Kind::Busy => Ink::Needs,
                Kind::Skipped => Ink::TextMuted,
            };
            let mut row = div().flex().items_center().gap(px(SPACE[1])).child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .child(label(&line.machine, TypeRole::Body, Ink::Text, a))
                    .child(label(&line.text, TypeRole::Meta, ink, a)),
            );
            if line.offers_anyway() {
                let name = line.machine.clone();
                row = row.child(
                    button(
                        SharedString::from(format!("update-anyway-{}", line.machine)),
                        t::cluster::update_anyway(),
                        ButtonKind::Secondary,
                        if self.update.running {
                            PartState::Disabled
                        } else {
                            PartState::Rest
                        },
                        a,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.update_anyway(&name, cx))),
                );
            }
            body = body.child(card(a).p(px(SPACE[1])).child(row));
        }
        if let Some(plan) = &self.update.plan {
            let open = self.update.commands;
            body = body.child(
                div().flex().child(
                    button(
                        "update-commands",
                        if open {
                            t::cluster::add_hide_commands()
                        } else {
                            t::cluster::add_show_commands()
                        },
                        ButtonKind::Ghost,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.update.commands = !this.update.commands;
                        cx.notify();
                    })),
                ),
            );
            if open {
                let text = commands(plan)
                    .into_iter()
                    .map(|(title, shown)| format!("{title}\n{shown}"))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                body = body.child(mono_lines("update-commands-text", &text, 220.0, a));
            }
        }
        if self.update.running && self.update.pending.is_none() {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(SPACE[1]))
                    .child(spinner("update-busy", SPINNER_SMALL, Ink::Working, a))
                    .child(label(
                        t::cluster::update_running(),
                        TypeRole::Body,
                        Ink::TextMuted,
                        a,
                    )),
            );
        }
        if self.update.pending.is_some() {
            body = body.child(self.update_password(window, a, cx));
        }
        if let Some(outcome) = &self.update.outcome {
            body = body.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(SPACE[0]))
                    .child(icon(
                        if outcome.ok {
                            Icon::Check
                        } else {
                            Icon::Warning
                        },
                        TypeRole::Body.spec().size,
                        if outcome.ok { Ink::Done } else { Ink::Needs },
                        a,
                    ))
                    .child(div().flex_1().min_w(px(0.0)).child(label(
                        &outcome.message,
                        TypeRole::Body,
                        if outcome.ok { Ink::Text } else { Ink::Needs },
                        a,
                    ))),
            );
        }
        let (title, enabled) = run_button(&rows, &self.update.anyway);
        let can_run = enabled
            && self.update.plan.is_some()
            && !self.update.running
            && self.update.outcome.is_none();
        let mut buttons = div()
            .pt(px(SPACE[1]))
            .flex()
            .justify_end()
            .gap(px(SPACE[1]));
        buttons = buttons.child(
            button(
                "update-close",
                if self.update.outcome.is_some() {
                    t::cluster::add_finish()
                } else {
                    t::cluster::add_close()
                },
                ButtonKind::Ghost,
                PartState::Rest,
                a,
            )
            .on_click(cx.listener(|this, _, _, cx| this.close_update(cx))),
        );
        buttons = buttons.child(
            button(
                "update-run",
                &title,
                ButtonKind::Primary,
                if can_run {
                    PartState::Rest
                } else {
                    PartState::Disabled
                },
                a,
            )
            .when(can_run, |b| {
                b.on_click(cx.listener(|this, _, _, cx| this.start_update(cx)))
            }),
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
                        .max_h(px(SHEET_MAX_HEIGHT))
                        .p(px(SPACE[3]))
                        .flex()
                        .flex_col()
                        .gap(px(SPACE[1]))
                        .child(label(
                            t::cluster::update_title(),
                            TypeRole::Heading,
                            Ink::Text,
                            a,
                        ))
                        .child(
                            div()
                                .id("update-body")
                                .flex_1()
                                .min_h(px(0.0))
                                .overflow_y_scroll()
                                .child(body),
                        )
                        .child(buttons)
                        .part_at("Update Machines Sheet", Corner::TopTrailing, a, cx),
                ),
        )
    }

    fn update_password(&self, window: &Window, a: Appearance, cx: &mut Context<Self>) -> Div {
        let focused = self
            .update
            .focus
            .as_ref()
            .is_some_and(|f| f.is_focused(window));
        let mut prompt = div().flex().flex_col().gap(px(SPACE[1]));
        prompt = prompt.child(label(
            &t::cluster::update_password_for(&self.update.password_target()),
            TypeRole::Body,
            Ink::Text,
            a,
        ));
        if let Some(handle) = &self.update.focus {
            prompt = prompt.child(
                div()
                    .id("update-password")
                    .w_full()
                    .cursor_text()
                    .track_focus(handle)
                    .on_key_down(
                        cx.listener(|this, event: &KeyDownEvent, _, cx| this.update_key(event, cx)),
                    )
                    .child(field(
                        &self.update.password.dots(),
                        t::cluster::update_password_placeholder(),
                        focused,
                        a,
                    ))
                    .on_click(cx.listener(|this, _, window, _| {
                        if let Some(handle) = &this.update.focus {
                            window.focus(handle);
                        }
                    })),
            );
        }
        prompt.child(
            div()
                .flex()
                .gap(px(SPACE[1]))
                .child(
                    button(
                        "update-password-go",
                        t::cluster::update_continue(),
                        ButtonKind::Primary,
                        if self.update.password.is_empty() {
                            PartState::Disabled
                        } else {
                            PartState::Rest
                        },
                        a,
                    )
                    .when(!self.update.password.is_empty(), |b| {
                        b.on_click(cx.listener(|this, _, _, cx| this.answer_update(true, cx)))
                    }),
                )
                .child(
                    button(
                        "update-password-cancel",
                        t::cluster::add_cancel(),
                        ButtonKind::Ghost,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.answer_update(false, cx))),
                ),
        )
    }
}
