//! Plan → confirm → run (the entry points of Swift `SetupEngine`'s Fixes, made stricter).
//!
//! Nothing changes a machine unless its exact commands were shown first:
//!
//! 1. [`SetupEngine::plan`] builds what an action would run and keeps it under a fresh random
//!    [`PlanId`]. It runs nothing. The window shows [`ShownPlan::steps`]' displays.
//! 2. [`SetupEngine::confirm`] takes that id (once: a second confirm of the same id fails) and the
//!    admin answer, and gives a [`Confirmed`]: not `Clone`, fields private, made nowhere else.
//! 3. [`SetupEngine::run`] consumes it, builds the plan again from the machine as it is now, and
//!    runs only when every display is the one shown; else it says the step needs a look.
//!
//! Every Fix, Replace key, the Running switch, Send again and Update N machines go this way.

use super::engine::{lock, SetupEngine};

use super::registry::{AdminAccess, Machine};

use super::shell;
use super::step::{FixOutcome, SetupStep, StepFix, StepId};
use super::usage::SecretUsage;
use crate::accounts::Secret;

/// A plan the engine showed. 64 random bits: not a counter a caller could guess.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlanId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanKind {
    /// One step's Set up.
    Fix(StepId),
    /// The Deploy key's Replace key.
    Replace(StepId),
    SetUpAll,
    /// The Running switch, to on (`true`) or off.
    Running(bool),
    SendAgain(SecretUsage),
    /// Update N machines with a renewed vault value.
    Update(String),
}

/// What an action needs before it may run with admin rights.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdminNeed {
    None,
    /// Through sudo: the admin password, for this one step.
    Password,
    /// As root over ssh: the user's confirmation, behind a warning.
    RootConfirm,
    /// Never on this machine (Linux, the worker as admin).
    Refused(String),
}

/// One step of a shown plan: what it will run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovedStep {
    pub id: StepId,
    pub title: String,
    pub display: String,
    /// Waiting for an earlier step (Set up all).
    pub waiting: bool,
    pub needs_admin: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShownPlan {
    pub id: PlanId,
    /// Empty for Update, which spans machines.
    pub machine: String,
    pub kind: PlanKind,
    pub title: String,
    pub steps: Vec<ApprovedStep>,
    pub admin: AdminNeed,
}

impl ShownPlan {
    /// Every command it shows, as the sheet prints them.
    pub fn display(&self) -> String {
        self.steps
            .iter()
            .map(|s| format!("# {}\n{}", s.title, s.display))
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

pub(super) struct ShownRecord {
    pub(super) plan: ShownPlan,
    at: f64,
}

/// How long a shown plan may wait for its confirmation.
pub const PLAN_LIFETIME: f64 = 30.0 * 60.0;
const MAX_SHOWN: usize = 64;

/// The admin answer to a plan or to one step of Set up all.
pub enum AdminAnswer {
    None,
    Password(Secret),
    RootConfirmed,
}

impl AdminAnswer {
    pub(super) fn password(&self) -> Option<&Secret> {
        match self {
            AdminAnswer::Password(p) if !p.is_empty() => Some(p),
            _ => None,
        }
    }
}

/// A shown plan the user confirmed. Only [`SetupEngine::confirm`] makes one and [`SetupEngine::run`]
/// takes it by value, so one confirmation runs once.
pub struct Confirmed {
    plan: ShownPlan,
    admin: AdminAnswer,
}

impl Confirmed {
    pub fn plan(&self) -> &ShownPlan {
        &self.plan
    }
}

fn fail(m: impl Into<String>) -> FixOutcome {
    FixOutcome::fail(m)
}

fn random_id() -> Option<u64> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut b = [0u8; 8];
    SystemRandom::new().fill(&mut b).ok()?;
    Some(u64::from_le_bytes(b))
}

impl SetupEngine {
    // MARK: plan → confirm → run

    /// What `kind` on `machine` would run, kept for [`SetupEngine::confirm`]. Runs nothing.
    pub fn plan(&self, machine: &str, kind: PlanKind) -> Result<ShownPlan, String> {
        let mut plan = self.build(machine, &kind)?;
        let id = random_id().ok_or("no randomness for the plan id")?;
        plan.id = PlanId(id);
        let now = (self.deps.clock)();
        let mut s = lock(&self.state);
        s.shown.retain(|_, r| now - r.at < PLAN_LIFETIME);
        while s.shown.len() >= MAX_SHOWN {
            let Some(oldest) = s
                .shown
                .iter()
                .min_by(|a, b| a.1.at.total_cmp(&b.1.at))
                .map(|(k, _)| *k)
            else {
                break;
            };
            s.shown.remove(&oldest);
        }
        s.shown.insert(
            id,
            ShownRecord {
                plan: plan.clone(),
                at: now,
            },
        );
        Ok(plan)
    }

