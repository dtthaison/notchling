use super::backend::{Audit, AuditEntry};
use crate::bridge::net::jwt::Clock;
use notchling_events::envelope::{Actor, ActorTier, Via};
use notchling_events::kinds::choices::ItemAction;
use notchling_events::kinds::phone::{
    PairAction, PhoneActed, PhoneAnswerResult, PhoneAnswered, PhoneOp, PhonePaired,
};
use notchling_events::scope::{self, Scope};
use notchling_events::spec::Outcome;
use notchling_events::{Count, Device, Emit, Event, Ref};
use std::sync::Arc;

pub struct EventsAudit {
    emit: Arc<dyn Emit>,
    now: Clock,
}

impl EventsAudit {
    pub fn new(emit: Arc<dyn Emit>, now: Clock) -> EventsAudit {
        EventsAudit { emit, now }
    }
}

fn device_of(entry: &AuditEntry) -> Ref<Device> {
    let id = if entry.device_id.is_empty() {
        &entry.device
    } else {
        &entry.device_id
    };
    Ref::<Device>::new(id)
}

fn op_of(op: &str) -> PhoneOp {
    match op {
        "message" => PhoneOp::Message,
        "sendNow" => PhoneOp::SendNow,
        "answer" => PhoneOp::Answer,
        "newSession" => PhoneOp::NewSession,
        "image" => PhoneOp::Image,
        _ => PhoneOp::Other,
    }
}

impl Audit for EventsAudit {
    fn append(&self, entry: AuditEntry) {
        let device = device_of(&entry);
        let ts = (self.now)();
        let op = entry.op.as_str();
        let event = match op.split_once(':') {
            None if op == "pair" => Event::new(
                &PhonePaired {
                    device: device.clone(),
                    action: PairAction::Paired,
                    via: None,
                },
                Outcome::Ok,
                ts,
            ),
            None if op == "revoke" => Event::new(
                &PhonePaired {
                    device: device.clone(),
                    action: PairAction::Revoked,
                    via: None,
                },
                Outcome::Ok,
                ts,
            ),
            Some(("inbox.answer", rest)) => match rest.strip_prefix("refused:") {
                Some(reason) => Event::new(
                    &PhoneAnswered {
                        device: device.clone(),
                        item_kind: None,
                        action: None,
                        result: PhoneAnswerResult::from_name(reason)
                            .unwrap_or(PhoneAnswerResult::Other),
                    },
                    Outcome::Refused,
                    ts,
                ),
                None => Event::new(
                    &PhoneAnswered {
                        device: device.clone(),
                        item_kind: None,
                        action: Some(ItemAction::from_name(rest).unwrap_or(ItemAction::Other)),
                        result: PhoneAnswerResult::Taken,
                    },
                    Outcome::Ok,
                    ts,
                ),
            },
            _ => Event::new(
                &PhoneActed {
                    device: device.clone(),
                    op: op_of(op),
                    session: entry
                        .session
                        .as_deref()
                        .map(Ref::<notchling_events::Session>::new),
                    chars: entry.length.map(|n| Count(n as u64)),
                },
                Outcome::Ok,
                ts,
            ),
        };
        let scope = Scope {
            op_id: scope::current().op_id,
            actor: Some(Actor {
                tier: ActorTier::Phone,
                via: Some(Via::Bridge),
                exe: None,
                device: Some(device),
                session: None,
            }),
        };
        scope.run(|| self.emit.push(event));
    }
}

#[cfg(test)]
#[path = "events_audit_tests.rs"]
mod tests;
