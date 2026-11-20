use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use notchling_engine::machines::registry::Machine;

use super::add_sheet::Draft;
use super::edit_key::{EditResult, EditTest};

pub struct TestRun {
    state: EditTest,
    tested: String,
    stop: Arc<AtomicBool>,
}

impl Default for TestRun {
    fn default() -> TestRun {
        TestRun {
            state: EditTest::Idle,
            tested: String::new(),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Drop for TestRun {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl TestRun {
    pub fn is_running(&self) -> bool {
        matches!(self.state, EditTest::Running(_))
    }

    pub fn is_stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    pub fn reset(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.stop = Arc::new(AtomicBool::new(false));
        self.state = EditTest::Idle;
    }

    pub fn start(&mut self, signature: String, round: u64) -> Arc<AtomicBool> {
        self.reset();
        self.tested = signature;
        self.state = EditTest::Running(round);
        self.stop.clone()
    }

    pub fn finish(&mut self, round: u64, now: &str, results: Vec<EditResult>) {
        if self.state == EditTest::Running(round) {
            self.state = if now == self.tested {
                EditTest::Done(round, results)
            } else {
                EditTest::Idle
            };
        }
    }

    pub fn shown(&self, now: &str) -> &[EditResult] {
        match &self.state {
            EditTest::Done(_, results) if now == self.tested => results,
            _ => &[],
        }
    }
}

pub fn may_test(has_target: bool, running: bool, lanes: usize, limit: usize) -> bool {
    has_target && !running && lanes < limit
}

pub fn test_signature(draft: &Draft, machines: &[Machine], jump: Option<&str>) -> String {
    let mut draft = draft.clone();
    draft.name.clear();
    let mut through: Vec<&str> = jump.into_iter().collect();
    through.extend(draft.first.via.as_deref());
    through.extend(draft.more.iter().filter_map(|r| r.meta.via.as_deref()));
    let hops: Vec<String> = through
        .iter()
        .filter_map(|name| machines.iter().find(|m| m.name == *name))
        .map(|m| format!("{}|{:?}|{:?}", m.user, m.address_list(), m.ssh_key()))
        .collect();
    format!("{draft:?}|{hops:?}")
}
