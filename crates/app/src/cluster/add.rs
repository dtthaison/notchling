use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use notchling_engine::accounts::Secret;
use notchling_engine::machines::address;
use notchling_engine::machines::engine::SetupEngine;
use notchling_engine::machines::engine_plan::{AdminAnswer, AdminNeed, PlanKind, ShownPlan};
use notchling_engine::machines::forms::machine_problems;
use notchling_engine::machines::registry::{
    write_lock, AdminAccess, Machine, MachineOs, NodeChoices, NodeSettings, RegistryState,
};
use notchling_engine::machines::shell::is_safe_machine_path;
use notchling_engine::machines::step::{CheckResult, Detected, StepId};
use notchling_locale as t;

use super::add_parts::{Account, AddParts};
use super::add_plan::{
    master_default, primary_account, rows, says, unsupported, Choice, PlanRow, PlanView,
};
use super::model::ClusterModel;
use crate::machines::model::Registry;

pub const JOIN_WAIT_SECS: f64 = 60.0;

pub use super::add_job::{End, Phase, Probe, Progress, Reply, RunJob, Stop};
use super::add_job::{Facts, Item};

pub struct AddFlow {
    parts: AddParts,
    name: String,
    host: String,
    user: String,
    known: bool,
    checked: bool,
    phase: Phase,
    round: u64,
    acting: bool,
    master_override: Option<bool>,
    facts: Facts,
    rows: Vec<PlanRow>,
    shown: Vec<(PlanRow, Option<ShownPlan>)>,
    blocked: Option<String>,
    needs_password: bool,
    root_confirm: bool,
    job: Option<RunJob>,
    primary: Option<Account>,
    node: Option<String>,
}

impl fmt::Debug for AddFlow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(std::any::type_name::<AddFlow>())
            .field("name", &self.name)
            .field("phase", &self.phase)
            .finish()
    }
}

fn commit(
    registry: &dyn Registry,
    change: impl FnOnce(&mut Vec<Machine>) -> Result<bool, String>,
) -> Result<(), String> {
    let _writing = write_lock();
    let mut list = match registry.load() {
        RegistryState::Loaded { machines, .. } => machines,
        RegistryState::Missing => Vec::new(),
        RegistryState::Unreadable(_) => return Err(t::cluster::add_registry_unreadable().into()),
    };
    if !change(&mut list)? {
        return Ok(());
    }
    if registry.save(&list) {
        Ok(())
    } else {
        Err(t::cluster::add_save_failed().into())
    }
}

impl AddFlow {
    fn start(parts: AddParts, name: &str, host: &str, user: &str, known: bool) -> AddFlow {
        AddFlow {
            parts,
            name: name.into(),
            host: host.into(),
            user: user.into(),
            known,
            checked: false,
            phase: Phase::Address,
            round: 0,
            acting: true,
            master_override: None,
            facts: Facts::default(),
            rows: Vec::new(),
            shown: Vec::new(),
            blocked: None,
            needs_password: false,
            root_confirm: false,
            job: None,
            primary: None,
            node: None,
        }
    }

    pub fn from_address(
        parts: AddParts,
        typed: &str,
        existing: &[String],
    ) -> Result<AddFlow, String> {
        let at = address::parse(typed, &parts.this.user, existing)?;
        Ok(AddFlow::start(parts, &at.name, &at.host, &at.user, false))
    }

    pub fn for_machine(parts: AddParts, name: &str) -> Result<AddFlow, String> {
        let machine = parts
            .registry
            .all()
            .into_iter()
            .find(|m| m.name == name)
            .ok_or_else(|| t::cluster::add_no_machine(name))?;
        commit(&*parts.registry, |list| {
            let Some(m) = list.iter_mut().find(|m| m.name == name) else {
                return Err(t::cluster::add_no_machine(name));
            };
            if m.node.is_some() {
                return Ok(false);
            }
            m.node = Some(NodeSettings::default());
            Ok(true)
        })?;
        Ok(AddFlow::start(
            parts,
            &machine.name,
            &machine.host,
            &machine.user,
            true,
        ))
    }