    /// Forgets a shown plan (the sheet was cancelled).
    pub fn cancel(&self, id: PlanId) {
        lock(&self.state).shown.remove(&id.0);
    }

    /// The user confirmed the plan `id` was shown as; `admin` answers its [`AdminNeed`].
    pub fn confirm(&self, id: PlanId, admin: AdminAnswer) -> Result<Confirmed, String> {
        let record = lock(&self.state)
            .shown
            .remove(&id.0)
            .ok_or("that plan was not shown, or it ran already; Set up again")?;
        if (self.deps.clock)() - record.at >= PLAN_LIFETIME {
            return Err("that plan is too old; Set up again".into());
        }
        let plan = record.plan;
        match (&plan.admin, &admin) {
            (AdminNeed::Refused(why), _) => return Err(why.clone()),
            (AdminNeed::Password, a) if a.password().is_none() => {
                return Err("needs the admin password".into())
            }
            (AdminNeed::RootConfirm, a) if !matches!(a, AdminAnswer::RootConfirmed) => {
                return Err(format!(
                    "needs your confirmation to run as root on {}",
                    plan.machine
                ))
            }
            _ => {}
        }
        Ok(Confirmed { plan, admin })
    }

    /// Runs a confirmed plan. Set up all's admin steps are refused; use [`SetupEngine::run_with`].
    pub fn run(&self, c: Confirmed) -> FixOutcome {
        self.run_with(c, &|_| AdminAnswer::None)
    }

    /// Runs a confirmed plan; `ask_admin` answers each admin step of Set up all at its turn.
    pub fn run_with(
        &self,
        c: Confirmed,
        ask_admin: &dyn Fn(&ApprovedStep) -> AdminAnswer,
    ) -> FixOutcome {
        let Confirmed { plan, admin } = c;
        if plan.kind == PlanKind::SetUpAll {
            return self.set_up_all(&plan.machine, &plan.steps, ask_admin);
        }
        // the plan as it is now: a refresh may have learned another home or python since it showed
        match self.build(&plan.machine, &plan.kind) {
            Ok(now) if now.steps == plan.steps && now.admin == plan.admin => {}
            _ => return fail(format!("{} needs a look; Set up again", plan.title)),
        }
        let display = plan
            .steps
            .first()
            .map(|s| s.display.clone())
            .unwrap_or_default();
        match &plan.kind {
            PlanKind::Fix(id) => self.run_fix(&plan.machine, id, &admin, &display),
            PlanKind::Replace(id) => self.run_replace(&plan.machine, id, &display),
            PlanKind::Running(on) => self.toggle_running(&plan.machine, *on, &admin, &display),
            PlanKind::SendAgain(usage) => self.send_again(usage),
            PlanKind::Update(secret) => {
                let outcomes = self.update_machines(secret);
                let bad: Vec<String> = outcomes
                    .iter()
                    .filter(|o| !o.ok)
                    .map(|o| o.message.clone())
                    .collect();
                if bad.is_empty() {
                    FixOutcome::ok(format!(
                        "{} updated on {} machine(s)",
                        secret,
                        outcomes.len()
                    ))
                } else {
                    fail(bad.join("\n"))
                }
            }
            PlanKind::SetUpAll => fail("Set up all runs on its own"),
        }
    }

    fn admin_need(m: &Machine, needs: bool) -> AdminNeed {
        if !needs {
            return AdminNeed::None;
        }
        if let Some(why) = shell::admin_refusal(m) {
            return AdminNeed::Refused(why.into());
        }
        match m.admin() {
            AdminAccess::RootSsh => AdminNeed::RootConfirm,
            AdminAccess::Sudo => AdminNeed::Password,
        }
    }

    pub(super) fn find_step(&self, m: &Machine, id: &StepId) -> Result<SetupStep, String> {
        self.step_list(m)
            .into_iter()
            .find(|s| &s.id == id)
            .ok_or(format!("{} has no step {}", m.name, id.raw()))
    }

