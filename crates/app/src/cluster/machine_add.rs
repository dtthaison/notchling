use std::sync::atomic::{AtomicU64, Ordering};

use notchling_engine::machines::engine::SetupEngine;
use notchling_engine::machines::registry::{AdminAccess, Machine, MachineOs};
use notchling_engine::machines::step::{CheckResult, StepId};
use notchling_locale as t;

use super::add::{commit, unreachable_stop};
use super::add_job::Stop;
use super::add_parts::AddParts;
use super::add_plan::unsupported;
use super::add_register::learn;
use super::add_sheet::{parse_draft, Draft};
use super::model::ClusterModel;
use super::not_joined::not_joined;
use crate::machines::model::MachinesModel;
use crate::machines::pieces::DraftCheck;

static ROUNDS: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Form,
    Checking,
    Saved { name: String, note: Option<String> },
    Stopped(Stop),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    name: String,
    host: String,
    user: String,
    organization: Option<String>,
}

#[derive(Debug)]
pub struct MachineAdd {
    phase: Phase,
    round: u64,
    target: Option<Target>,
}

#[derive(Clone, Debug)]
pub struct Probe {
    round: u64,
    name: String,
    host: String,
    user: String,
    existing: Vec<String>,
}

#[derive(Debug)]
pub struct Answer {
    round: u64,
    check: DraftCheck,
}

impl Probe {
    #[cfg(test)]
    pub fn with_existing(mut self, existing: &[&str]) -> Probe {
        self.existing = existing.iter().map(|n| (*n).to_owned()).collect();
        self
    }

    pub fn run(&self, engine: &SetupEngine) -> Answer {
        Answer {
            round: self.round,
            check: MachinesModel::draft_check(
                engine,
                &self.name,
                &self.host,
                &self.user,
                &self.existing,
            ),
        }
    }
}

impl Default for MachineAdd {
    fn default() -> MachineAdd {
        MachineAdd::new()
    }
}

impl MachineAdd {
    pub fn new() -> MachineAdd {
        MachineAdd {
            phase: Phase::Form,
            round: 0,
            target: None,
        }
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn name(&self) -> Option<&str> {
        self.target.as_ref().map(|t| t.name.as_str())
    }

    pub fn begin(
        &mut self,
        parts: &AddParts,
        draft: &Draft,
        organization: Option<String>,
    ) -> Result<Probe, String> {
        if !matches!(self.phase, Phase::Form | Phase::Stopped(_)) {
            return Err(t::cluster::add_not_now().into());
        }
        let existing = parts.names();
        let parsed = parse_draft(draft, &parts.this.user, &existing)?;
        let (user, host) = parsed
            .target
            .split_once('@')
            .map(|(u, h)| (u.to_owned(), h.to_owned()))
            .ok_or_else(|| t::cluster::add_not_now().to_owned())?;
        self.round = ROUNDS.fetch_add(1, Ordering::SeqCst) + 1;
        self.phase = Phase::Checking;
        self.target = Some(Target {
            name: parsed.name.clone(),
            host: host.clone(),
            user: user.clone(),
            organization: organization.filter(|o| !o.trim().is_empty()),
        });
        Ok(Probe {
            round: self.round,
            name: parsed.name,
            host,
            user,
            existing,
        })
    }

    pub fn answered(&mut self, parts: &AddParts, answer: Answer) -> bool {
        if self.phase != Phase::Checking || answer.round != self.round {
            return false;
        }
        let Some(target) = self.target.clone() else {
            return false;
        };
        let check = answer.check;
        if !check.reachable {
            self.phase = Phase::Stopped(match check.lines.first() {
                Some(line) => unreachable_stop(&target.user, &target.host, line.state.text()),
                None => match check.problems.first() {
                    Some(problem) => Stop::new(problem.clone(), false),
                    None => {
                        unreachable_stop(&target.user, &target.host, t::cluster::add_no_answer())
                    }
                },
            });
            return false;
        }
        match save(parts, &target, &check) {
            Ok(()) => {
                let note = unsupported(&check.detected);
                if let Some(line) = check.lines.first() {
                    parts.engine.show(
                        &target.name,
                        StepId::REACHABLE,
                        CheckResult::of(line.state.clone()).detected(check.detected.clone()),
                    );
                }
                self.phase = Phase::Saved {
                    name: target.name,
                    note,
                };
                true
            }
            Err(why) => {
                self.phase = Phase::Stopped(Stop::new(why, true));
                false
            }
        }
    }
}

fn save(parts: &AddParts, target: &Target, check: &DraftCheck) -> Result<(), String> {
    commit(&*parts.registry, |list| {
        if list.iter().any(|m| m.name == target.name) {
            return Err(t::cluster::add_name_taken(&target.name));
        }
        let mut machine = Machine::new(&target.name, &target.host, &target.user);
        learn(&mut machine, &check.detected);
        if machine.os == Some(MachineOs::Linux) {
            machine.set_admin(AdminAccess::RootSsh);
        }
        machine.organization = target.organization.clone();
        list.push(machine);
        Ok(true)
    })
}

pub fn remove(parts: &AddParts, model: &ClusterModel, name: &str) -> Result<(), String> {
    if !model.is_loaded() {
        return Err(t::cluster::machine_remove_unread().to_owned());
    }
    let machines = parts.registry.all();
    let joined = |n: &str| parts.engine.results(n).get(&StepId::NODE_JOINED).cloned();
    let outside = not_joined(&machines, &joined, model);
    let known = machines.iter().any(|m| m.name == name);
    if known && !outside.iter().any(|m| m.name == name) {
        return Err(t::cluster::machine_remove_member(name));
    }
    commit(&*parts.registry, |list| {
        let before = list.len();
        list.retain(|m| m.name != name);
        if list.len() == before {
            return Err(t::cluster::add_no_machine(name));
        }
        Ok(true)
    })?;
    parts.engine.forget(name);
    Ok(())
}
