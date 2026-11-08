//! Running a Fix (Swift `SetupEngine`, the Fix half): the refusals, the Fix in flight with its
//! masked log, Set up all, the Running switch. Reached only from a confirmed plan
//! ([`super::engine_plan`]).

use super::engine::{lock, SetupEngine};
use super::engine_plan::{AdminAnswer, ApprovedStep};
use super::masks::{FixLog, Masks};
use super::registry::{AdminAccess, Machine};
use super::runner::{Channel, OutputSink};
use super::shell;
use super::step::{run, FixContext, FixOutcome, SetupStep, StepFix, StepId};
use crate::accounts::Secret;
use std::sync::Arc;

fn fail(m: impl Into<String>) -> FixOutcome {
    FixOutcome::fail(m)
}

impl SetupEngine {
    // MARK: running a Fix

    fn append_log(&self, machine: &str, lines: Vec<String>) {
        append(&self.state, machine, lines);
    }

    /// Why a Fix can't run now, or `None`. Nothing has run when this answers.
    pub(super) fn refusal(
        &self,
        step: &SetupStep,
        m: &Machine,
        admin: &AdminAnswer,
        needs_admin: bool,
        check_needs: bool,
        turning_on: bool,
    ) -> Option<String> {
        if self.running(&m.name).is_some() {
            return Some(format!("another Fix is running on {}", m.name));
        }
        if turning_on {
            if let Some(blocker) = self.turn_on_blocker(&m.name) {
                return Some(format!("waits for {blocker}"));
            }
        }
        if check_needs {
            let results = self.results(&m.name);
            let list = self.step_list(m);
            for need in &step.needs {
                if results.get(need).is_some_and(|r| r.state.is_pass()) {
                    continue;
                }
                let title = list
                    .iter()
                    .find(|s| &s.id == need)
                    .map_or(need.raw().to_string(), |s| s.title.clone());
                return Some(format!("waits for {title}"));
            }
        }
        if needs_admin {
            if let Some(why) = shell::admin_refusal(m) {
                return Some(why.into());
            }
            match m.admin() {
                AdminAccess::RootSsh if !matches!(admin, AdminAnswer::RootConfirmed) => {
                    return Some(format!(
                        "needs your confirmation to run as root on {}",
                        m.name
                    ))
                }
                AdminAccess::Sudo if admin.password().is_none() => {
                    return Some("needs the admin password".into())
                }
                _ => {}
            }
        }
        None
    }

    pub(super) fn run_fix(
        &self,
        machine: &str,
        id: &StepId,
        admin: &AdminAnswer,
        expected: &str,
    ) -> FixOutcome {
        let Some(m) = self.machine(machine) else {
            return fail(format!("no machine named {machine}"));
        };
        let step = match self.find_step(&m, id) {
            Ok(s) => s,
            Err(e) => return fail(e),
        };
        let Some(plan) = step.plan().cloned() else {
            return fail(format!("{} has no Fix", step.title));
        };
        if plan.display != expected {
            return fail(format!("{} needs a look; Set up again", step.title));
        }
        if let Some(why) = self.refusal(&step, &m, admin, plan.needs_admin, true, false) {
            return fail(why);
        }
        // only an admin step on a sudo machine gets the password, and only for this one step
        let password = (plan.needs_admin && m.admin() == AdminAccess::Sudo)
            .then(|| admin.password().cloned())
            .flatten();
        let outcome = self.perform(id.clone(), &step.title, &m, password, |ctx| (plan.run)(ctx));
        self.refresh(Some(&[machine.to_string()]), true);
        outcome
    }

    pub(super) fn run_replace(&self, machine: &str, id: &StepId, expected: &str) -> FixOutcome {
        let Some(m) = self.machine(machine) else {
            return fail(format!("no machine named {machine}"));
        };
        let step = match self.find_step(&m, id) {
            Ok(s) => s,
            Err(e) => return fail(e),
        };
        let Some(plan) = step.replace.clone() else {
            return fail(format!("{machine} has nothing to replace in {}", id.raw()));
        };
        if !(self.catalog.offers_replace)(self.results(machine).get(id)) || plan.display != expected
        {
            return fail(format!(
                "{} needs a look; Check again",
                step.replace_title()
            ));
        }
        if let Some(why) = self.refusal(&step, &m, &AdminAnswer::None, false, true, false) {
            return fail(why);
        }
        let outcome = self.perform(id.clone(), &step.replace_title(), &m, None, |ctx| {
            (plan.run)(ctx)
        });
        self.refresh(Some(&[machine.to_string()]), true);
        outcome
    }