    /// The plan of `kind` as it is now, with no id yet.
    fn build(&self, machine: &str, kind: &PlanKind) -> Result<ShownPlan, String> {
        let shown =
            |m: &Machine, title: String, steps: Vec<ApprovedStep>, admin: AdminNeed| ShownPlan {
                id: PlanId(0),
                machine: m.name.clone(),
                kind: kind.clone(),
                title,
                steps,
                admin,
            };
        if let PlanKind::Update(secret) = kind {
            let offers = self
                .update_offers()
                .get(secret)
                .cloned()
                .unwrap_or_default();
            if offers.is_empty() {
                return Err(format!("no machine waits for {secret}"));
            }
            let mut steps = Vec::new();
            for usage in &offers {
                steps.push(
                    self.delivery_step(
                        usage,
                        format!("Update {} on {}", usage.name, usage.machine),
                    )?,
                );
            }
            return Ok(ShownPlan {
                id: PlanId(0),
                machine: String::new(),
                kind: kind.clone(),
                title: format!("Update {} machine(s)", steps.len()),
                steps,
                admin: AdminNeed::None,
            });
        }
        let m = self
            .machine(machine)
            .ok_or(format!("no machine named {machine}"))?;
        match kind {
            PlanKind::Fix(id) => {
                let step = self.find_step(&m, id)?;
                let plan = match &step.fix {
                    StepFix::Run(p) => p,
                    StepFix::HowTo(text) => {
                        return Err(format!("{} has no Fix. {text}", step.title))
                    }
                    StepFix::Toggle { .. } => {
                        return Err(format!("{} is a switch, not a Fix", step.title))
                    }
                };
                let a = ApprovedStep {
                    id: id.clone(),
                    title: step.title.clone(),
                    display: plan.display.clone(),
                    waiting: false,
                    needs_admin: plan.needs_admin,
                };
                Ok(shown(
                    &m,
                    step.title.clone(),
                    vec![a],
                    Self::admin_need(&m, plan.needs_admin),
                ))
            }
            PlanKind::Replace(id) => {
                let step = self.find_step(&m, id)?;
                let plan = step.replace.clone().ok_or(format!(
                    "{} has nothing to replace in {}",
                    m.name,
                    id.raw()
                ))?;
                if !(self.catalog.offers_replace)(self.results(&m.name).get(id)) {
                    return Err(format!(
                        "{} is offered when the Deploy key check passes or the key is not on GitHub; Check again",
                        step.replace_title()
                    ));
                }
                let a = ApprovedStep {
                    id: id.clone(),
                    title: step.replace_title(),
                    display: plan.display,
                    waiting: false,
                    needs_admin: false,
                };
                Ok(shown(&m, step.replace_title(), vec![a], AdminNeed::None))
            }
            PlanKind::SetUpAll => {
                let steps = self.set_up_all_steps(&m);
                if steps.is_empty() {
                    return Err("nothing to set up".into());
                }
                Ok(shown(
                    &m,
                    format!("Set up all on {}", m.name),
                    steps,
                    AdminNeed::None,
                ))
            }
            PlanKind::Running(on) => {
                let step = self.find_step(&m, &StepId::RUNNING)?;
                let StepFix::Toggle { needs_admin } = step.fix else {
                    return Err(format!("{} has no Running switch", m.name));
                };
                let title = if *on {
                    "Turn Running on"
                } else {
                    "Turn Running off"
                };
                let a = ApprovedStep {
                    id: StepId::RUNNING,
                    title: title.into(),
                    display: (self.catalog.toggle_script)(m.is_linux(), *on),
                    waiting: false,
                    needs_admin,
                };
                Ok(shown(
                    &m,
                    title.into(),
                    vec![a],
                    Self::admin_need(&m, needs_admin),
                ))
            }
            PlanKind::SendAgain(usage) => {
                if usage.machine != m.name
                    || !(self.catalog.usages)(&self.current_machines()).contains(usage)
                {
                    return Err(format!(
                        "{} at {} is not a secret file of {}",
                        usage.name, usage.remote_path, m.name
                    ));
                }
                let a = self.delivery_step(usage, format!("Send {} again", usage.name))?;
                Ok(shown(&m, a.title.clone(), vec![a], AdminNeed::None))
            }
            PlanKind::Update(_) => Err("unreachable".into()),
        }
    }

    /// The delivery of `usage`, shown as its step's own Fix commands.
    fn delivery_step(&self, usage: &SecretUsage, title: String) -> Result<ApprovedStep, String> {
        let m = self
            .machine(&usage.machine)
            .ok_or(format!("no machine named {}", usage.machine))?;
        let id = Self::delivery_id(usage, &m);
        let display = self
            .find_step(&m, &id)
            .ok()
            .and_then(|s| s.plan().map(|p| p.display.clone()))
            .unwrap_or_else(|| {
                format!(
                    "# {} goes to {}:{} over ssh's stdin; it is never shown",
                    usage.name, usage.machine, usage.remote_path
                )
            });
        Ok(ApprovedStep {
            id,
            title,
            display,
            waiting: false,
            needs_admin: false,
        })
    }

    fn delivery_id(usage: &SecretUsage, m: &Machine) -> StepId {
        let issues = m.sd_worker().is_some_and(|j| j.issues_secret == usage.name)
            || (usage.name != m.claude_secret && usage.remote_path == shell::GITHUB_TOKEN);
        if issues {
            StepId::ISSUES_TOKEN
        } else {
            StepId::CLAUDE_LOGIN
        }
    }
}
