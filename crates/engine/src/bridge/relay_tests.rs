use super::*;

fn parts(act: &HeldAct) -> HeldAct {
    HeldAct::from_parts(act.op(), &act.args()).expect("parses back")
}

#[test]
fn every_act_survives_being_stored_and_read_back() {
    let acts = [
        HeldAct::Message {
            session_id: "m5/job-7".into(),
            text: "go on".into(),
        },
        HeldAct::SendNow {
            session_id: "m5/job-7".into(),
        },
        HeldAct::Answer {
            request_id: "req-1".into(),
            session_id: "m5/job-7".into(),
            allow: Some(false),
            answers: None,
        },
        HeldAct::Answer {
            request_id: "req-2".into(),
            session_id: "m5/job-7".into(),
            allow: None,
            answers: Some(vec!["Blue".into(), "free text".into()]),
        },
        HeldAct::NewSession {
            cwd: "/Users/example/src/app".into(),
            model: Some("opus".into()),
            model_id: None,
        },
    ];
    for act in &acts {
        assert_eq!(&parts(act), act);
        assert!(act.args().get("op").is_none(), "the op is stored apart");
    }
}

#[test]
fn an_unknown_op_or_a_broken_args_value_is_refused() {
    assert!(HeldAct::from_parts("session.takeover", &json!({})).is_none());
    assert!(HeldAct::from_parts("session.message", &json!({"text": "x"})).is_none());
    assert!(HeldAct::from_parts("session.message", &json!("x")).is_none());
}

#[test]
fn summaries_are_short_single_lines() {
    let long = HeldAct::Message {
        session_id: "s".into(),
        text: format!("first line\nsecond {}", "x".repeat(200)),
    };
    let summary = long.summary();
    assert!(summary.chars().count() <= 83, "{summary}");
    assert!(!summary.contains('\n'));
    assert!(summary.starts_with("first line second"));
    let new = HeldAct::NewSession {
        cwd: "/a".into(),
        model: None,
        model_id: None,
    };
    assert_eq!(new.summary(), "New session in /a");
}

#[test]
fn the_list_is_camel_case_and_leaves_out_empty_notices() {
    let list = HeldList {
        entries: vec![HeldView {
            key: None,
            id: "h1".into(),
            device: "phone-1".into(),
            op: "session.message".into(),
            session_id: Some("s".into()),
            summary: "hi".into(),
            at_ms: 5,
            waiting: true,
        }],
        notices: Vec::new(),
    };
    assert_eq!(
        list_json(&list),
        json!({"entries": [{"id": "h1", "op": "session.message", "sessionId": "s", "summary": "hi", "at": 5, "waiting": true}]})
    );
    assert_eq!(
        held_json(&HeldRef {
            id: "h1".into(),
            at_ms: 5
        }),
        json!({"held": {"id": "h1", "at": 5}})
    );
}
