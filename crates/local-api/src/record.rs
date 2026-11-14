use crate::server::Shared;
use crate::wire::{ops, ErrorCode, Tier};
use notchling_events::kinds::choices::{CallerTier, FailWhy, SwitchName};
use notchling_events::kinds::diagnostics::{CallResult, OpCalled, OpFailed};
use notchling_events::kinds::security::{ApiRefused, ApiUnknownPeer, PeerWhy, RefusedWhy};
use notchling_events::spec::Outcome;
use notchling_events::{Event, Exe, Kind, Millis, Op};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

pub(crate) fn caller_tier(tier: Tier) -> CallerTier {
    match tier {
        Tier::App => CallerTier::App,
        Tier::Agent => CallerTier::Agent,
    }
}

fn exe_of(peer: Option<&Path>) -> Option<Exe> {
    peer.map(Exe::from_path)
}

fn millis(elapsed: Duration) -> Millis {
    Millis(u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
}

impl Shared {
    pub(crate) fn op_name(&self, op: &str) -> Op {
        let builtin = [ops::HELLO, ops::DAEMON_STATUS, ops::EVENTS_SUBSCRIBE];
        match builtin.iter().find(|name| **name == op) {
            Some(name) => Op::named(name),
            None => Op::parse(op, self.handler.known_ops()),
        }
    }

    pub(crate) fn push_event<K: Kind>(&self, kind: &K, outcome: Outcome) {
        if self.events.is_some() {
            self.emit(Event::new(kind, outcome, now_ms()));
        }
    }

    pub(crate) fn refused(
        &self,
        op: &str,
        tier: Option<Tier>,
        peer: Option<&Path>,
        why: RefusedWhy,
        switch: Option<SwitchName>,
    ) {
        self.push_event(
            &ApiRefused {
                op: self.op_name(op),
                exe: exe_of(peer),
                tier: caller_tier(tier.unwrap_or(Tier::Agent)),
                why,
                switch,
            },
            Outcome::Refused,
        );
    }

    pub(crate) fn unknown_peer(&self, why: PeerWhy, peer: Option<&Path>) {
        self.push_event(
            &ApiUnknownPeer {
                why,
                exe: exe_of(peer),
            },
            Outcome::Refused,
        );
    }

    pub(crate) fn failed(&self, op: &str, code: ErrorCode, started: Instant) {
        self.push_event(
            &OpFailed {
                op: self.op_name(op),
                code: notchling_events::ErrorCode::parse(code.as_str()),
                why: FailWhy::Other,
                duration_ms: millis(started.elapsed()),
            },
            Outcome::Failed,
        );
    }

    pub(crate) fn called(&self, op: &str, tier: Tier, started: Instant) {
        self.push_event(
            &OpCalled {
                op: self.op_name(op),
                tier: caller_tier(tier),
                result: CallResult::Ok,
                code: None,
                ms: millis(started.elapsed()),
            },
            Outcome::Ok,
        );
    }

    pub(crate) fn detach_dropped_apps(&self) {
        for gone in self.presence.due(Instant::now()) {
            self.emit(Event::new(&gone, Outcome::Info, now_ms()));
        }
    }
}
