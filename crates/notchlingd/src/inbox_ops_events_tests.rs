use crate::event_assert::{of, text};
use crate::inbox_ops_tests::{answer, item, rig, Rig, NOW};
use crate::late_emit::Events;
use crate::op_group::OpGroup;
use notchling_engine::inbox::{ActionId, State};
use notchling_events::envelope::{Actor, ActorTier};
use notchling_events::field::FieldValue;
use notchling_events::scope::Scope;
use notchling_events::{Device, MemorySink, Ref, Salt};
use std::sync::atomic::Ordering;
use std::sync::Arc;

fn watched() -> (Rig, Arc<MemorySink>) {
    let r = rig();
    let sink = Arc::new(MemorySink::new());
    let ops = r
        .ops
        .clone()
        .with_events(Events::fixed(sink.clone(), Salt([1; 32])));
    (Rig { ops, ..r }, sink)
}

fn phone(device: &str) -> Scope {
    Scope {
        op_id: None,
        actor: Some(Actor {
            tier: ActorTier::Phone,
            via: None,
            exe: None,
            device: Some(Ref::<Device>::new(device)),
            session: None,
        }),
    }
}

#[test]
fn raise_and_answer_push_raised_and_closed() {
    let (r, sink) = watched();
    let mut a = item("a");
    a.source.session = Some("sess-1".into());
    r.ops.raise(a, None);
    r.clock.store(NOW + 4_000, Ordering::SeqCst);
    answer(&r.ops, "a", "deny");
    assert_eq!(sink.names(), ["inbox.raised", "inbox.closed"]);
    let raised = &of(&sink, "inbox.raised")[0];
    assert_eq!(
        text(raised, "item_kind").as_deref(),
        Some("secret_approval")
    );
    assert_eq!(text(raised, "session").as_deref(), Some("sess-1"));
    let closed = &of(&sink, "inbox.closed")[0];
    assert_eq!(text(closed, "state").as_deref(), Some("answered"));
    assert_eq!(text(closed, "action").as_deref(), Some("deny"));
    assert_eq!(text(closed, "by").as_deref(), Some("mac"));
    assert_eq!(text(closed, "waited_ms").as_deref(), Some("4000"));
}

#[test]
fn phone_answer_has_device_from_scope() {
    let (r, sink) = watched();
    r.ops.raise(item("a"), None);
    phone("d1").run(|| r.ops.decide("a", ActionId::Deny, "phone").unwrap());
    let closed = &of(&sink, "inbox.closed")[0];
    assert_eq!(text(closed, "by").as_deref(), Some("phone"));
    assert_eq!(text(closed, "device").as_deref(), Some("d1"));
}

#[test]
fn expiry_pushes_closed_expired() {
    let (r, sink) = watched();
    let mut a = item("a");
    a.expires_ms = Some(NOW + 1_000);
    r.ops.raise(a, None);
    r.ops.tick(NOW + 2_000);
    let closed = &of(&sink, "inbox.closed")[0];
    assert_eq!(text(closed, "state").as_deref(), Some("expired"));
    assert!(text(closed, "action").is_none());
    assert!(text(closed, "device").is_none());
}

#[test]
fn resolve_withdrawn_and_seen_marks() {
    let (r, sink) = watched();
    r.ops.raise(item("a"), None);
    r.ops.resolve("a", State::Withdrawn, None, None);
    r.ops.resolve("a", State::Withdrawn, None, None);
    r.ops
        .call(
            &crate::inbox_ops_tests::app(),
            notchling_local_api::ops::INBOX_SEEN,
            &serde_json::json!({"ids": ["a"]}),
        )
        .unwrap();
    let closed = of(&sink, "inbox.closed");
    assert_eq!(
        closed.len(),
        1,
        "closing twice, or marking seen, is not another close"
    );
    assert_eq!(text(&closed[0], "state").as_deref(), Some("withdrawn"));
}

#[test]
fn item_id_is_hashed_in_activity() {
    let (r, sink) = watched();
    r.ops.raise(item("vault-approval:secret-ticket"), None);
    let raised = &of(&sink, "inbox.raised")[0];
    assert!(matches!(
        raised.fields.get("item"),
        Some(FieldValue::Ref {
            always_hashed: true,
            ..
        })
    ));
}
