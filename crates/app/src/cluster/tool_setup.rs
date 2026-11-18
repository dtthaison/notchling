use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use notchling_engine::machines::device_login::PromptSink;
use notchling_engine::machines::engine::SetupEngine;
use notchling_engine::machines::engine_plan::{AdminAnswer, PlanKind};
use notchling_engine::machines::registry::Machine;
use notchling_engine::machines::step::FixOutcome;
use notchling_engine::machines::tool_installs::ToolInstall;
use notchling_engine::versions::worker_install::{install_plan, INSTALLABLE};
use notchling_engine::versions::{Tool, ToolState, Version, WorkerTools};
use notchling_locale as t;

use super::model::{ClusterModel, Login, NodeRow};
use super::tool_runs::ToolRuns;
use super::tool_updates::State;
use super::updates::machine_of;
use crate::machines::versions_feed::MacSide;
use crate::switches::{Switch, Switches};

pub const FRESH_MS: i64 = 20 * 60 * 1000;

const CHECKED: [Tool; 3] = [Tool::Claude, Tool::Codex, Tool::Agy];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Install,
    SignIn,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    pub node: String,
    pub machine: String,
    pub tool: Tool,
    pub kind: Kind,
    pub mac: Option<Version>,
    pub note: Option<String>,
    pub state: State,
}

impl Setup {
    pub fn button(&self) -> String {
        match self.kind {
            Kind::Install => t::cluster::tool_install_button(self.tool.title(), &self.machine),
            Kind::SignIn => t::cluster::tool_sign_in_button(&self.machine),
        }
    }

    pub fn ready(&self) -> bool {
        self.state == State::Ready
    }

    pub fn lines(&self) -> Vec<&str> {
        let blocked = match &self.state {
            State::Ready => None,
            State::HowTo(text) | State::Busy(text) | State::Off(text) => Some(text.as_str()),
        };
        blocked.into_iter().chain(self.note.as_deref()).collect()
    }
}

pub fn find<'a>(setups: &'a [Setup], node: &str, tool: Tool, kind: Kind) -> Option<&'a Setup> {
    setups
        .iter()
        .find(|s| s.node == node && s.tool == tool && s.kind == kind)
}

pub trait Corrections {
    fn mark_absent(&mut self, tool: Tool);
    fn mark_present(&mut self, tool: Tool, version: Option<&Version>);
    fn mark_signed_in(&mut self, tool: Tool);
    fn shows_installed(&self, tool: Tool) -> bool;
    fn login_of(&self, tool: Tool) -> Option<Login>;
}

impl Corrections for NodeRow {
    fn mark_absent(&mut self, tool: Tool) {
        let title = tool.title();
        for (name, version) in &mut self.tools {
            if name == title {
                *version = None;
            }
        }
        for (name, login) in &mut self.logins {
            if name == title {
                *login = Login::NotInstalled;
            }
        }
        let gone: Vec<String> = self
            .drifts
            .iter()
            .filter(|d| d.tool == tool)
            .map(|d| d.text.clone())
            .collect();
        self.drifts.retain(|d| d.tool != tool);
        self.warnings
            .retain(|w| !gone.iter().any(|g| w.contains(g)));
        if let Some(versions) = &mut self.versions {
            versions.tools.remove(&tool);
            versions.logins.remove(&tool);
            versions.own_logins.remove(&tool);
        }
    }

    fn mark_present(&mut self, tool: Tool, version: Option<&Version>) {
        let shown = version.map_or_else(|| "installed".to_owned(), |v| v.to_string());
        for (name, seen) in &mut self.tools {
            if name == tool.title() {
                *seen = Some(shown.clone());
            }
        }
        for (name, login) in &mut self.logins {
            if name == tool.title() && *login == Login::NotInstalled {
                *login = Login::Unknown;
            }
        }
    }

    fn mark_signed_in(&mut self, tool: Tool) {
        for (name, login) in &mut self.logins {
            if name == tool.title() && *login != Login::NotInstalled {
                *login = Login::SignedIn;
            }
        }
    }

