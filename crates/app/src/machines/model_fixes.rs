//! The Fixes of the Machines window (Swift `MachinesModel`'s Fix half): every one starts in a
//! `request_*` a button calls, which only asks the engine for the plan and shows it in the Fix
//! sheet; `confirm` takes the one click on Run and hands back the [`Run`] to execute off the UI
//! thread; `tick` picks up the admin step a Set up all reached.

use notchling_engine::machines::engine_plan::{AdminAnswer, AdminNeed, PlanKind};
use notchling_engine::machines::step::{FixOutcome, StepId};
use notchling_engine::machines::usage::SecretUsage;

use super::model::{MachinesModel, Sheet};
use super::pieces::{FixLine, FixPrompt};
use super::runs::{Run, RunDone};

impl MachinesModel {
    // MARK: Fixes: every one starts here, from a button, and only shows its plan

    /// A request while a sheet is on show came from a click that reached a card under it (or a
    /// key): it is refused, so the Run on screen stays the plan it shows.
    fn busy(&self) -> bool {
        self.sheet.is_some()
    }

    fn show_plan(&mut self, machine: &str, kind: PlanKind) {
        if self.busy() {
            return;
        }
        match self.parts.engine.plan(machine, kind) {
            Ok(plan) => {
                let prompt = match self.machine(machine) {
                    Some(m) => FixPrompt::new(plan, &m),
                    None => FixPrompt {
                        plan,
                        host: String::new(),
                        user: String::new(),
                        asked: false,
                    },
                };
                self.set_sheet(Some(Sheet::Fix(prompt)));
            }
            Err(why) if why == "nothing to set up" => {
                self.outcomes.insert(machine.to_owned(), FixLine::note(why));
            }
            Err(why) => {
                self.outcomes
                    .insert(machine.to_owned(), FixLine::of(&FixOutcome::fail(why)));
            }
        }
    }

    /// Set up on one step: shows its commands. Runs nothing.
    pub fn request_set_up(&mut self, id: &StepId, machine: &str) {
        self.show_plan(machine, PlanKind::Fix(id.clone()));
    }

    /// Replace key on a Deploy key step: shows its exact commands. Runs nothing. The engine offers
    /// it only while the step's Check is ✓, or says GitHub has no record of the machine's key.
    pub fn request_replace_key(&mut self, id: &StepId, machine: &str) {
        self.show_plan(machine, PlanKind::Replace(id.clone()));
    }

    /// Set up all: shows the commands of every step it will run, the ones waiting for an earlier
    /// step too. The admin password is asked for per admin step, when its turn comes.
    pub fn request_set_up_all(&mut self, machine: &str) {
        self.show_plan(machine, PlanKind::SetUpAll);
    }

    /// The Running switch: shows its script. Runs nothing.
    pub fn request_toggle(&mut self, machine: &str, on: bool) {
        if self.busy() {
            return;
        }
        if on {
            if let Some(blocker) = self.parts.engine.turn_on_blocker(machine) {
                self.outcomes.insert(
                    machine.to_owned(),
                    FixLine::fail(format!("waits for {blocker}")),
                );
                return;
            }
        }
        self.show_plan(machine, PlanKind::Running(on));
    }

    /// Send again on a secret file, an OK one too: shows the delivery. Runs nothing.
    pub fn request_send_again(&mut self, usage: &SecretUsage) {
        self.show_plan(&usage.machine.clone(), PlanKind::SendAgain(usage.clone()));
    }

    /// Update N machines: shows each delivery. Runs nothing.
    pub fn request_update(&mut self, secret: &str) {
        if self.busy() {
            return;
        }
        match self
            .parts
            .engine
            .plan("", PlanKind::Update(secret.to_owned()))
        {
            Ok(plan) => self.set_sheet(Some(Sheet::Fix(FixPrompt {
                plan,
                host: String::new(),
                user: String::new(),
                asked: false,
            }))),
            Err(why) => {
                self.update_notes.insert(secret.to_owned(), why);
            }
        }
    }

    /// The sheet's Run. The typed password is for this one run: it goes into the answer and
    /// leaves the model. The [`Run`] is executed by the caller, off the UI thread.
    pub fn confirm(&mut self) -> Option<Run> {
        if !matches!(self.sheet, Some(Sheet::Fix(_))) {
            return None;
        }
        let password = self.password.take();
        let Some(Sheet::Fix(prompt)) = self.sheet.take() else {
            return None;
        };
        let answer = match &prompt.plan.admin {
            AdminNeed::RootConfirm => AdminAnswer::RootConfirmed,
            AdminNeed::Password => match password {
                Some(p) => AdminAnswer::Password(p),
                None => AdminAnswer::None,
            },
            AdminNeed::None | AdminNeed::Refused(_) => AdminAnswer::None,
        };
        if prompt.asked {
            // Set up all asked for this admin step: the answer goes back to its run
            if let Some(pending) = self.pending.take() {
                pending.answer(answer);
            }
            return None;
        }
        match self.parts.engine.confirm(prompt.plan.id, answer) {
            Ok(confirmed) => Some(Run::new(
                prompt.plan.machine.clone(),
                prompt.plan.kind.clone(),
                confirmed,
                self.asks.clone(),
            )),
            Err(why) => {
                self.finished(
                    &RunDone {
                        machine: prompt.plan.machine.clone(),
                        kind: prompt.plan.kind.clone(),
                    },
                    &FixOutcome::fail(why),
                );
                None
            }
        }
    }

    /// The sheet's Cancel (or Esc): nothing runs. A Set up all waiting on it stops.
    pub fn cancel_sheet(&mut self) {
        let closed = self.sheet.take();
        self.set_sheet(None);
        if let Some(Sheet::Fix(prompt)) = closed {
            if prompt.asked {
                if let Some(pending) = self.pending.take() {
                    pending.answer(AdminAnswer::None);
                }
            } else {
                self.parts.engine.cancel(prompt.plan.id);
            }
        }
    }

    /// A run ended: its line on the machine's card, or under the update banner.
    pub fn finished(&mut self, done: &RunDone, outcome: &FixOutcome) {
        match &done.kind {
            PlanKind::Update(secret) => {
                if outcome.ok {
                    self.update_notes.remove(secret);
                } else {
                    self.update_notes
                        .insert(secret.clone(), outcome.message.clone());
                }
            }
            _ => {
                self.outcomes
                    .insert(done.machine.clone(), FixLine::of(outcome));
            }
        }
    }

    // MARK: Checks while the window is open

    /// A poll ended: whether to check the machines again (checks only, the slow probes as their
    /// cadence allows), as Swift's window does on each poller change while it is visible. One
    /// round at a time; [`MachinesModel::checked`] ends it.
    pub fn check_after_poll(&mut self) -> bool {
        if self.machines.is_empty() || self.checking {
            return false;
        }
        self.checking = true;
        true
    }

    /// The round of checks a poll started ended.
    pub fn checked(&mut self) {
        self.checking = false;
    }

    /// Every tick: a Set up all that reached an admin step gets its sheet. Whether one was shown.
    pub fn tick(&mut self) -> bool {
        let Some(pending) = self.asks.take() else {
            return false;
        };
        if self.sheet.is_some() {
            self.asks.put_back(pending);
            return false;
        }
        match self.machine(&pending.machine) {
            Some(m) => {
                self.set_sheet(Some(Sheet::Fix(FixPrompt::asking(&m, &pending.step))));
                self.pending = Some(pending);
                true
            }
            None => {
                pending.answer(AdminAnswer::None);
                false
            }
        }
    }
}
