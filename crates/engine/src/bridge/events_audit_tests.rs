use super::*;
use notchling_events::emit::{Committed, Unwritable};
use notchling_events::envelope::ActorTier;
use notchling_events::field::FieldValue;
use std::sync::Mutex;

#[derive(Default)]
struct Catch {
    seen: Mutex<Vec<(Event, Scope)>>,
}

impl Emit for Catch {
    fn push(&self, event: Event) {
        self.seen.lock().unwrap().push((event, scope::current()));
    }

    fn commit(&self, event: Event) -> Result<Committed, Unwritable> {
        self.push(event);
        Ok(Committed {
            seq: 0,
            audit_n: None,
        })
    }
}

fn rig() -> (Arc<Catch>, EventsAudit) {
    let catch = Arc::new(Catch::default());
    let audit = EventsAudit::new(catch.clone(), Arc::new(|| 1_800_000_000_000));
    (catch, audit)
}

fn entry(op: &str) -> AuditEntry {
    AuditEntry {
        device: "Son's iPhone".into(),
        device_id: "dev-1".into(),
        op: op.into(),
        session: Some("s1".into()),
        length: Some(5),
        address: Some("203.0.113.9".into()),
        stopped_pid: Some(4242),
    }
}

fn text(event: &Event, field: &str) -> Option<String> {
    match event.fields.get(field)? {
        FieldValue::Name(name) => Some((*name).to_owned()),
        FieldValue::Ref { id, .. } => Some(id.to_string()),
        FieldValue::UInt(n) => Some(n.to_string()),
        _ => None,
    }
}

#[test]
fn each_bridge_op_maps_to_its_kind() {
    let table = [
        ("pair", "phone.paired", ("action", "paired")),
        ("revoke", "phone.paired", ("action", "revoked")),
        ("message", "phone.acted", ("op", "message")),
        ("sendNow", "phone.acted", ("op", "send_now")),
        ("image", "phone.acted", ("op", "image")),
        ("answer", "phone.acted", ("op", "answer")),
        ("newSession", "phone.acted", ("op", "new_session")),
        ("takeover", "phone.acted", ("op", "other")),
        (
            "inbox.answer:allow_once",
            "phone.answered",
            ("result", "taken"),
        ),
        (
            "inbox.answer:refused:acting_off",
            "phone.answered",
            ("result", "acting_off"),
        ),
        (
            "inbox.answer:refused:not_taken",
            "phone.answered",
            ("result", "not_taken"),
        ),
    ];
    for (op, kind, (field, value)) in table {
        let (catch, audit) = rig();
        audit.append(entry(op));
        let seen = catch.seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "{op}");
        assert_eq!(seen[0].0.spec.name, kind, "{op}");
        assert_eq!(text(&seen[0].0, field).as_deref(), Some(value), "{op}");
        assert_eq!(text(&seen[0].0, "device").as_deref(), Some("dev-1"), "{op}");
    }
}

#[test]
fn answered_action_is_recorded() {
    let (catch, audit) = rig();
    audit.append(entry("inbox.answer:allow_always"));
    let seen = catch.seen.lock().unwrap();
    assert_eq!(text(&seen[0].0, "action").as_deref(), Some("allow_always"));
}

#[test]
fn every_event_has_the_phone_as_its_actor() {
    let (catch, audit) = rig();
    audit.append(entry("message"));
    let seen = catch.seen.lock().unwrap();
    let actor = seen[0].1.actor.clone().unwrap();
    assert_eq!(actor.tier, ActorTier::Phone);
    assert_eq!(actor.via, Some(Via::Bridge));
    assert_eq!(actor.device.unwrap().as_str(), "dev-1");
    assert!(scope::current().actor.is_none(), "the scope is left again");
}

#[test]
fn phone_message_text_never_written() {
    let (catch, audit) = rig();
    audit.append(entry("message"));
    let seen = catch.seen.lock().unwrap();
    let event = &seen[0].0;
    assert_eq!(text(event, "chars").as_deref(), Some("5"));
    let shown = format!("{event:?}");
    for private in ["203.0.113.9", "4242", "Son's iPhone"] {
        assert!(!shown.contains(private), "{private}");
    }
}

#[test]
fn a_device_with_no_id_falls_back_to_its_name() {
    let (catch, audit) = rig();
    let mut old = entry("pair");
    old.device_id.clear();
    audit.append(old);
    let seen = catch.seen.lock().unwrap();
    assert_eq!(text(&seen[0].0, "device").as_deref(), Some("Son's iPhone"));
}

#[test]
fn revoke_goes_through_the_sink() {
    use crate::bridge::net::core::{tests::parts, Core};
    use crate::bridge::net::jwt::tests::{TestClock, START};
    use crate::bridge::test_support::FakeBackend;
    use crate::sessions::testutil::TempDir;
    let dir = TempDir::new("bridge-revoke-events");
    let (catch, audit) = rig();
    let clock = TestClock::new(START);
    let core = Core::with_audit(
        &dir.path("bridge"),
        parts(
            Arc::new(FakeBackend::default()),
            &dir.path("home"),
            clock.clock(),
        ),
        Arc::new(audit),
    );
    let (device, _) = core.devices.register("Pixel", "android", START).unwrap();
    core.revoke(&device.id).unwrap();
    let seen = catch.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0.spec.name, "phone.paired");
    assert_eq!(text(&seen[0].0, "action").as_deref(), Some("revoked"));
    assert_eq!(
        text(&seen[0].0, "device").as_deref(),
        Some(device.id.as_str())
    );
    assert!(
        !dir.path("bridge/remote-log.jsonl").exists(),
        "remote-log.jsonl is not written"
    );
}
