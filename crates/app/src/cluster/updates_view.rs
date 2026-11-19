use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{div, prelude::*, px, Context, Div, FocusHandle, KeyDownEvent, SharedString, Window};
use notchling_engine::machines::node_updates::UpdateRow;
use notchling_kit::labels::Corner;
use notchling_kit::metric::{SPACE, SPINNER_SMALL};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, field, hsla, icon, label, spinner, KitPart};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::add_sheet::AddPassword;
use super::model::ClusterModel;
use super::updates::{banner, commands, execute, lines, Kind};
use super::updates_state::{Offered, UpdateSession};
use super::view::ClusterSheetView;
use crate::machines::view::mono_lines;
use crate::notch::answer::{type_key, Typed};
use crate::sheets::header::{card_title, close_button};
use crate::vault::pick::VaultPick;
use crate::vault::pick_view::{pick_block, PickEvent};

const SHEET_WIDTH: f32 = 560.0;
const SHEET_MAX_HEIGHT: f32 = super::HEIGHT - 60.0;
const EVERY_SECS: u64 = 5;
const ASK_POLL: Duration = Duration::from_millis(250);

pub(super) struct UpdateUi {
    session: Arc<UpdateSession>,
    pub(super) open: bool,
    anyway: BTreeSet<String>,
    offered: Offered,
    commands: bool,
    password: AddPassword,
    vault: VaultPick,
    vault_error: Option<String>,
    focus: Option<FocusHandle>,
    seen: Option<(u64, Instant, Option<String>, Vec<UpdateRow>)>,
}

impl UpdateUi {
    pub(super) fn vault_loaded(
        &mut self,
        result: Result<Vec<notchling_engine::vault::entry::VaultEntry>, String>,
    ) {
        self.vault.loaded(result);
    }

    pub(super) fn vault_reread(&mut self) {
        self.vault.reread();
    }

    pub(super) fn new(session: Arc<UpdateSession>) -> UpdateUi {
        UpdateUi {
            session,
            open: false,
            anyway: BTreeSet::new(),
            offered: Offered::default(),
            commands: false,
            password: AddPassword::default(),
            vault: VaultPick::default(),
            vault_error: None,
            focus: None,
            seen: None,
        }
    }

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
        self.session.pending_target().unwrap_or_default()
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
        if self.update.session.hides(build.as_deref()) && !self.update.open {
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
            this.update.session.hide(build.as_deref());
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
        self.update.commands = false;
        self.update.password.clear();
        self.update.vault_error = None;
        self.update.focus = Some(cx.focus_handle());
        self.update.seen = None;
        let parts = self.feed.add_source().parts();
        self.update
            .offered
            .reset(parts.as_ref().map(|p| &*p.engine));
        if self.update.session.running() {
            self.watch_asks(cx);
        }
        let feed = self.feed.clone();
        if feed.is_live() {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .spawn(async move { feed.refresh() })
                    .await;
                this.update(cx, |view, cx| {
                    view.update.seen = None;
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    fn rows(&mut self) -> Vec<UpdateRow> {
        let model = self.feed.snapshot();
        self.update.offer(&self.feed, &model).1
    }

    fn sync_plan(&mut self, rows: &[UpdateRow]) {
        if self.update.session.running() {
            return;
        }
        if let Some(parts) = self.feed.add_source().parts() {
            self.update
                .offered
                .sync(&parts.engine, rows, &self.update.anyway);
        }
    }

    fn close_update(&mut self, cx: &mut Context<Self>) {
        self.update.session.dismissed();
        if !self.update.session.running() {
            let parts = self.feed.add_source().parts();
            self.update
                .offered
                .reset(parts.as_ref().map(|p| &*p.engine));
        }
        self.update.password.clear();
        self.update.vault_error = None;
        self.update.open = false;
        cx.notify();
    }

    fn update_anyway(&mut self, machine: &str, cx: &mut Context<Self>) {
        if self.update.session.running() {
            return;
        }
        self.update.anyway.insert(machine.to_owned());
        cx.notify();
    }

    fn start_update(&mut self, cx: &mut Context<Self>) {
        let Some(parts) = self.feed.add_source().parts() else {
            return;
        };
        if self.update.session.running() {
            return;
        }
        let Some(plan) = self.update.offered.take() else {
            return;
        };
        if !self.update.session.begin() {
            return;
        }
        let session = self.update.session.clone();
        let asks = session.asks();
        let engine = parts.engine.clone();
        let feed = self.feed.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .spawn(async move {
                    let outcome = execute(&engine, &plan, &asks);
                    feed.refresh();
                    session.finish(outcome);
                })
                .await;
            this.update(cx, |view, cx| {
                view.update.seen = None;
                view.update.password.clear();
                cx.notify();
            })
            .ok();
        })
        .detach();
        self.watch_asks(cx);
        cx.notify();
    }

    fn watch_asks(&mut self, cx: &mut Context<Self>) {
        let session = self.update.session.clone();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(ASK_POLL).await;
            let (running, changed) = session.pull();
            let alive = this.update(cx, |view, cx| {
                if !running {
                    view.update.seen = None;
                    view.update.password.clear();
                }
                if changed || !running {
                    cx.notify();
                }
            });
            if !running || alive.is_err() {
                break;
            }
        })
        .detach();
    }

    pub(super) fn update_vault_event(
        &mut self,
        event: PickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            PickEvent::Toggle => {
                self.update.vault_error = None;
                if self.update.vault.toggle() {
                    self.read_vault_entries(cx);
                }
            }
            PickEvent::Choose(id) => {
                let Some(machine) = self.update.session.pending_machine() else {
                    return;
                };
                let Some(parts) = self.feed.add_source().parts() else {
                    return;
                };
                match parts.engine.set_admin_secret(&machine, Some(&id)) {
                    Ok(()) => {
                        self.update.vault_error = None;
                        self.update.vault.close();
                        self.update
                            .session
                            .answer_vault(&machine, &id, &mut self.update.password);
                    }
                    Err(why) => self.update.vault_error = Some(t::cluster::vault_save_failed(&why)),
                }
            }
            PickEvent::Clear => self.update.vault.close(),
            PickEvent::New => self.open_new_entry(window, cx),
        }
        cx.notify();
    }

    fn answer_update(&mut self, go: bool, cx: &mut Context<Self>) {
        if self.update.session.answer(go, &mut self.update.password) {
            cx.notify();
        }
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
        self.sync_plan(&rows);
        let running = self.update.session.running();
        let outcome = self.update.session.outcome();
        let asking = self.update.session.pending_target().is_some();
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
                        if running {
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
        if let Some(plan) = self.update.offered.shown() {
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
        if running && !asking {
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
        if asking {
            body = body.child(self.update_password(window, a, cx));
        }
        if let Some(outcome) = &outcome {
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
        let (title, enabled) = self.update.offered.button();
        let can_run = enabled && !running && outcome.is_none();
        let mut buttons = div()
            .pt(px(SPACE[1]))
            .flex()
            .justify_end()
            .gap(px(SPACE[1]));
        let close = close_button("update-close", a, cx, |this: &mut Self, _, cx| {
            this.close_update(cx)
        });
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
                        .child(card_title(
                            label(t::cluster::update_title(), TypeRole::Heading, Ink::Text, a),
                            close,
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
        prompt = prompt.children(
            self.update
                .vault_error
                .as_deref()
                .map(|why| label(why, TypeRole::Meta, Ink::Needs, a)),
        );
        prompt = prompt.child(pick_block(
            "update",
            &self.update.vault,
            None,
            true,
            a,
            cx,
            ClusterSheetView::update_vault_event,
        ));
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
