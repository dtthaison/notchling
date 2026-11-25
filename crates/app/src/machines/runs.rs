use std::collections::VecDeque;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

use notchling_engine::machines::engine::SetupEngine;
use notchling_engine::machines::engine_plan::{AdminAnswer, ApprovedStep, Confirmed, PlanKind};
use notchling_engine::machines::step::FixOutcome;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

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

#[derive(Clone, Default)]
pub struct AdminAsks(Arc<Mutex<VecDeque<PendingAdmin>>>);

impl AdminAsks {
    pub fn ask(&self, machine: &str, step: &ApprovedStep) -> AdminAnswer {
        let (tx, rx) = channel();
        lock(&self.0).push_back(PendingAdmin {
            machine: machine.to_owned(),
            step: step.clone(),
            reply: tx,
        });
        rx.recv().unwrap_or(AdminAnswer::None)
    }

    pub fn take(&self) -> Option<PendingAdmin> {
        lock(&self.0).pop_front()
    }

    pub fn waiting(&self) -> usize {
        lock(&self.0).len()
    }

    pub(super) fn put_back(&self, pending: PendingAdmin) {
        lock(&self.0).push_front(pending);
    }
}

pub struct Run {
    pub machine: String,
    pub kind: PlanKind,
    confirmed: Confirmed,
    asks: AdminAsks,
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::accounts::Secret;
    use notchling_engine::machines::step::StepId;

    fn step() -> ApprovedStep {
        ApprovedStep {
            id: StepId::STAYS_AWAKE,
            title: "Stays awake".into(),
            display: "sudo pmset -a sleep 0".into(),
            waiting: false,
            needs_admin: true,
        }
    }

    fn wait_for(asks: &AdminAsks, n: usize) {
        for _ in 0..400 {
            if asks.waiting() >= n {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("{} asks waiting, not {n}", asks.waiting());
    }

    #[test]
    fn two_runs_asking_at_once_each_get_their_own_answer() {
        let asks = AdminAsks::default();
        let first = {
            let asks = asks.clone();
            std::thread::spawn(move || asks.ask("m1", &step()))
        };
        wait_for(&asks, 1);
        let second = {
            let asks = asks.clone();
            std::thread::spawn(move || asks.ask("m2", &step()))
        };
        wait_for(&asks, 2);

        let a = asks.take().expect("the first ask");
        assert_eq!(a.machine, "m1", "the oldest first");
        asks.put_back(a);
        let a = asks.take().unwrap();
        assert_eq!(a.machine, "m1");
        a.answer(AdminAnswer::Password(Secret::new("pw-1")));
        let b = asks.take().expect("the second ask is still there");
        assert_eq!(b.machine, "m2");
        b.answer(AdminAnswer::Password(Secret::new("pw-2")));
        assert!(asks.take().is_none());

        let one = first.join().unwrap();
        let two = second.join().unwrap();
        assert!(matches!(one, AdminAnswer::Password(p) if p.expose() == "pw-1"));
        assert!(matches!(two, AdminAnswer::Password(p) if p.expose() == "pw-2"));
    }
}
