use crate::cluster_relay_testkit::*;
use notchling_engine::bridge::relay::{Call, HeldAct, Relay, Routed};
use notchling_engine::cluster::wire::WireError;
use serde_json::json;

#[test]
fn held_answer_to_a_closed_prompt_is_dropped_and_reported() {
    let rig = rig(M1, M5, false);
    let act = HeldAct::Answer {
        request_id: format!("{M5}/p1"),
        session_id: format!("{M5}/job-7"),
        allow: None,
        answers: Some(vec!["Blue".into()]),
    };
    let found = session(&format!("{M5}/job-7"));
    let routed = rig.relay.route(&Call {
        device: DEV,
        request: "9",
        key: None,
        act: &act,
        session: Some(&found),
    });
    assert!(matches!(routed, Routed::Held(_)));
    go(&rig, M1, M5, true);
    *rig.net.reply.lock().unwrap() = Err(WireError::new("failed", "no such prompt"));
    rig.relay.flush();
    rig.relay.flush();
    assert_eq!(
        sent(&rig),
        [(
            M5.to_string(),
            "session.answer".to_string(),
            json!({"id": "p1", "answers": [[2]]})
        )],
        "tried once, as an option number"
    );
    let list = rig.relay.list(DEV);
    assert!(list.entries.is_empty());
    assert_eq!(list.notices.len(), 1);
    assert_eq!(
        list.notices[0].reason,
        "m5 closed that question before your answer arrived"
    );
    assert!(
        rig.relay.list(OTHER).notices.is_empty(),
        "another phone isn't told"
    );
}

#[test]
fn a_held_allow_goes_as_a_permission_decision() {
    let rig = rig(M1, M5, false);
    let act = HeldAct::Answer {
        request_id: format!("{M5}/p2"),
        session_id: format!("{M5}/job-7"),
        allow: Some(true),
        answers: None,
    };
    let found = session(&format!("{M5}/job-7"));
    rig.relay.route(&Call {
        device: DEV,
        request: "9",
        key: None,
        act: &act,
        session: Some(&found),
    });
    go(&rig, M1, M5, true);
    rig.relay.flush();
    assert_eq!(sent(&rig)[0].1, "session.permission");
}

#[test]
fn reads_are_answered_from_the_set_marked_stale() {
    let rig = rig(M1, M5, false);
    assert_eq!(rig.relay.stale_ms(), Some(T0 - 90_000));
    let mut found = session(&format!("{M5}/job-7"));
    found.transcript_path = Some("/Users/example/.claude/projects/app/job-7.jsonl".into());
    assert_eq!(rig.relay.set_transcript(&found), Some(rig.set.kept.clone()));
    let mut worker = session(&format!("{W}/job-3"));
    worker.transcript_path = found.transcript_path.clone();
    assert_eq!(rig.relay.set_transcript(&worker), None);
    go(&rig, M1, M5, true);
    assert_eq!(rig.relay.stale_ms(), None);
    assert_eq!(rig.relay.set_transcript(&found), None);
}

#[test]
fn phone_session_new_relays_to_primary() {
    let rig = rig(M1, M5, true);
    let act = HeldAct::NewSession {
        cwd: "/Users/example/src/app".into(),
        model: Some("opus".into()),
        model_id: None,
    };
    let routed = rig.relay.route(&Call {
        device: DEV,
        request: "17",
        key: None,
        act: &act,
        session: None,
    });
    assert_eq!(
        routed,
        Routed::Done(json!({"started": true, "relayed": true}))
    );
    assert_eq!(
        sent(&rig),
        [(
            M5.to_string(),
            "session.new".to_string(),
            json!({"folder": "/Users/example/src/app", "flags": ["--model", "claude-opus-5-5"]})
        )]
    );
}
