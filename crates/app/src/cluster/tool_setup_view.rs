use std::sync::atomic::Ordering;
use std::sync::Arc;

use gpui::{div, prelude::*, px, Context, Div, SharedString};
use notchling_engine::machines::device_login::{is_device_link, PromptSink};
use notchling_engine::machines::tool_login::{can_sign_in, how_to_text, LOGIN_SECS};
use notchling_engine::versions::{Tool, ToolState};
use notchling_kit::metric::{SPACE, SPINNER_SMALL};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, card, hsla, label, spinner};
use notchling_kit::{Appearance, Ink, TypeRole};
use notchling_locale as t;

use super::model::NodeRow;
use super::tool_setup::{
    idle_now, run_check, run_install, run_sign_in, start_check, start_install, start_sign_in, Kind,
    Setup,
};
use super::tool_updates_view::{now_ms, ToolView};
use super::view::ClusterSheetView;
use crate::accounts::view_rows::mono;

impl ClusterSheetView {
    pub(super) fn start_install(&mut self, node: &str, tool: Tool, cx: &mut Context<Self>) {
        self.menu = None;
        let model = self.feed.snapshot();
        let view = self.tool_view(&model);
        let switches = self.access.switches.switches();
        let go = match start_install(&view.setups, node, tool, &model, &switches) {
            Ok(go) => go,
            Err(why) => {
                self.note = Some((why, true));
                cx.notify();
                return;
            }
        };
        let Some(parts) = self.feed.add_source().parts() else {
            return;
        };
        let runs = self.tools.runs.clone();
        if !runs.begin(&go.machine, &t::cluster::tool_reading(&go.machine)) {
            return;
        }
        self.note = None;
        let engine = parts.engine.clone();
        let feed = self.feed.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .spawn(async move {
                    let outcome = match idle_now(&feed, &go.node, &go.machine, Kind::Install) {
                        Some(busy) => busy,
                        None => run_install(&engine, &runs, &go),
                    };
                    if outcome.ok {
                        let version =
                            engine.worker_tools(&go.machine).and_then(|known| {
                                match known.tools.get(&go.tool) {
                                    Some(ToolState::Installed { version, .. }) => version.clone(),
                                    _ => None,
                                }
                            });
                        runs.mark_installed(&go.machine, go.tool, version, now_ms());
                    }
                    runs.finish(&go.machine, &outcome);
                    feed.refresh();
                })
                .await;
            this.update(cx, |view, cx| {
                view.tools.seen = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn start_sign_in(&mut self, node: &str, tool: Tool, cx: &mut Context<Self>) {
        self.menu = None;
        let model = self.feed.snapshot();
        let view = self.tool_view(&model);
        let switches = self.access.switches.switches();
        let go = match start_sign_in(&view.setups, node, tool, &model, &switches) {
            Ok(go) => go,
            Err(why) => {
                self.note = Some((why, true));
                cx.notify();
                return;
            }
        };
        let runs = self.tools.runs.clone();
        if runs.running(&go.machine) || runs.sheet().is_some() {
            return;
        }
        self.note = None;
        if !can_sign_in(tool) {
            runs.set_sheet(&go.machine, tool);
            cx.notify();
            return;
        }
        let Some(parts) = self.feed.add_source().parts() else {
            return;
        };
        let Some(cancel) = runs.open_session(&go.machine, tool) else {
            return;
        };
        let text = t::cluster::tool_signing_in(tool.title(), &go.machine);
        if !runs.begin(&go.machine, &text) {
            runs.close_session(&go.machine, &cancel);
            return;
        }
        runs.set_sheet(&go.machine, tool);
        let engine = parts.engine.clone();
        let feed = self.feed.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .spawn(async move {
                    let offered = runs.clone();
                    let to = go.machine.clone();
                    let owner = cancel.clone();
                    let sink: PromptSink =
                        Arc::new(move |prompt| offered.offer(&to, &owner, prompt));
                    let outcome = match idle_now(&feed, &go.node, &go.machine, Kind::SignIn) {
                        Some(busy) => busy,
                        None => run_sign_in(&engine, &runs, &go, sink, cancel.clone()),
                    };
                    let cancelled = cancel.load(Ordering::SeqCst);
                    runs.close_session(&go.machine, &cancel);
                    if cancelled {
                        let check = engine.check_login(&go.machine, go.tool);
                        if let Some(signed) = check.signed {
                            runs.mark_login(&go.machine, go.tool, signed, now_ms());
                        }
                    } else {
                        if outcome.ok {
                            runs.mark_login(&go.machine, go.tool, true, now_ms());
                        }
                        runs.finish(&go.machine, &outcome);
                        runs.clear_sheet_if(&go.machine, go.tool);
                    }
                    feed.refresh();
                })
                .await;
            this.update(cx, |view, cx| {
                view.tools.seen = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn start_check(&mut self, node: &str, tool: Tool, cx: &mut Context<Self>) {
        self.menu = None;
        let model = self.feed.snapshot();
        let view = self.tool_view(&model);
        match start_check(&view.setups, node, tool) {
            Ok(go) => self.check_in_background(go.machine, go.tool, false, cx),
            Err(why) => {
                self.note = Some((why, true));
                cx.notify();
            }
        }
    }

    fn check_from_sheet(&mut self, cx: &mut Context<Self>) {
        let Some((machine, tool)) = self.tools.runs.sheet() else {
            return;
        };
        self.check_in_background(machine, tool, true, cx);
    }

    fn check_in_background(
        &mut self,
        machine: String,
        tool: Tool,
        from_sheet: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(parts) = self.feed.add_source().parts() else {
            return;
        };
        let runs = self.tools.runs.clone();
        let text = t::cluster::tool_checking_login(tool.title(), &machine);
        if !runs.begin(&machine, &text) {
            return;
        }
        if from_sheet {
            runs.clear_sheet_if(&machine, tool);
        }
        self.note = None;
        let engine = parts.engine.clone();
        let feed = self.feed.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .spawn(async move {
                    let check = run_check(&engine, &runs, &machine, tool);
                    if let Some(signed) = check.signed {
                        runs.mark_login(&machine, tool, signed, now_ms());
                    }
                    runs.finish(&machine, &check.outcome);
                    feed.refresh();
                })
                .await;
            this.update(cx, |view, cx| {
                view.tools.seen = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn setup_lines(
        &self,
        view: &ToolView,
        row: &NodeRow,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Vec<Div> {
        let running = view
            .machine(&row.id)
            .is_some_and(|m| self.tools.runs.running(m));
        let mut seen: Vec<String> = Vec::new();
        let mut out = Vec::new();
        for setup in view.setups.iter().filter(|s| s.node == row.id) {
            out.push(self.setup_line(setup, running, &mut seen, a, cx));
        }
        out
    }

    fn setup_line(
        &self,
        setup: &Setup,
        running: bool,
        seen: &mut Vec<String>,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Div {
        let node = setup.node.clone();
        let tool = setup.tool;
        let kind = setup.kind;
        let id = match kind {
            Kind::Install => "node-tool-install",
            Kind::SignIn => "node-tool-sign-in",
            Kind::Check => "node-tool-check",
        };
        let machine = setup.machine.clone();
        let mut line = div().flex().items_center().gap(px(SPACE[1])).child(label(
            tool.title(),
            TypeRole::Meta,
            Ink::TextMuted,
            a,
        ));
        if setup.ready() {
            line = line.child(
                button(
                    SharedString::from(format!("{id}-{}-{}", setup.node, tool.name())),
                    &setup.button(),
                    ButtonKind::Secondary,
                    if running {
                        PartState::Disabled
                    } else {
                        PartState::Rest
                    },
                    a,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.tools.runs.running(&machine) {
                        return;
                    }
                    match kind {
                        Kind::Install => this.start_install(&node, tool, cx),
                        Kind::SignIn => this.start_sign_in(&node, tool, cx),
                        Kind::Check => this.start_check(&node, tool, cx),
                    }
                })),
            );
        }
        let mut block = div().flex().flex_col().gap(px(SPACE[0] / 2.0)).child(line);
        for text in setup.lines() {
            if seen.iter().any(|s| s == text) {
                continue;
            }
            seen.push(text.to_owned());
            block = block.child(div().pl(px(SPACE[2])).child(label(
                text,
                TypeRole::Meta,
                Ink::TextMuted,
                a,
            )));
        }
        block
    }

    pub(super) fn login_overlay(&self, a: Appearance, cx: &mut Context<Self>) -> Option<Div> {
        let (machine, tool) = self.tools.runs.sheet()?;
        let mut inner = div().flex().flex_col().gap(px(SPACE[1])).child(label(
            &t::cluster::login_title(tool.title(), &machine),
            TypeRole::Heading,
            Ink::Text,
            a,
        ));
        let mut buttons = div()
            .pt(px(SPACE[1]))
            .flex()
            .justify_end()
            .gap(px(SPACE[1]));
        if can_sign_in(tool) {
            let Some(session) = self.tools.runs.session(&machine) else {
                self.tools.runs.clear_sheet_if(&machine, tool);
                return None;
            };
            match &session.prompt {
                None => {
                    inner = inner.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(SPACE[1]))
                            .child(spinner(
                                SharedString::from("node-login-starting"),
                                SPINNER_SMALL,
                                Ink::Working,
                                a,
                            ))
                            .child(label(
                                &t::cluster::login_starting(&machine),
                                TypeRole::Body,
                                Ink::TextMuted,
                                a,
                            )),
                    );
                }
                Some(prompt) => {
                    inner = inner
                        .child(label(
                            t::cluster::login_link_label(),
                            TypeRole::Meta,
                            Ink::TextMuted,
                            a,
                        ))
                        .child(mono(prompt.url(), a))
                        .child(label(
                            t::cluster::login_code_label(),
                            TypeRole::Meta,
                            Ink::TextMuted,
                            a,
                        ))
                        .child(mono(prompt.code(), a))
                        .child(label(
                            &t::cluster::login_replaces(&machine),
                            TypeRole::Body,
                            Ink::Needs,
                            a,
                        ))
                        .child(label(
                            &t::cluster::login_steps(&machine),
                            TypeRole::Body,
                            Ink::TextMuted,
                            a,
                        ))
                        .child(label(
                            &t::cluster::login_waiting(&(LOGIN_SECS / 60).to_string()),
                            TypeRole::Meta,
                            Ink::TextMuted,
                            a,
                        ))
                        .child(label(
                            t::cluster::login_private(),
                            TypeRole::Meta,
                            Ink::TextFaint,
                            a,
                        ));
                }
            }
            let to = machine.clone();
            buttons = buttons.child(
                button(
                    "node-login-cancel",
                    t::cluster::login_cancel(),
                    ButtonKind::Ghost,
                    PartState::Rest,
                    a,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.tools
                        .runs
                        .cancel(&to, &t::cluster::tool_sign_in_cancelled(tool.title(), &to));
                    this.tools.seen = None;
                    cx.notify();
                })),
            );
            if let Some(prompt) = &session.prompt {
                let url = prompt.url().to_owned();
                buttons = buttons.child(
                    button(
                        "node-login-open",
                        t::cluster::login_open(),
                        ButtonKind::Primary,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(move |_, _, _, cx| {
                        if is_device_link(&url) {
                            cx.open_url(&url);
                        }
                    })),
                );
            }
        } else {
            inner = inner.child(label(
                &how_to_text(tool, &machine),
                TypeRole::Body,
                Ink::TextMuted,
                a,
            ));
            buttons = buttons
                .child(
                    button(
                        "node-login-close",
                        t::cluster::login_close(),
                        ButtonKind::Ghost,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tools.runs.clear_sheet();
                        cx.notify();
                    })),
                )
                .child(
                    button(
                        "node-login-check",
                        t::cluster::login_check(),
                        ButtonKind::Primary,
                        PartState::Rest,
                        a,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.check_from_sheet(cx))),
                );
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
                        .w(px(440.0))
                        .p(px(SPACE[3]))
                        .flex()
                        .flex_col()
                        .gap(px(SPACE[1]))
                        .child(inner)
                        .child(buttons),
                ),
        )
    }
}
