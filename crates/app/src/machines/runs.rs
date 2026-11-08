//! A Set up all's admin ask and a confirmed plan on its way to run, shared between the window
//! and the background thread that runs it.

use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

use notchling_engine::machines::engine::SetupEngine;
use notchling_engine::machines::engine_plan::{AdminAnswer, ApprovedStep, Confirmed, PlanKind};
use notchling_engine::machines::step::FixOutcome;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// An admin step Set up all reached: its run waits on the window's answer.
pub struct PendingAdmin {
    pub machine: String,
    pub step: ApprovedStep,
    reply: Sender<AdminAnswer>,
}

impl PendingAdmin {
    pub fn answer(self, answer: AdminAnswer) {
        let _ = self.reply.send(answer);
    }
}

/// Where a Set up all run leaves its admin ask for the window (`ask`, on the run's thread), and
/// where the window picks it up (`take`, on each tick).
#[derive(Clone, Default)]
pub struct AdminAsks(Arc<Mutex<Option<PendingAdmin>>>);

impl AdminAsks {
    /// Blocks until the window answers; a window gone before that cancels.
    pub fn ask(&self, machine: &str, step: &ApprovedStep) -> AdminAnswer {
        let (tx, rx) = channel();
        *lock(&self.0) = Some(PendingAdmin {
            machine: machine.to_owned(),
            step: step.clone(),
            reply: tx,
        });
        rx.recv().unwrap_or(AdminAnswer::None)
    }

    pub fn take(&self) -> Option<PendingAdmin> {
        lock(&self.0).take()
    }

    pub(super) fn put_back(&self, pending: PendingAdmin) {
        *lock(&self.0) = Some(pending);
    }
}

/// A confirmed plan, to run off the UI thread.
pub struct Run {
    pub machine: String,
    pub kind: PlanKind,
    confirmed: Confirmed,
    asks: AdminAsks,
}

/// What a run was, for recording its outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunDone {
    pub machine: String,
    pub kind: PlanKind,
}

impl Run {
    pub(super) fn new(
        machine: String,
        kind: PlanKind,
        confirmed: Confirmed,
        asks: AdminAsks,
    ) -> Run {
        Run {
            machine,
            kind,
            confirmed,
            asks,
        }
    }

    /// Runs the plan (blocking). Set up all's admin steps ask through the window.
    pub fn execute(self, engine: &SetupEngine) -> (RunDone, FixOutcome) {
        let Run {
            machine,
            kind,
            confirmed,
            asks,
        } = self;
        let name = machine.clone();
        let outcome = engine.run_with(confirmed, &|step| asks.ask(&name, step));
        (RunDone { machine, kind }, outcome)
    }
}
