use std::collections::BTreeMap;
use std::fmt;

use notchling_engine::machines::engine::SetupEngine;
use notchling_engine::machines::engine_plan::{AdminAnswer, Confirmed};
use notchling_engine::machines::registry::{Machine, MachineOs};
use notchling_engine::machines::step::{CheckResult, StepId};
use notchling_locale as t;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stop {
    pub reason: String,
    pub howto: Option<String>,
    pub check_again: bool,
}

impl Stop {
    pub(super) fn new(reason: impl Into<String>, check_again: bool) -> Stop {
        Stop {
            reason: reason.into(),
            howto: None,
            check_again,
        }
    }

    pub(super) fn with_howto(mut self, howto: String) -> Stop {
        self.howto = Some(howto);
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Address,
    Checking,
    Plan,
    Running {
        step: usize,
        of: usize,
        title: String,
    },
    Joining {
        since: f64,
    },
    Done,
    Stopped(Stop),
}

#[derive(Clone, Debug)]
pub enum Probe {
    Draft {
        machine: Box<Machine>,
        round: u64,
        sd: bool,
    },
}

pub struct Reply {
    pub(super) round: u64,
    pub(super) results: BTreeMap<StepId, CheckResult>,
}

impl fmt::Debug for Reply {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(std::any::type_name::<Reply>())
            .field("round", &self.round)
            .field("steps", &self.results.len())
            .finish()
    }
}

impl Probe {
    pub fn run(&self, engine: &SetupEngine) -> Reply {
        let Probe::Draft { machine, round, sd } = self;
        Reply {
            round: *round,
            results: engine.check_draft_join(machine, *sd),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct Facts {
    pub(super) os: Option<MachineOs>,
    pub(super) file_vault: Option<bool>,
    pub(super) home: Option<String>,
}

pub(super) enum Item {
    Fix {
        id: StepId,
        title: String,
        confirmed: Box<Confirmed>,
    },
    HowTo {
        id: StepId,
        title: String,
        howto: String,
    },
}

pub struct RunJob {
    pub(super) machine: String,
    pub(super) items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    pub step: usize,
    pub of: usize,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum End {
    Joined,
    Stopped(Stop),
}

impl RunJob {
    pub(super) fn fixes(&self) -> usize {
        self.items
            .iter()
            .filter(|i| matches!(i, Item::Fix { .. }))
            .count()
    }

    pub fn execute(self, engine: &SetupEngine, report: &dyn Fn(Progress)) -> End {
        let of = self.fixes();
        let mut fixed = 0;
        let result = |id: &StepId| engine.results(&self.machine).get(id).cloned();
        for item in self.items {
            match item {
                Item::HowTo { id, title, howto } => {
                    let now = result(&id);
                    if now.as_ref().is_some_and(|r| r.state.is_pass()) {
                        continue;
                    }
                    let line = now.map(|r| r.state.text().to_owned()).unwrap_or_default();
                    let why = t::cluster::add_step_not_done(&title, &line);
                    return End::Stopped(Stop::new(why, true).with_howto(howto));
                }
                Item::Fix {
                    id,
                    title,
                    confirmed,
                } => {
                    if result(&id).is_some_and(|r| r.state.is_pass()) {
                        continue;
                    }
                    fixed += 1;
                    report(Progress {
                        step: fixed,
                        of,
                        title: title.clone(),
                    });
                    let outcome = engine.run_with(*confirmed, &|_| AdminAnswer::None);
                    if !outcome.ok {
                        let why = t::cluster::add_run_failed(&title, &outcome.message);
                        return End::Stopped(Stop::new(why, true));
                    }
                    if id == StepId::NODE_JOINED {
                        continue;
                    }
                    if let Some(r) = result(&id).filter(|r| !r.state.is_pass()) {
                        let why = t::cluster::add_step_not_done(&title, r.state.text());
                        return End::Stopped(Stop::new(why, true));
                    }
                }
            }
        }
        End::Joined
    }
}
