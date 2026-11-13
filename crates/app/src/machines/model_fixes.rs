use notchling_engine::machines::engine_plan::{AdminAnswer, AdminNeed, PlanKind};
use notchling_engine::machines::step::{FixOutcome, StepId};
use notchling_engine::machines::usage::SecretUsage;

use super::model::{MachinesModel, Sheet};
use super::pieces::{FixLine, FixPrompt};
use super::runs::{Run, RunDone};

impl MachinesModel {
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

    pub fn request_set_up(&mut self, id: &StepId, machine: &str) {
        self.show_plan(machine, PlanKind::Fix(id.clone()));
    }

    pub fn request_replace_key(&mut self, id: &StepId, machine: &str) {
        self.show_plan(machine, PlanKind::Replace(id.clone()));
    }

    pub fn request_set_up_all(&mut self, machine: &str) {
        self.show_plan(machine, PlanKind::SetUpAll);
    }

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

    pub fn request_send_again(&mut self, usage: &SecretUsage) {
        self.show_plan(&usage.machine.clone(), PlanKind::SendAgain(usage.clone()));
    }

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

    pub fn check_after_poll(&mut self) -> bool {
        if self.machines.is_empty() || self.checking {
            return false;
        }
        self.checking = true;
        true
    }

    pub fn checked(&mut self) {
        self.checking = false;
    }

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
