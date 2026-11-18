use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use gpui::{div, prelude::*, px, Context, Div, SharedString};
use notchling_engine::machines::step::StepId;
use notchling_engine::versions::Tool;
use notchling_kit::metric::{SPACE, SPINNER_SMALL};
use notchling_kit::part::{ButtonKind, PartState};
use notchling_kit::ui::{button, icon, label, spinner};
use notchling_kit::{Appearance, Icon, Ink, TypeRole};
use notchling_locale as t;

use super::model::{ClusterModel, NodeRow};
use super::tool_runs::{Run, ToolRuns};
use super::tool_setup::{correct, setups, Marks, Setup};
use super::tool_updates::{offers, run, start, Offer, State};
use super::updates::machine_of;
use super::view::ClusterSheetView;

const EVERY_SECS: u64 = 5;

#[derive(Clone, Default)]
pub(super) struct ToolView {
    pub(super) offers: Vec<Offer>,
    pub(super) setups: Vec<Setup>,
    corrected: Option<ClusterModel>,
    machines: BTreeMap<String, String>,
}

pub(super) struct ToolUi {
    pub(super) runs: Arc<ToolRuns>,
    pub(super) seen: Option<(u64, (bool, bool), Instant, ToolView)>,
}

impl ToolUi {
    pub(super) fn new(runs: Arc<ToolRuns>) -> ToolUi {
        ToolUi { runs, seen: None }
    }
}

impl ToolView {
    pub(super) fn machine(&self, node: &str) -> Option<&str> {
        self.machines.get(node).map(String::as_str)
    }

    pub(super) fn rows(&self, model: &ClusterModel) -> Vec<NodeRow> {
        self.corrected.as_ref().unwrap_or(model).rows()
    }
}

pub(super) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl ClusterSheetView {
    pub(super) fn tool_view(&mut self, model: &ClusterModel) -> ToolView {
        let switches = self.access.switches.switches();
        let generation = self.feed.generation();
        let gates = (
            switches.is_on(crate::switches::Switch::Install),
            switches.is_on(crate::switches::Switch::Accounts),
        );
        if let Some((seen, was, at, view)) = &self.tools.seen {
            if *seen == generation && *was == gates && at.elapsed().as_secs() < EVERY_SECS {
                return view.clone();
            }
        }
        let view = match self.feed.add_source().parts() {
            None => ToolView::default(),
            Some(parts) => {
                let machines = parts.registry.all();
                let node_of = |name: &str| {
                    parts
                        .engine
                        .results(name)
                        .get(&StepId::NODE_JOINED)
                        .and_then(|r| r.detected.node.clone())
                };
                let tools_of = |name: &str| parts.engine.worker_tools(name);
                let mac = self.feed.mac();
                let now = now_ms();
                let login = |machine: &str, tool: Tool| self.tools.runs.login_mark(machine, tool);
                let installed =
                    |machine: &str, tool: Tool| self.tools.runs.installed_since(machine, tool);
                let reported = |node: &str, tool: Tool| self.feed.reported_at(node, tool);
                let marks = Marks {
                    login: &login,
                    installed: &installed,
                    reported: &reported,
                };
                let fixed = correct(model, &machines, &node_of, &tools_of, &marks, now);
                let model = &fixed;
                let all = model.rows();
                let names = all
                    .iter()
                    .filter_map(|row| {
                        machine_of(row, &all, &machines, &node_of)
                            .map(|m| (row.id.clone(), m.name.clone()))
                    })
                    .collect();
                ToolView {
                    offers: offers(
                        model,
                        &machines,
                        &node_of,
                        &tools_of,
                        mac.as_ref(),
                        &switches,
                    ),
                    setups: setups(model, &machines, &node_of, mac.as_ref(), &switches),
                    corrected: Some(fixed.clone()),
                    machines: names,
                }
            }
        };
        self.tools.seen = Some((generation, gates, Instant::now(), view.clone()));
        view
    }

    pub(super) fn start_tools(&mut self, node: &str, only: Option<Tool>, cx: &mut Context<Self>) {
        self.menu = None;
        let model = self.feed.snapshot();
        let view = self.tool_view(&model);
        let switches = self.access.switches.switches();
        let go = match start(&view.offers, node, only, &model, &switches) {
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
                    let outcome = run(&engine, &runs, &go);
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

    pub(super) fn tool_warning_extras(
        &self,
        view: &ToolView,
        row: &NodeRow,
        warning: &str,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> (Option<Div>, Option<Div>) {
        let Some(offer) = super::tool_updates::for_warning(&view.offers, &row.id, warning) else {
            return (None, None);
        };
        let running = view
            .machine(&row.id)
            .is_some_and(|m| self.tools.runs.running(m));
        let action = offer.ready().then(|| {
            let node = row.id.clone();
            let tool = offer.tool;
            button(
                SharedString::from(format!("node-tool-update-{}-{}", row.id, tool.name())),
                &offer.button(),
                ButtonKind::Secondary,
                if running {
                    PartState::Disabled
                } else {
                    PartState::Rest
                },
                a,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.start_tools(&node, Some(tool), cx);
            }))
        });
        let note = offer.note().map(|text| {
            let ink = match offer.state {
                State::HowTo(_) | State::Busy(_) | State::Off(_) => Ink::TextMuted,
                State::Ready => Ink::Text,
            };
            div()
                .pl(px(SPACE[2]))
                .child(label(text, TypeRole::Meta, ink, a))
        });
        (action.map(|b| div().flex_shrink_0().child(b)), note)
    }

    pub(super) fn tool_run_line(
        &self,
        view: &ToolView,
        row: &NodeRow,
        a: Appearance,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let machine = view.machine(&row.id)?.to_owned();
        match self.tools.runs.get(&machine)? {
            Run::Running(text) => Some(
                div()
                    .flex()
                    .items_center()
                    .gap(px(SPACE[1]))
                    .child(spinner(
                        SharedString::from(format!("node-tool-busy-{}", row.id)),
                        SPINNER_SMALL,
                        Ink::Working,
                        a,
                    ))
                    .child(label(&text, TypeRole::Meta, Ink::TextMuted, a)),
            ),
            Run::Done { ok, text } => {
                let runs = self.tools.runs.clone();
                let dismiss = button(
                    SharedString::from(format!("node-tool-dismiss-{}", row.id)),
                    t::cluster::tool_dismiss(),
                    ButtonKind::Ghost,
                    PartState::Rest,
                    a,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    runs.dismiss(&machine);
                    this.tools.seen = None;
                    cx.notify();
                }));
                Some(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(SPACE[0]))
                        .child(icon(
                            if ok { Icon::Check } else { Icon::Warning },
                            TypeRole::Meta.spec().size,
                            if ok { Ink::Done } else { Ink::Needs },
                            a,
                        ))
                        .child(div().flex_1().min_w(px(0.0)).child(label(
                            &text,
                            TypeRole::Meta,
                            if ok { Ink::TextMuted } else { Ink::Needs },
                            a,
                        )))
                        .child(div().flex_shrink_0().child(dismiss)),
                )
            }
        }
    }
}
