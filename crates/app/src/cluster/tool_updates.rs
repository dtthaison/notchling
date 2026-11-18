use notchling_engine::machines::engine::SetupEngine;
use notchling_engine::machines::engine_plan::{AdminAnswer, PlanKind};
use notchling_engine::machines::registry::Machine;
use notchling_engine::machines::step::FixOutcome;
use notchling_engine::machines::tool_updates::ToolMove;
use notchling_engine::versions::worker_update::{plan, unread_how_to, Move};
use notchling_engine::versions::{Tool, ToolState, Version, WorkerTools};
use notchling_locale as t;

use super::model::{ClusterModel, ToolDrift};
use super::tool_runs::ToolRuns;
use super::updates::machine_of;
use crate::machines::versions_feed::MacSide;
use crate::switches::{Switch, Switches};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Ready,
    HowTo(String),
    Busy(String),
    Off(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub node: String,
    pub machine: String,
    pub tool: Tool,
    pub found: Version,
    pub to: Version,
    pub text: String,
    pub state: State,
}

impl Offer {
    pub fn down(&self) -> bool {
        self.found > self.to
    }

    pub fn button(&self) -> String {
        let tool = self.tool.title();
        let to = self.to.as_str();
        if self.down() {
            t::cluster::tool_update_down(tool, &self.machine, to)
        } else {
            t::cluster::tool_update_button(tool, &self.machine, to)
        }
    }

    pub fn ready(&self) -> bool {
        self.state == State::Ready
    }

    pub fn note(&self) -> Option<&str> {
        match &self.state {
            State::Ready => None,
            State::HowTo(text) | State::Busy(text) | State::Off(text) => Some(text),
        }
    }
}

pub fn busy_text(machine: &str, jobs: usize) -> String {
    t::cluster::tool_busy(jobs, machine)
}

fn state_for(
    machine: &Machine,
    drift: &ToolDrift,
    to: &Version,
    tools: Option<&WorkerTools>,
) -> Option<State> {
    if &drift.found == to {
        return None;
    }
    Some(match tools {
        Some(known) => {
            let seen = known
                .tools
                .get(&drift.tool)
                .unwrap_or(&ToolState::NotInstalled);
            match plan(&machine.name, drift.tool, seen, to) {
                Ok(Move::Run(_)) => State::Ready,
                Ok(Move::HowTo(text)) => State::HowTo(text),
                Err(_) => return None,
            }
        }
        None => match unread_how_to(&machine.name, drift.tool, to) {
            Some(text) => State::HowTo(text),
            None => State::Ready,
        },
    })
}

pub fn offers(
    model: &ClusterModel,
    machines: &[Machine],
    node_of: &dyn Fn(&str) -> Option<String>,
    tools_of: &dyn Fn(&str) -> Option<WorkerTools>,
    mac: Option<&MacSide>,
    switches: &Switches,
) -> Vec<Offer> {
    let Some(mac) = mac else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for row in model.rows() {
        if !row.active || row.removed || row.me || !row.online {
            continue;
        }
        let Some(machine) = machine_of(&row, machines, node_of) else {
            continue;
        };
        let known = tools_of(&machine.name);
        for drift in &row.drifts {
            let Some(to) = mac.usable(drift.tool) else {
                continue;
            };
            let Some(mut state) = state_for(machine, drift, &to, known.as_ref()) else {
                continue;
            };
            if state == State::Ready {
                if let Some(why) = switches.message(Switch::Install) {
                    state = State::Off(why.to_owned());
                } else if model.running_jobs(&row.id) > 0 {
                    state = State::Busy(busy_text(&machine.name, model.running_jobs(&row.id)));
                }
            }
            out.push(Offer {
                node: row.id.clone(),
                machine: machine.name.clone(),
                tool: drift.tool,
                found: drift.found.clone(),
                to,
                text: drift.text.clone(),
                state,
            });
        }
    }
    out
}

pub fn for_warning<'a>(offers: &'a [Offer], node: &str, text: &str) -> Option<&'a Offer> {
    offers.iter().find(|o| o.node == node && o.text == text)
}

pub fn offers_runnable(offers: &[Offer], node: &str) -> bool {
    offers
        .iter()
        .any(|o| o.node == node && !matches!(o.state, State::HowTo(_)))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Start {
    pub machine: String,
    pub moves: Vec<ToolMove>,
}

pub fn start(
    offers: &[Offer],
    node: &str,
    only: Option<Tool>,
    model: &ClusterModel,
    switches: &Switches,
) -> Result<Start, String> {
    if let Some(why) = switches.message(Switch::Install) {
        return Err(why.to_owned());
    }
    let mine: Vec<&Offer> = offers
        .iter()
        .filter(|o| o.node == node && only.is_none_or(|t| t == o.tool))
        .collect();
    let Some(first) = mine.first() else {
        return Err(t::cluster::tool_nothing().to_owned());
    };
    let moves: Vec<ToolMove> = mine
        .iter()
        .filter(|o| !matches!(o.state, State::HowTo(_)))
        .map(|o| ToolMove {
            tool: o.tool,
            to: o.to.clone(),
        })
        .collect();
    if moves.is_empty() {
        let why = mine
            .iter()
            .find_map(|o| o.note().map(str::to_owned))
            .unwrap_or_else(|| t::cluster::tool_nothing().to_owned());
        return Err(why);
    }
    let jobs = model.running_jobs(node);
    if jobs > 0 {
        return Err(busy_text(&first.machine, jobs));
    }
    Ok(Start {
        machine: first.machine.clone(),
        moves,
    })
}

pub fn run(engine: &SetupEngine, runs: &ToolRuns, go: &Start) -> FixOutcome {
    let machine = go.machine.as_str();
    runs.say(machine, &t::cluster::tool_reading(machine));
    if let Err(why) = engine.read_worker_tools(machine) {
        return FixOutcome::fail(why);
    }
    let shown = match engine.plan(machine, PlanKind::UpdateTools(go.moves.clone())) {
        Ok(shown) => shown,
        Err(why) => return FixOutcome::fail(why),
    };
    let names: Vec<&str> = go.moves.iter().map(|m| m.tool.title()).collect();
    runs.say(
        machine,
        &t::cluster::tool_updating(&names.join(", "), machine),
    );
    match engine.confirm(shown.id, AdminAnswer::None) {
        Ok(confirmed) => engine.run(confirmed),
        Err(why) => FixOutcome::fail(why),
    }
}
