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
    run_check, run_install, run_sign_in, start_install, start_sign_in, Kind, Setup,
};
use super::tool_updates_view::{now_ms, LoginSheet, ToolView};
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
                    let outcome = run_install(&engine, &runs, &go);
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
        let go = match start_sign_in(&view.setups, node, tool, &switches) {
            Ok(go) => go,
            Err(why) => {
                self.note = Some((why, true));
                cx.notify();
                return;
            }
        };
        self.note = None;
        if !can_sign_in(tool) {
            self.tools.sheet = Some(LoginSheet {
                machine: go.machine,
                tool,
            });
            cx.notify();
            return;
        }
        let Some(parts) = self.feed.add_source().parts() else {
            return;
        };
        let runs = self.tools.runs.clone();
        if runs.running(&go.machine) {
            return;
        }
        let Some(cancel) = runs.open_session(&go.machine, tool) else {
            return;
        };
        let text = t::cluster::tool_signing_in(tool.title(), &go.machine);
        if !runs.begin(&go.machine, &text) {
            runs.close_session(&go.machine);
            return;
        }
        self.tools.sheet = Some(LoginSheet {
            machine: go.machine.clone(),
            tool,
        });
        let engine = parts.engine.clone();
        let feed = self.feed.clone();
        cx.spawn(async move |this, cx| {
            let machine = go.machine.clone();
            cx.background_executor()
                .spawn(async move {
                    let offered = runs.clone();
                    let to = go.machine.clone();
                    let sink: PromptSink = Arc::new(move |prompt| offered.offer(&to, prompt));
                    let outcome = run_sign_in(&engine, &runs, &go, sink, cancel);
                    runs.close_session(&go.machine);
                    if outcome.ok {
                        runs.mark_signed_in(&go.machine, go.tool, now_ms());
                    }
                    runs.finish(&go.machine, &outcome);
                    feed.refresh();
                })
                .await;
            this.update(cx, |view, cx| {
                if view
                    .tools
                    .sheet
                    .as_ref()
                    .is_some_and(|s| s.machine == machine)
                {
                    view.tools.sheet = None;
                }
                view.tools.seen = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn check_login(&mut self, cx: &mut Context<Self>) {
        let Some(sheet) = self.tools.sheet.take() else {
            return;
        };
        let Some(parts) = self.feed.add_source().parts() else {
            return;
        };
        let runs = self.tools.runs.clone();
        let text = t::cluster::tool_checking_login(sheet.tool.title(), &sheet.machine);
        if !runs.begin(&sheet.machine, &text) {
            return;
        }
        let engine = parts.engine.clone();
        let feed = self.feed.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .spawn(async move {
                    let outcome = run_check(&engine, &runs, &sheet.machine, sheet.tool);
                    if outcome.ok {
                        runs.mark_signed_in(&sheet.machine, sheet.tool, now_ms());
                    }
                    runs.finish(&sheet.machine, &outcome);
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
        };
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
                .on_click(cx.listener(move |this, _, _, cx| match kind {
                    Kind::Install => this.start_install(&node, tool, cx),
                    Kind::SignIn => this.start_sign_in(&node, tool, cx),
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
        let sheet = self.tools.sheet.as_ref()?;
        let machine = sheet.machine.clone();
        let tool = sheet.tool;
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
            let session = self.tools.runs.session(&machine)?;
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
                    this.tools.runs.cancel(&to);
                    this.tools.sheet = None;
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
                        this.tools.sheet = None;
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
                    .on_click(cx.listener(|this, _, _, cx| this.check_login(cx))),
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
