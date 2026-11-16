use super::*;
use crate::outbox_testkit::{rig, Rig, IDLE};
use notchling_engine::bridge::model::SessionState;
use notchling_engine::outbox::{State, Via};

#[test]
fn answers_become_option_numbers_or_text() {
    let item = PromptQuestion {
        header: "Pick".into(),
        question: "Which?".into(),
        options: vec!["Red".into(), "Blue".into()],
        multi_select: false,
    };
    let out = answer_json(&["Blue".into(), "green".into()], &[item.clone(), item]);
    assert_eq!(out, json!([[2], "green"]));
}

const CODEX_ROW: &str = "codex:019a5c3e-aaaa-7bbb-8ccc-000000000001";
const AGY_ROW: &str = "agy:5b6c7d8e-1f2a-4b3c-8d4e-9f0a1b2c3d4e";

fn phone(r: &Rig) -> DaemonActing {
    DaemonActing {
        ops: r.acting.clone(),
        sessions: r.acting.sessions.clone(),
        cluster: crate::bridge_cluster::PhoneCluster::none(),
    }
}

fn session(control: Control) -> Session {
    Session::new("s1", "n", "/tmp/demo", control, SessionState::Running)
}

#[test]
fn phone_message_saves_row_and_answers_sent() {
    let r = rig();
    phone(&r)
        .send_message("from the phone", &session(Control::Job(IDLE.into())))
        .unwrap();
    let rows = r.ops.store().list(Some(IDLE)).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text, "from the phone");
    assert_eq!(rows[0].via, Via::Phone);
    assert_eq!(rows[0].state, State::Queued);
}

#[test]
fn phone_message_to_codex_session_queues_row() {
    let r = rig();
    phone(&r)
        .send_message("to codex", &session(Control::Job(CODEX_ROW.into())))
        .unwrap();
    let rows = r.ops.store().list(Some(CODEX_ROW)).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].via, Via::Phone);
}

#[test]
fn phone_message_to_held_agy_session_fails_with_its_message() {
    let r = rig();
    let held = "Another agy has this conversation open.";
    r.ops.set_agy_check(Arc::new(move |_| {
        Err(ApiError::new(ErrorCode::Failed, held))
    }));
    let err = phone(&r)
        .send_message("to agy", &session(Control::Job(AGY_ROW.into())))
        .unwrap_err();
    assert_eq!(err.message, held);
    assert!(r.ops.store().list(None).unwrap().is_empty());
}