    /// Runs `body` as the Fix in flight on `m`: logs `▸ <title>` and the outcome, streams its masked
    /// output into the log.
    pub(super) fn perform(
        &self,
        id: StepId,
        title: &str,
        m: &Machine,
        password: Option<Secret>,
        body: impl FnOnce(&FixContext) -> FixOutcome,
    ) -> FixOutcome {
        lock(&self.state).running.insert(m.name.clone(), id);
        let masks: Masks = self.masks(m, password.as_ref().map(Secret::expose));
        self.append_log(&m.name, vec![format!("▸ {title}")]);
        self.changed();
        let log = Arc::new(FixLog::new(masks.clone()));
        let state = self.state.clone();
        let name = m.name.clone();
        let sink_log = log.clone();
        let sink: Arc<OutputSink> = Arc::new(move |c: Channel, t: &str| {
            sink_log.add(c, t);
            append(&state, &name, sink_log.take());
        });
        let ctx = FixContext {
            step: self.context(m, &masks),
            admin_password: password,
            log: sink,
            masks: masks.clone(),
        };
        let raw = body(&ctx);
        drop(ctx);
        log.end();
        self.append_log(&m.name, log.take());
        let message = masks.apply(&raw.message);
        let mut lines: Vec<String> = message.split('\n').map(str::to_string).collect();
        if let Some(first) = lines.first_mut() {
            *first = format!("{} {first}", if raw.ok { "✓" } else { "✗" });
        }
        self.append_log(&m.name, lines);
        {
            let mut s = lock(&self.state);
            s.running.remove(&m.name);
            *s.fixes_ended.entry(m.name.clone()).or_default() += 1;
        }
        self.changed();
        FixOutcome {
            ok: raw.ok,
            message,
        }
    }

    // MARK: Set up all

    pub(super) fn set_up_all_steps(&self, m: &Machine) -> Vec<ApprovedStep> {
        let results = self.results(&m.name);
        self.step_list(m)
            .into_iter()
            .filter_map(|step| {
                let plan = step.plan()?;
                let state = &results.get(&step.id)?.state;
                let waiting = if state.is_fail() {
                    false
                } else if state.is_waiting() {
                    true
                } else {
                    return None;
                };
                Some(ApprovedStep {
                    id: step.id.clone(),
                    title: step.title.clone(),
                    display: plan.display.clone(),
                    waiting,
                    needs_admin: plan.needs_admin,
                })
            })
            .collect()
    }

    /// Runs the approved steps that are ✗ when their turn comes, in catalog order, and stops at the
    /// first that fails. A step outside `approved` that is ✗ at its turn, or one whose commands
    /// differ from the ones shown, stops the run.
    pub(super) fn set_up_all(
        &self,
        machine: &str,
        approved: &[ApprovedStep],
        ask_admin: &dyn Fn(&ApprovedStep) -> AdminAnswer,
    ) -> FixOutcome {
        let Some(first) = self.machine(machine) else {
            return fail(format!("no machine named {machine}"));
        };
        if self.running(machine).is_some() {
            return fail(format!("another Fix is running on {machine}"));
        }
        let mut fixed = 0;
        for id in self.step_list(&first).into_iter().map(|s| s.id) {
            let Some(m) = self.machine(machine) else {
                break;
            };
            let Ok(step) = self.find_step(&m, &id) else {
                continue;
            };
            let Some(plan) = step.plan().cloned() else {
                continue;
            };
            if !self
                .results(machine)
                .get(&id)
                .is_some_and(|r| r.state.is_fail())
            {
                continue;
            }
            let Some(shown) = approved
                .iter()
                .find(|a| a.id == id && a.display == plan.display)
            else {
                return fail(format!("{} needs a look; Set up again", step.title));
            };
            let mut answer = AdminAnswer::None;
            if plan.needs_admin {
                if let Some(why) = shell::admin_refusal(&m) {
                    return fail(format!("{}: {why}", step.title));
                }
                answer = ask_admin(shown);
                if matches!(answer, AdminAnswer::None) {
                    return fail(format!(
                        "Set up all stopped before {}: cancelled",
                        step.title
                    ));
                }
            }
            let outcome = self.run_fix(machine, &id, &answer, &plan.display);
            drop(answer);
            if !outcome.ok {
                return fail(format!("{}: {}", step.title, outcome.message));
            }
            fixed += 1;
        }
        let done = match fixed {
            0 => "nothing to set up".to_string(),
            1 => "1 step set up".to_string(),
            n => format!("{n} steps set up"),
        };
        let Some(last) = self.machine(machine) else {
            return FixOutcome::ok(done);
        };
        let results = self.results(machine);
        let remaining: Vec<String> = self
            .step_list(&last)
            .into_iter()
            .filter(|s| {
                s.id != StepId::RUNNING && !results.get(&s.id).is_some_and(|r| r.state.is_pass())
            })
            .map(|s| s.title)
            .collect();
        if remaining.is_empty() {
            return FixOutcome::ok(done);
        }
        let mut shown = remaining
            .iter()
            .take(4)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if remaining.len() > 4 {
            shown.push_str(&format!(" and {} more", remaining.len() - 4));
        }
        fail(format!("{done}; still not ✓: {shown}"))
    }