    fn shows_installed(&self, tool: Tool) -> bool {
        self.versions.is_some()
            && self
                .login_of(tool)
                .is_some_and(|login| login != Login::NotInstalled)
    }

    fn login_of(&self, tool: Tool) -> Option<Login> {
        self.logins
            .iter()
            .find(|(name, _)| name == tool.title())
            .map(|(_, login)| *login)
    }
}

fn known_absent(known: Option<&WorkerTools>, tool: Tool, now_ms: i64) -> bool {
    known.is_some_and(|k| {
        now_ms - k.at_ms <= FRESH_MS
            && k.at_ms > 0
            && matches!(k.tools.get(&tool), Some(ToolState::NotInstalled))
    })
}

pub struct Marks<'a> {
    pub signed: &'a dyn Fn(&str, Tool) -> bool,
    pub installed: &'a dyn Fn(&str, Tool) -> Option<Option<Version>>,
}

pub fn correct(
    model: &ClusterModel,
    machines: &[Machine],
    node_of: &dyn Fn(&str) -> Option<String>,
    tools_of: &dyn Fn(&str) -> Option<WorkerTools>,
    marks: &Marks,
    now_ms: i64,
) -> ClusterModel {
    let all = model.rows();
    model.map_rows(|mut row| {
        if !row.active || row.removed || row.me || !row.online {
            return row;
        }
        let Some(machine) = machine_of(&row, &all, machines, node_of) else {
            return row;
        };
        let known = tools_of(&machine.name);
        for tool in CHECKED {
            if row.shows_installed(tool) && known_absent(known.as_ref(), tool, now_ms) {
                row.mark_absent(tool);
            }
        }
        for tool in CHECKED {
            if !row.shows_installed(tool) && row.versions.is_some() {
                if let Some(version) = (marks.installed)(&machine.name, tool) {
                    row.mark_present(tool, version.as_ref());
                }
            }
            if (marks.signed)(&machine.name, tool) && row.login_of(tool) != Some(Login::SignedIn) {
                row.mark_signed_in(tool);
            }
        }
        row
    })
}

pub fn setups(
    model: &ClusterModel,
    machines: &[Machine],
    node_of: &dyn Fn(&str) -> Option<String>,
    mac: Option<&MacSide>,
    switches: &Switches,
) -> Vec<Setup> {
    let mut out = Vec::new();
    let all = model.rows();
    for row in all.iter() {
        if !row.active || row.removed || row.me || !row.online || row.versions.is_none() {
            continue;
        }
        let Some(machine) = machine_of(row, &all, machines, node_of) else {
            continue;
        };
        for tool in INSTALLABLE {
            let reference = mac.and_then(|m| m.usable(tool));
            if !row.shows_installed(tool) {
                out.push(install_offer(
                    row,
                    &machine.name,
                    tool,
                    reference,
                    model,
                    switches,
                ));
            } else if matches!(
                row.login_of(tool),
                Some(Login::SignedOut) | Some(Login::Unknown)
            ) {
                out.push(sign_in_offer(row, &machine.name, tool, switches));
            }
        }
    }
    out
}

fn install_offer(
    row: &NodeRow,
    machine: &str,
    tool: Tool,
    reference: Option<Version>,
    model: &ClusterModel,
    switches: &Switches,
) -> Setup {
    let planned = install_plan(machine, tool, &ToolState::NotInstalled, reference.as_ref());
    let (mut state, note) = match planned {
        Err(text) => (State::HowTo(text), None),
        Ok(plan) => (State::Ready, plan.note),
    };
    if state == State::Ready {
        if let Some(why) = switches.message(Switch::Install) {
            state = State::Off(why.to_owned());
        } else if model.running_jobs(&row.id) > 0 {
            state = State::Busy(t::cluster::tool_install_busy(
                model.running_jobs(&row.id),
                machine,
            ));
        }
    }
    Setup {
        node: row.id.clone(),
        machine: machine.to_owned(),
        tool,
        kind: Kind::Install,
        mac: reference,
        note,
        state,
    }
}

