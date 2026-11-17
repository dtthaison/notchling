use std::collections::BTreeSet;
use std::sync::{Mutex, MutexGuard};

use notchling_engine::machines::engine::SetupEngine;
use notchling_engine::machines::engine_plan::{AdminAnswer, PlanKind, ShownPlan};
use notchling_engine::machines::node_updates::{runnable, UpdateRow};
use notchling_engine::machines::step::FixOutcome;
use notchling_locale as t;

use super::add_sheet::AddPassword;
use super::updates::{plan, NotNow};
use crate::machines::runs::{AdminAsks, PendingAdmin};

#[derive(Default)]
struct Inner {
    hidden: NotNow,
    running: bool,
    outcome: Option<FixOutcome>,
    pending: Option<PendingAdmin>,
}

#[derive(Default)]
pub struct UpdateSession {
    inner: Mutex<Inner>,
    asks: AdminAsks,
}

impl UpdateSession {
    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn hide(&self, this_build: Option<&str>) {
        self.inner().hidden.hide(this_build);
    }

    pub fn hides(&self, this_build: Option<&str>) -> bool {
        self.inner().hidden.hides(this_build)
    }

    pub fn running(&self) -> bool {
        self.inner().running
    }

    pub fn outcome(&self) -> Option<FixOutcome> {
        self.inner().outcome.clone()
    }

    pub fn asks(&self) -> AdminAsks {
        self.asks.clone()
    }

    pub fn dismissed(&self) {
        let mut inner = self.inner();
        if !inner.running {
            inner.outcome = None;
        }
    }

    pub fn begin(&self) -> bool {
        let mut inner = self.inner();
        if inner.running {
            return false;
        }
        inner.running = true;
        inner.outcome = None;
        inner.pending = None;
        true
    }

    pub fn finish(&self, outcome: FixOutcome) {
        let mut inner = self.inner();
        inner.running = false;
        inner.pending = None;
        inner.outcome = Some(outcome);
    }

    pub fn pull(&self) -> (bool, bool) {
        let mut inner = self.inner();
        if !inner.running {
            return (false, false);
        }
        if inner.pending.is_none() {
            if let Some(pending) = self.asks.take() {
                inner.pending = Some(pending);
                return (true, true);
            }
        }
        (true, false)
    }

    pub fn pending_target(&self) -> Option<String> {
        self.inner().pending.as_ref().map(|p| p.step.title.clone())
    }

    pub fn pending_machine(&self) -> Option<String> {
        self.inner().pending.as_ref().map(|p| p.machine.clone())
    }

    pub fn answer_vault(&self, machine: &str, entry: &str, password: &mut AddPassword) -> bool {
        let mut inner = self.inner();
        if inner.pending.as_ref().is_none_or(|p| p.machine != machine) {
            return false;
        }
        let Some(pending) = inner.pending.take() else {
            return false;
        };
        password.clear();
        pending.answer(AdminAnswer::Vault(entry.to_owned()));
        true
    }

    pub fn answer(&self, go: bool, password: &mut AddPassword) -> bool {
        let mut inner = self.inner();
        if inner.pending.is_none() || (go && password.is_empty()) {
            return false;
        }
        let Some(pending) = inner.pending.take() else {
            return false;
        };
        let answer = match (go, password.take()) {
            (true, Some(secret)) => AdminAnswer::Password(secret),
            _ => AdminAnswer::None,
        };
        pending.answer(answer);
        true
    }
}

#[derive(Default)]
pub struct Offered {
    names: Option<Vec<String>>,
    shown: Option<ShownPlan>,
}

impl Offered {
    pub fn sync(
        &mut self,
        engine: &SetupEngine,
        rows: &[UpdateRow],
        anyway: &BTreeSet<String>,
    ) -> bool {
        let names = runnable(rows, &anyway.iter().cloned().collect::<Vec<_>>());
        if self.names.as_ref() == Some(&names) {
            return false;
        }
        self.cancel(engine);
        self.shown = plan(engine, rows, anyway).ok();
        self.names = Some(names);
        true
    }

    pub fn reset(&mut self, engine: Option<&SetupEngine>) {
        match engine {
            Some(engine) => self.cancel(engine),
            None => self.shown = None,
        }
        self.names = None;
    }

    pub fn cancel(&mut self, engine: &SetupEngine) {
        if let Some(old) = self.shown.take() {
            engine.cancel(old.id);
        }
    }

    pub fn take(&mut self) -> Option<ShownPlan> {
        self.shown.take()
    }

    pub fn shown(&self) -> Option<&ShownPlan> {
        self.shown.as_ref()
    }

    pub fn button(&self) -> (String, bool) {
        let count = match self.shown.as_ref().map(|p| &p.kind) {
            Some(PlanKind::UpdateNotchling(names)) => names.len(),
            _ => 0,
        };
        (t::cluster::update_button_run(count), self.shown.is_some())
    }
}