    pub fn set_name(&mut self, name: &str, existing: &[String]) -> Result<(), String> {
        if self.known || self.checked || self.phase != Phase::Address {
            return Err(t::cluster::add_not_now().into());
        }
        if let Some(first) = machine_problems(name, &self.host, &self.user, existing).first() {
            return Err(first.clone());
        }
        self.name = name.into();
        Ok(())
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn engine(&self) -> Arc<SetupEngine> {
        self.parts.engine.clone()
    }

    pub fn rows(&self) -> &[PlanRow] {
        &self.rows
    }

    pub fn log(&self) -> Vec<String> {
        self.parts.engine.log(&self.name)
    }

    pub fn running_line(&self) -> Option<(String, usize, usize)> {
        match &self.phase {
            Phase::Running { step, of, .. } => Some((self.name.clone(), *step, *of)),
            _ => None,
        }
    }

    fn master(&self) -> Choice {
        let default = master_default(
            self.facts.os,
            self.facts.file_vault,
            &self.user,
            self.facts.home.as_deref(),
            self.primary.as_ref(),
        );
        let on = match self.master_override {
            Some(chosen) if default.editable => chosen,
            _ => default.on,
        };
        Choice { on, ..default }
    }

    fn choices(&self) -> NodeChoices {
        NodeChoices {
            can_be_master: self.master().on,
            acting: self.acting,
        }
    }

    pub fn set_can_be_master(&mut self, on: bool) {
        self.master_override = Some(on);
    }

    pub fn set_acting(&mut self, on: bool) {
        self.acting = on;
    }

    fn cancel_plans(&mut self) {
        for (_, plan) in self.shown.drain(..) {
            if let Some(plan) = plan {
                self.parts.engine.cancel(plan.id);
            }
        }
        self.rows.clear();
        self.blocked = None;
        self.needs_password = false;
        self.root_confirm = false;
    }

    pub fn check(&mut self) -> Vec<Probe> {
        if !matches!(self.phase, Phase::Address | Phase::Plan | Phase::Stopped(_)) {
            return Vec::new();
        }
        self.cancel_plans();
        self.round += 1;
        self.checked = true;
        self.phase = Phase::Checking;
        let mut machine = self
            .parts
            .registry
            .all()
            .into_iter()
            .find(|m| m.name == self.name)
            .unwrap_or_else(|| Machine::new(&self.name, &self.host, &self.user));
        if machine.node.is_none() {
            machine.node = Some(NodeSettings {
                choices: self.choices(),
            });
        }
        machine.jobs.clear();
        vec![Probe::Draft {
            machine: Box::new(machine),
            round: self.round,
        }]
    }

    pub fn reply(&mut self, reply: Reply) {
        if self.phase != Phase::Checking || reply.round != self.round {
            return;
        }
        self.phase = match self.settle(reply.results) {
            Ok(phase) => phase,
            Err(stop) => Phase::Stopped(stop),
        };
    }

    fn unreachable(&self, text: &str) -> Stop {
        let target = format!("{}@{}", self.user, self.host);
        let howto = if says(text, &["host", "key", "verification", "failed"]) {
            t::cluster::add_howto_host_key(&target)
        } else if says(text, &["permission", "denied"]) {
            t::cluster::add_howto_key(&target)
        } else {
            t::cluster::add_howto_reach().to_owned()
        };
        Stop::new(text, true).with_howto(howto)
    }

    fn settle(&mut self, results: BTreeMap<StepId, CheckResult>) -> Result<Phase, Stop> {
        let Some(reach) = results.get(&StepId::REACHABLE) else {
            return Err(Stop::new(t::cluster::add_no_answer(), true));
        };
        if !reach.state.is_pass() {
            return Err(self.unreachable(reach.state.text()));
        }
        let detected = results
            .values()
            .fold(Detected::default(), |all, r| all.merged(&r.detected));
        if let Some(why) = unsupported(&detected) {
            return Err(Stop::new(why, false));
        }
        if let Some(joined) = results.get(&StepId::NODE_JOINED) {
            if joined.state.is_fail() && says(joined.state.text(), &["in", "another", "cluster"]) {
                let why = t::cluster::add_other_cluster(joined.state.text());
                return Err(Stop::new(why, false));
            }
        }
        self.facts = Facts {
            os: detected.os,
            file_vault: detected.file_vault,
            home: detected.home.clone(),
        };
        self.node = detected.node.clone();
        self.register(&detected)?;
        for (id, result) in results {
            self.parts.engine.show(&self.name, id, result);
        }
        self.plan_rows()
    }

    fn register(&self, detected: &Detected) -> Result<(), Stop> {
        let mut machine = Machine::new(&self.name, &self.host, &self.user);
        machine.os = detected.os;
        if machine.os == Some(MachineOs::Linux) {
            machine.set_admin(AdminAccess::RootSsh);
        }
        if let Some(fv) = detected.file_vault {
            machine.file_vault = fv;
        }
        if let Some(mut abilities) = detected.abilities.clone() {
            if let Some(home) = detected.home.as_ref().filter(|h| is_safe_machine_path(h)) {
                abilities.home = Some(home.clone());
            }
            machine.abilities = Some(abilities);
        }
        machine.node = Some(NodeSettings {
            choices: self.choices(),
        });
        commit(&*self.parts.registry, |list| {
            if let Some(there) = list.iter().find(|m| m.name == self.name) {
                if self.known || (there.host == self.host && there.user == self.user) {
                    return Ok(false);
                }
                return Err(t::cluster::add_name_taken(&self.name));
            }
            list.push(machine);
            Ok(true)
        })
        .map_err(|why| Stop::new(why, true))
    }

    fn plan_rows(&mut self) -> Result<Phase, Stop> {
        let engine = &self.parts.engine;
        let machine = self
            .parts
            .registry
            .all()
            .into_iter()
            .find(|m| m.name == self.name)
            .map(|m| engine.overlaid(&m))
            .ok_or_else(|| Stop::new(t::cluster::add_no_machine(&self.name), true))?;
        let mut node_view = machine.clone();
        node_view.jobs.clear();
        let ids = engine.steps(&node_view).into_iter().map(|s| s.id).collect();
        let results = engine.results(&self.name);
        let list = rows(&engine.steps(&machine), &ids, &results);
        if list.is_empty() {
            let joined = results
                .get(&StepId::NODE_JOINED)
                .is_some_and(|r| r.state.is_pass());
            return if joined {
                Ok(Phase::Done)
            } else {
                Err(Stop::new(t::cluster::add_nothing(&self.name), true))
            };
        }
        let mut shown = Vec::new();
        let mut blocked = None;
        let (mut needs_password, mut root_confirm) = (false, false);
        for row in &list {
            if row.howto {
                if row.id == StepId::NODE_TAILSCALE && blocked.is_none() {
                    blocked = Some(t::cluster::add_blocked_tailscale(&self.name));
                }
                shown.push((row.clone(), None));
                continue;
            }
            let plan = engine
                .plan(&self.name, PlanKind::Fix(row.id.clone()))
                .map_err(|why| Stop::new(why, true))?;
            match &plan.admin {
                AdminNeed::Password => needs_password = true,
                AdminNeed::RootConfirm => root_confirm = true,
                AdminNeed::Refused(why) => blocked = blocked.or_else(|| Some(why.clone())),
                AdminNeed::None => {}
            }
            shown.push((row.clone(), Some(plan)));
        }
        self.rows = list;
        self.shown = shown;
        self.blocked = blocked;
        self.needs_password = needs_password;
        self.root_confirm = root_confirm;
        Ok(Phase::Plan)
    }

    pub fn plan(&self) -> Option<PlanView> {
        (self.phase == Phase::Plan).then(|| PlanView {
            rows: self.rows.clone(),
            needs_password: self.needs_password,
            root_confirm: self.root_confirm,
            can_be_master: self.master(),
            acting: Choice {
                on: self.acting,
                editable: true,
                reason: None,
            },
            blocked: self.blocked.clone(),
        })
    }

    fn unchanged(&self) -> Result<(), String> {
        for (_, plan) in &self.shown {
            let Some(plan) = plan else { continue };
            let now = self
                .parts
                .engine
                .plan(&self.name, plan.kind.clone())
                .map_err(|_| t::cluster::add_changed().to_owned())?;
            self.parts.engine.cancel(now.id);
            if now.steps != plan.steps || now.admin != plan.admin {
                return Err(t::cluster::add_changed().to_owned());
            }
        }
        Ok(())
    }

    fn save_choices(&self) -> Result<(), String> {
        let want = self.choices();
        commit(&*self.parts.registry, |list| {
            let Some(m) = list.iter_mut().find(|m| m.name == self.name) else {
                return Err(t::cluster::add_no_machine(&self.name));
            };
            let node = m.node.get_or_insert_with(NodeSettings::default);
            if node.choices == want {
                return Ok(false);
            }
            node.choices = want;
            Ok(true)
        })
    }

    fn stop(&mut self, why: String) -> String {
        self.cancel_plans();
        self.phase = Phase::Stopped(Stop::new(why.clone(), true));
        why
    }

    pub fn run(&mut self, password: Option<Secret>) -> Result<(), String> {
        let Some(view) = self.plan() else {
            return Err(t::cluster::add_not_now().into());
        };
        if let Some(why) = view.blocked {
            return Err(why);
        }
        let password = password.filter(|p| !p.is_empty());
        if view.needs_password && password.is_none() {
            return Err(t::cluster::add_needs_password().into());
        }
        if let Err(why) = self.unchanged() {
            return Err(self.stop(why));
        }
        if let Err(why) = self.save_choices() {
            return Err(self.stop(why));
        }
        let mut items = Vec::new();
        let shown = std::mem::take(&mut self.shown);
        let mut left = shown.into_iter();
        while let Some((row, plan)) = left.next() {
            let Some(plan) = plan else {
                items.push(Item::HowTo {
                    id: row.id,
                    title: row.title,
                    howto: row.display,
                });
                continue;
            };
            let answer = match &plan.admin {
                AdminNeed::Password => password
                    .clone()
                    .map_or(AdminAnswer::None, AdminAnswer::Password),
                AdminNeed::RootConfirm => AdminAnswer::RootConfirmed,
                AdminNeed::None | AdminNeed::Refused(_) => AdminAnswer::None,
            };
            match self.parts.engine.confirm(plan.id, answer) {
                Ok(confirmed) => items.push(Item::Fix {
                    id: row.id,
                    title: row.title,
                    confirmed: Box::new(confirmed),
                }),
                Err(why) => {
                    for (_, rest) in left {
                        if let Some(rest) = rest {
                            self.parts.engine.cancel(rest.id);
                        }
                    }
                    return Err(self.stop(why));
                }
            }
        }
        let title = items
            .first()
            .map(|i| match i {
                Item::Fix { title, .. } | Item::HowTo { title, .. } => title.clone(),
            })
            .unwrap_or_default();
        self.phase = Phase::Running {
            step: 1,
            of: items.len(),
            title,
        };
        self.job = Some(RunJob {
            machine: self.name.clone(),
            items,
        });
        Ok(())
    }

    pub fn take_job(&mut self) -> Option<RunJob> {
        self.job.take()
    }

    pub fn progress(&mut self, progress: Progress) {
        if let Phase::Running { .. } = self.phase {
            self.phase = Phase::Running {
                step: progress.step,
                of: progress.of,
                title: progress.title,
            };
        }
    }

    pub fn finished(&mut self, end: End, now: f64) {
        if !matches!(self.phase, Phase::Running { .. }) {
            return;
        }
        self.phase = match end {
            End::Joined => {
                let node = self
                    .parts
                    .engine
                    .results(&self.name)
                    .get(&StepId::NODE_JOINED)
                    .and_then(|r| r.detected.node.clone());
                self.node = node.or(self.node.take());
                Phase::Joining { since: now }
            }
            End::Stopped(stop) => Phase::Stopped(stop),
        };
    }

    pub fn seen(&mut self, model: &ClusterModel, now: f64) {
        if !model.is_loaded() {
            return;
        }
        self.primary = primary_account(model, &self.parts.this);
        let Phase::Joining { since } = self.phase else {
            return;
        };
        let rows = model.rows();
        let by_id = self
            .node
            .as_ref()
            .and_then(|id| rows.iter().find(|r| &r.id == id && !r.removed));
        let member = by_id.or_else(|| rows.iter().find(|r| r.name == self.name && !r.removed));
        if member.is_some_and(|r| r.active && r.online) {
            self.phase = Phase::Done;
        } else if now - since > JOIN_WAIT_SECS {
            self.phase = Phase::Stopped(Stop::new(t::cluster::add_join_timeout(&self.name), true));
        }
    }
}