fn sign_in_offer(row: &NodeRow, machine: &str, tool: Tool, switches: &Switches) -> Setup {
    let state = match switches.message(Switch::Accounts) {
        Some(why) => State::Off(why.to_owned()),
        None => State::Ready,
    };
    Setup {
        node: row.id.clone(),
        machine: machine.to_owned(),
        tool,
        kind: Kind::SignIn,
        mac: None,
        note: None,
        state,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallGo {
    pub machine: String,
    pub tool: Tool,
    pub mac: Option<Version>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignInGo {
    pub node: String,
    pub machine: String,
    pub tool: Tool,
}

pub fn start_install(
    list: &[Setup],
    node: &str,
    tool: Tool,
    model: &ClusterModel,
    switches: &Switches,
) -> Result<InstallGo, String> {
    if let Some(why) = switches.message(Switch::Install) {
        return Err(why.to_owned());
    }
    let Some(offer) = find(list, node, tool, Kind::Install) else {
        return Err(t::cluster::tool_install_nothing().to_owned());
    };
    match &offer.state {
        State::HowTo(text) | State::Busy(text) | State::Off(text) => return Err(text.clone()),
        State::Ready => {}
    }
    let jobs = model.running_jobs(node);
    if jobs > 0 {
        return Err(t::cluster::tool_install_busy(jobs, &offer.machine));
    }
    Ok(InstallGo {
        machine: offer.machine.clone(),
        tool,
        mac: offer.mac.clone(),
    })
}

pub fn start_sign_in(
    list: &[Setup],
    node: &str,
    tool: Tool,
    switches: &Switches,
) -> Result<SignInGo, String> {
    if let Some(why) = switches.message(Switch::Accounts) {
        return Err(why.to_owned());
    }
    let Some(offer) = find(list, node, tool, Kind::SignIn) else {
        return Err(t::cluster::tool_sign_in_nothing().to_owned());
    };
    match &offer.state {
        State::HowTo(text) | State::Busy(text) | State::Off(text) => Err(text.clone()),
        State::Ready => Ok(SignInGo {
            node: node.to_owned(),
            machine: offer.machine.clone(),
            tool,
        }),
    }
}

pub fn run_install(engine: &SetupEngine, runs: &ToolRuns, go: &InstallGo) -> FixOutcome {
    let machine = go.machine.as_str();
    runs.say(machine, &t::cluster::tool_reading(machine));
    if let Err(why) = engine.read_worker_tools(machine) {
        return FixOutcome::fail(why);
    }
    let kind = PlanKind::InstallTool(ToolInstall {
        tool: go.tool,
        mac: go.mac.clone(),
    });
    let shown = match engine.plan(machine, kind) {
        Ok(shown) => shown,
        Err(why) => return FixOutcome::fail(why),
    };
    runs.say(
        machine,
        &t::cluster::tool_installing(go.tool.title(), machine),
    );
    match engine.confirm(shown.id, AdminAnswer::None) {
        Ok(confirmed) => engine.run(confirmed),
        Err(why) => FixOutcome::fail(why),
    }
}

pub fn run_sign_in(
    engine: &SetupEngine,
    runs: &ToolRuns,
    go: &SignInGo,
    prompt: PromptSink,
    cancel: Arc<AtomicBool>,
) -> FixOutcome {
    let machine = go.machine.as_str();
    runs.say(
        machine,
        &t::cluster::tool_signing_in(go.tool.title(), machine),
    );
    engine.sign_in_tool(machine, go.tool, prompt, cancel)
}

pub fn run_check(engine: &SetupEngine, runs: &ToolRuns, machine: &str, tool: Tool) -> FixOutcome {
    runs.say(
        machine,
        &t::cluster::tool_checking_login(tool.title(), machine),
    );
    engine.check_login(machine, tool)
}
