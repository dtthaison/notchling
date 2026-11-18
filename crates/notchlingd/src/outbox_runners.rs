use crate::agy_ops::AgyOps;
use crate::codex_ops::CodexOps;
use crate::outbox_courier::{Courier, NOTHING};
use crate::outbox_ops::OutboxOps;
use notchling_engine::contract::channel::Ability;
use notchling_engine::outbox::{Facts, Job, Row, Tried, TryEnd, TURN_SETTLE_MS};
use std::sync::Arc;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Delivery {
    Busy,
    Failed(String),
    Cut,
}

const STARTED: Tried = Tried {
    pasted: true,
    entered: true,
};

fn runner_facts(now_ms: i64, busy: bool) -> Facts {
    Facts {
        now_ms,
        job: Job::Live { status: None },
        dialog_on_screen: false,
        queue_waiting: 0,
        safe: busy,
        turn_over_since_ms: (!busy).then_some(now_ms - TURN_SETTLE_MS),
    }
}

fn ended(delivered: Result<(), Delivery>) -> (TryEnd, Tried, Option<String>) {
    match delivered {
        Ok(()) => (TryEnd::Ok, STARTED, None),
        Err(Delivery::Busy) => (TryEnd::NotIdle, NOTHING, None),
        Err(Delivery::Cut) => (TryEnd::Cut, STARTED, None),
        Err(Delivery::Failed(why)) => (TryEnd::AttachFailed, NOTHING, Some(why)),
    }
}

fn bare<'a>(row: &'a Row, prefix: &str) -> &'a str {
    row.session.strip_prefix(prefix).unwrap_or(&row.session)
}

pub struct CodexCourier {
    ops: Arc<CodexOps>,
}

impl CodexCourier {
    pub fn new(ops: Arc<CodexOps>) -> CodexCourier {
        CodexCourier { ops }
    }
}

impl Courier for CodexCourier {
    fn facts(&self, row: &Row, now_ms: i64) -> Facts {
        runner_facts(now_ms, self.ops.is_busy(bare(row, "codex:")))
    }

    fn deliver(&self, row: &Row, _guarded: bool) -> (TryEnd, Tried, Option<String>) {
        ended(
            self.ops
                .start_turn(bare(row, "codex:"), &row.text, row.options.as_ref()),
        )
    }

    fn close(&self, _session: &str) {}

    fn verifies(&self) -> bool {
        false
    }
}

pub struct AgyCourier {
    ops: Arc<AgyOps>,
}

impl AgyCourier {
    pub fn new(ops: Arc<AgyOps>) -> AgyCourier {
        AgyCourier { ops }
    }
}

impl Courier for AgyCourier {
    fn facts(&self, row: &Row, now_ms: i64) -> Facts {
        runner_facts(now_ms, self.ops.is_busy(bare(row, "agy:")))
    }

    fn deliver(&self, row: &Row, _guarded: bool) -> (TryEnd, Tried, Option<String>) {
        let attempt = format!("{}-{}", row.id, row.tries);
        ended(self.ops.deliver(&attempt, bare(row, "agy:"), &row.text))
    }

    fn close(&self, _session: &str) {}

    fn verifies(&self) -> bool {
        false
    }
}

pub fn link_codex(outbox: &Arc<OutboxOps>, codex: &Arc<CodexOps>) {
    codex.attach_outbox(outbox.clone());
    let weak = Arc::downgrade(codex);
    outbox.set_codex_check(Arc::new(move |session| {
        weak.upgrade()
            .map_or(Ok(()), |codex| codex.check_session(session))
    }));
}

pub fn link_agy(outbox: &Arc<OutboxOps>, agy: &Arc<AgyOps>) {
    agy.attach_outbox(outbox.clone());
    let weak = Arc::downgrade(agy);
    outbox.set_agy_check(Arc::new(move |session| {
        weak.upgrade()
            .map_or(Ok(()), |agy| agy.check_session(session))
    }));
    let weak = Arc::downgrade(agy);
    outbox.set_agy_gate(Arc::new(move |phone| {
        weak.upgrade()
            .map_or(Ok(()), |agy| agy.allow(Ability::Reply, phone))
    }));
}

#[cfg(test)]
#[path = "outbox_runners_tests.rs"]
mod tests;
