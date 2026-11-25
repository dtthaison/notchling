use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use notchling_engine::machines::device_login::PromptSink;
use notchling_engine::machines::engine::SetupEngine;
use notchling_engine::machines::engine_plan::{AdminAnswer, PlanKind};
use notchling_engine::machines::registry::Machine;
use notchling_engine::machines::step::FixOutcome;
use notchling_engine::machines::tool_installs::ToolInstall;
use notchling_engine::machines::tool_login::{can_sign_in, LoginCheck};
use notchling_engine::versions::worker_install::{install_plan, INSTALLABLE};
use notchling_engine::versions::{Tool, ToolState, Version, WorkerTools};
use notchling_locale as t;

use super::feed::ClusterFeed;
use super::model::{ClusterModel, Login, NodeRow};
use super::tool_runs::{Installed, ToolRuns};
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
    Check,
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
            Kind::Check => t::cluster::tool_check_button(&self.machine),
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
    fn mark_signed_out(&mut self, tool: Tool);
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
        let gone: Vec<String> = self
            .drifts
            .iter()
            .filter(|d| d.tool == tool)
            .map(|d| d.text.clone())
            .collect();
        self.drifts.retain(|d| d.tool != tool);
        self.warnings
            .retain(|w| !gone.iter().any(|g| w.contains(g)));
    }

    fn mark_signed_in(&mut self, tool: Tool) {
        for (name, login) in &mut self.logins {
            if name == tool.title() && *login != Login::NotInstalled {
                *login = Login::SignedIn;
            }
        }
    }

    fn mark_signed_out(&mut self, tool: Tool) {
        for (name, login) in &mut self.logins {
            if name == tool.title() && *login != Login::NotInstalled {
                *login = Login::SignedOut;
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

fn known_absent(known: Option<&WorkerTools>, tool: Tool, now_ms: i64, reported: i64) -> bool {
    known.is_some_and(|k| {
        now_ms - k.at_ms <= FRESH_MS
            && k.at_ms > 0
            && k.at_ms >= reported
            && matches!(k.tools.get(&tool), Some(ToolState::NotInstalled))
    })
}

pub type LoginMark = Option<(i64, bool)>;

pub struct Marks<'a> {
    pub login: &'a dyn Fn(&str, Tool) -> LoginMark,
    pub installed: &'a dyn Fn(&str, Tool) -> Option<Installed>,
    pub reported: &'a dyn Fn(&str, Tool) -> Option<i64>,
}

fn absent_after(known: Option<&WorkerTools>, tool: Tool, at: i64) -> bool {
    known.is_some_and(|k| {
        k.at_ms > at && matches!(k.tools.get(&tool), Some(ToolState::NotInstalled))
    })
}

fn standing(mark: Option<(i64, bool)>, now_ms: i64, reported: i64) -> Option<bool> {
    mark.filter(|(at, _)| now_ms - at <= FRESH_MS && *at >= reported)
        .map(|(_, signed)| signed)
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
        let node_id = row.id.clone();
        let reported_of = |tool: Tool| (marks.reported)(&node_id, tool).unwrap_or(0);
        for tool in CHECKED {
            let reported = reported_of(tool);
            if row.shows_installed(tool) && known_absent(known.as_ref(), tool, now_ms, reported) {
                row.mark_absent(tool);
            }
        }
        for tool in CHECKED {
            let reported = reported_of(tool);
            if row.versions.is_some() {
                let fresh = (marks.installed)(&machine.name, tool).filter(|(at, _)| {
                    now_ms - at <= FRESH_MS
                        && *at >= reported
                        && !absent_after(known.as_ref(), tool, *at)
                });
                if let Some((_, version)) = fresh {
                    row.mark_present(tool, version.as_ref());
                }
            }
            let mark = standing((marks.login)(&machine.name, tool), now_ms, reported);
            match mark {
                Some(true) if row.login_of(tool) != Some(Login::SignedIn) => {
                    row.mark_signed_in(tool)
                }
                Some(false) if row.login_of(tool) != Some(Login::SignedOut) => {
                    row.mark_signed_out(tool)
                }
                _ => {}
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
            } else {
                match row.login_of(tool) {
                    Some(Login::SignedOut) => {
                        out.push(sign_in_offer(row, &machine.name, tool, model, switches))
                    }
                    Some(Login::Unknown) => out.push(check_offer(row, &machine.name, tool)),
                    _ => {}
                }
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

fn sign_in_offer(
    row: &NodeRow,
    machine: &str,
    tool: Tool,
    model: &ClusterModel,
    switches: &Switches,
) -> Setup {
    let state = if let Some(why) = switches.message(Switch::Accounts) {
        State::Off(why.to_owned())
    } else if model.running_jobs(&row.id) > 0 {
        State::Busy(t::cluster::tool_sign_in_busy(
            model.running_jobs(&row.id),
            machine,
        ))
    } else {
        State::Ready
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

fn check_offer(row: &NodeRow, machine: &str, tool: Tool) -> Setup {
    Setup {
        node: row.id.clone(),
        machine: machine.to_owned(),
        tool,
        kind: Kind::Check,
        mac: None,
        note: None,
        state: State::Ready,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallGo {
    pub node: String,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckGo {
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
        node: node.to_owned(),
        machine: offer.machine.clone(),
        tool,
        mac: offer.mac.clone(),
    })
}

pub fn start_sign_in(
    list: &[Setup],
    node: &str,
    tool: Tool,
    model: &ClusterModel,
    switches: &Switches,
) -> Result<SignInGo, String> {
    if let Some(why) = switches.message(Switch::Accounts) {
        return Err(why.to_owned());
    }
    let Some(offer) = find(list, node, tool, Kind::SignIn) else {
        return Err(t::cluster::tool_sign_in_nothing().to_owned());
    };
    match &offer.state {
        State::HowTo(text) | State::Busy(text) | State::Off(text) => return Err(text.clone()),
        State::Ready => {}
    }
    let jobs = model.running_jobs(node);
    if jobs > 0 {
        return Err(t::cluster::tool_sign_in_busy(jobs, &offer.machine));
    }
    Ok(SignInGo {
        node: node.to_owned(),
        machine: offer.machine.clone(),
        tool,
    })
}

pub fn start_check(list: &[Setup], node: &str, tool: Tool) -> Result<CheckGo, String> {
    let Some(offer) = find(list, node, tool, Kind::Check) else {
        return Err(t::cluster::tool_sign_in_nothing().to_owned());
    };
    Ok(CheckGo {
        node: node.to_owned(),
        machine: offer.machine.clone(),
        tool,
    })
}

pub fn idle_text(model: &ClusterModel, node: &str, machine: &str, kind: Kind) -> Option<String> {
    let jobs = model.running_jobs(node);
    (jobs > 0).then(|| match kind {
        Kind::Install => t::cluster::tool_install_busy(jobs, machine),
        Kind::SignIn | Kind::Check => t::cluster::tool_sign_in_busy(jobs, machine),
    })
}

pub fn idle_now(feed: &ClusterFeed, node: &str, machine: &str, kind: Kind) -> Option<FixOutcome> {
    if !feed.refresh() {
        return Some(FixOutcome::fail(t::cluster::tool_refresh_failed(machine)));
    }
    idle_text(&feed.snapshot(), node, machine, kind).map(FixOutcome::fail)
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
        node: go.node.clone(),
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
    engine.sign_in_tool(machine, go.tool, &go.node, prompt, cancel)
}

pub fn run_check(
    engine: &SetupEngine,
    runs: &ToolRuns,
    machine: &str,
    tool: Tool,
    node: &str,
) -> LoginCheck {
    runs.say(
        machine,
        &t::cluster::tool_checking_login(tool.title(), machine),
    );
    engine.check_login(machine, tool, node)
}

pub fn settle_sign_in(
    engine: &SetupEngine,
    runs: &ToolRuns,
    go: &SignInGo,
    id: u64,
    cancel: &Arc<AtomicBool>,
    outcome: &FixOutcome,
    now: &dyn Fn() -> i64,
) {
    let cancelled = cancel.load(Ordering::SeqCst);
    runs.close_session(&go.machine, cancel);
    if cancelled {
        let check = engine.check_login(&go.machine, go.tool, &go.node);
        if let Some(signed) = check.signed {
            runs.mark_login_run(&go.machine, id, go.tool, signed, now());
        }
        let said = t::cluster::tool_sign_in_cancelled(go.tool.title(), &go.machine);
        runs.finish_run(&go.machine, id, &FixOutcome::fail(said));
        return;
    }
    if outcome.ok {
        runs.mark_login_run(&go.machine, id, go.tool, true, now());
    }
    runs.finish_run(&go.machine, id, outcome);
    if runs.is_current(&go.machine, id) {
        runs.clear_sheet_if(&go.machine, go.tool);
    }
}

pub fn settle_check(runs: &ToolRuns, go: &CheckGo, id: u64, check: &LoginCheck, now: i64) {
    if let Some(signed) = check.signed {
        runs.mark_login_run(&go.machine, id, go.tool, signed, now);
    }
    runs.finish_run(&go.machine, id, &check.outcome);
    if check.signed.is_none() && !can_sign_in(go.tool) && runs.is_current(&go.machine, id) {
        runs.set_sheet(&go.machine, go.tool, &go.node);
    }
}