    // MARK: the Running switch

    /// The first step before Running that isn't ✓, or `None`. Turning the worker on waits for it;
    /// turning it off never does.
    pub fn turn_on_blocker(&self, machine: &str) -> Option<String> {
        let m = self.machine(machine)?;
        let results = self.results(machine);
        for step in self.step_list(&m) {
            if step.id == StepId::RUNNING {
                return None;
            }
            if !results.get(&step.id).is_some_and(|r| r.state.is_pass()) {
                return Some(step.title);
            }
        }
        None
    }

    pub(super) fn toggle_running(
        &self,
        machine: &str,
        on: bool,
        admin: &AdminAnswer,
    ) -> FixOutcome {
        let Some(m) = self.machine(machine) else {
            return fail(format!("no machine named {machine}"));
        };
        let Ok(step) = self.find_step(&m, &StepId::RUNNING) else {
            return fail(format!("{machine} has no Running switch"));
        };
        let StepFix::Toggle { needs_admin } = step.fix else {
            return fail(format!("{machine} has no Running switch"));
        };
        if let Some(why) = self.refusal(&step, &m, admin, needs_admin, false, on) {
            return fail(why);
        }
        let script = (self.catalog.toggle_script)(m.is_linux(), on);
        let password = (needs_admin && m.admin() == AdminAccess::Sudo)
            .then(|| admin.password().cloned())
            .flatten();
        let title = if on {
            "Turn Running on"
        } else {
            "Turn Running off"
        };
        let outcome = self.perform(StepId::RUNNING, title, &m, password, |ctx| {
            let r = if needs_admin {
                run::admin(ctx, &script, 900)
            } else {
                run::script(ctx, &script, None, 600)
            };
            if r.code == 0 {
                return FixOutcome::ok(if on {
                    "Running is on"
                } else {
                    "Running is off"
                });
            }
            let text = if r.err.is_empty() { &r.out } else { &r.err }
                .trim()
                .to_string();
            fail(if text.is_empty() {
                format!("exited {}", r.code)
            } else {
                ctx.mask(&text)
            })
        });
        self.refresh(Some(&[machine.to_string()]), false);
        outcome
    }

    /// A machine was removed: drops everything held for it, so one added later under the same
    /// name starts clean. The counters stay.
    pub fn forget(&self, machine: &str) {
        {
            let mut s = lock(&self.state);
            s.results.remove(machine);
            s.logs.remove(machine);
            s.last_slow.remove(machine);
            s.facts.remove(machine);
            s.saved_state.remove(machine);
            s.expiry_sent.remove(machine);
            s.shown.retain(|_, r| r.plan.machine != machine);
            for usages in s.offers.values_mut() {
                usages.retain(|u| u.machine != machine);
            }
            s.offers.retain(|_, u| !u.is_empty());
        }
        self.save_offers();
        self.changed();
    }
}

pub(super) fn append(
    state: &std::sync::Mutex<super::engine::State>,
    machine: &str,
    lines: Vec<String>,
) {
    if lines.is_empty() {
        return;
    }
    let mut s = lock(state);
    let log = s.logs.entry(machine.to_string()).or_default();
    log.extend(lines);
    if log.len() > super::engine::LOG_LIMIT {
        let cut = log.len() - super::engine::LOG_LIMIT;
        log.drain(..cut);
    }
    s.revision += 1;
}
