use crate::activity_ops::ActivityOps;
use crate::consent_testkit::{shell, verified};
use crate::op_group::OpGroup;
use notchling_engine::activity::{ActivityRow, EventsDb};
use notchling_local_api::{bind_identity, ops, Caller, ClientKind, Tier};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

const OWN: &str = "s-own";
const OTHER: &str = "s-other";
const OTHER_FOLDER: &str = "/planted/folder/of/another/session";
const OTHER_NOTE: &str = "planted-note-of-another-session";

fn agent() -> Caller {
    Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
        peer: None,
        pid: Some(4242),
    }
}

fn row(seq: u64, kind: &str, session: Option<&str>, actor: Option<&str>) -> ActivityRow {
    let mut row = ActivityRow::blank(seq, 5_000 + seq as i64, kind);
    row.grp = "agents".to_owned();
    row.session = session.map(str::to_owned);
    row.actor_session = actor.map(str::to_owned);
    row
}

fn rig() -> ActivityOps {
    let mut db = EventsDb::memory().unwrap();
    let mut own = row(1, "session.reply", Some(OWN), None);
    own.f = json!({"result": "sent", "length": 5}).to_string();
    let mut own_by_actor = row(2, "session.outbox", None, Some(OWN));
    own_by_actor.f = json!({"then": "queued"}).to_string();
    let mut other = row(3, "session.reply", Some(OTHER), None);
    other.folder = Some(OTHER_FOLDER.to_owned());
    other.f = json!({"result": OTHER_NOTE}).to_string();
    let mut other_actor = row(4, "session.outbox", None, Some(OTHER));
    other_actor.folder = Some(OTHER_FOLDER.to_owned());
    other_actor.f = json!({"then": OTHER_NOTE}).to_string();
    let mut not_own_kind = row(5, "daemon.started", Some(OWN), None);
    not_own_kind.grp = "daemon".to_owned();
    db.insert(&[own, own_by_actor, other, other_actor, not_own_kind])
        .unwrap();
    ActivityOps::new(Arc::new(Mutex::new(db)), Arc::new(|_, _| None))
}

fn mine(ops: &ActivityOps, args: Value) -> Value {
    ops.call(&agent(), ops::ACTIVITY_MINE, &args).unwrap()
}

#[test]
fn it_is_an_agent_read() {
    let activity = rig();
    let spec = activity.spec(ops::ACTIVITY_MINE).unwrap();
    assert_eq!(spec.tier, Tier::Agent);
    assert!(!spec.approval);
}

#[test]
fn a_verified_session_reads_only_its_own_events_of_the_own_kinds() {
    let activity = rig();
    let _who = bind_identity(Some(verified(OWN)));
    let reply = mine(&activity, json!({}));
    let mut seqs: Vec<u64> = reply["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["seq"].as_u64().unwrap())
        .collect();
    seqs.sort_unstable();
    assert_eq!(seqs, [1, 2]);
    let text = reply.to_string();
    assert!(
        !text.contains(OTHER_FOLDER) && !text.contains(OTHER_NOTE),
        "{text}"
    );
    assert!(!text.contains(OTHER), "{text}");
}

#[test]
fn since_keeps_only_the_events_after_it() {
    let activity = rig();
    let _who = bind_identity(Some(verified(OWN)));
    let seqs = |reply: Value| -> Vec<u64> {
        reply["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["seq"].as_u64().unwrap())
            .collect()
    };
    assert_eq!(seqs(mine(&activity, json!({"since": 0}))), [2, 1]);
    assert_eq!(seqs(mine(&activity, json!({"since": 5001}))), [2]);
    assert!(seqs(mine(&activity, json!({"since": 5002}))).is_empty());
    let bad = activity.call(&agent(), ops::ACTIVITY_MINE, &json!({"since": "soon"}));
    assert!(bad.is_err());
}

#[test]
fn a_filter_cannot_widen_the_read() {
    let activity = rig();
    let _who = bind_identity(Some(verified(OWN)));
    let asked_for_other = mine(&activity, json!({"session": OTHER}));
    let text = asked_for_other.to_string();
    assert!(
        !text.contains(OTHER_FOLDER) && !text.contains(OTHER_NOTE),
        "{text}"
    );
    let by_group = mine(&activity, json!({"groups": ["secrets"]}));
    assert_eq!(by_group["rows"].as_array().unwrap().len(), 2);
    let by_kind = mine(&activity, json!({"event_kinds": ["daemon.started"]}));
    assert_eq!(by_kind["rows"], json!([]));
    let narrowed = mine(&activity, json!({"event_kinds": ["session.reply"]}));
    assert_eq!(narrowed["rows"].as_array().unwrap().len(), 1);
}

#[test]
fn a_caller_with_no_verified_session_gets_nothing_and_the_reason() {
    let activity = rig();
    let mut unverified = verified(OWN);
    unverified.verified = false;
    for who in [None, Some(shell("Terminal")), Some(unverified)] {
        let _bound = bind_identity(who);
        let reply = mine(&activity, json!({}));
        assert_eq!(reply["rows"], json!([]));
        assert_eq!(reply["reason"], "no_session");
        let text = reply.to_string();
        assert!(
            !text.contains(OTHER_FOLDER) && !text.contains(OTHER_NOTE),
            "{text}"
        );
    }
}
