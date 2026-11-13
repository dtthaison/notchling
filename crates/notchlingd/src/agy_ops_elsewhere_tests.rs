use super::*;
use notchling_engine::tools::agy::elsewhere::{Holder, DELEGATION_NOTE};
use std::sync::atomic::AtomicBool;

const DELEGATION: Holder = Holder {
    pid: 64817,
    delegation: true,
};

fn held_rig() -> (Rig, Arc<AtomicBool>, PathBuf) {
    let rig = rig(true);
    let held = Arc::new(AtomicBool::new(true));
    let flag = held.clone();
    rig.ops.elsewhere().set_finder(
        Arc::new(move |session: &str| {
            (session == SESSION && flag.load(Ordering::SeqCst)).then_some(DELEGATION)
        }),
        Duration::from_millis(20),
    );
    let log = rig.root.join(format!(
        "brain/{SESSION}/.system_generated/logs/transcript.jsonl"
    ));
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(
        &log,
        concat!(
            r#"{"type":"USER_INPUT","created_at":"2026-11-11T16:43:10Z","content":"<USER_REQUEST>\nReview it.\n</USER_REQUEST>"}"#,
            "\n"
        ),
    )
    .unwrap();
    (rig, held, log)
}

fn texts(reply: &Value) -> Vec<String> {
    reply["conversation"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            format!(
                "{}: {}",
                i["kind"]["type"].as_str().unwrap(),
                i["kind"]["data"]["text"].as_str().unwrap_or("")
            )
        })
        .collect()
}

#[test]
fn a_session_a_delegation_holds_is_read_from_its_log_and_never_sent_to() {
    let (rig, _held, _log) = held_rig();
    let opened = rig.call(OPEN, json!({"id": ROW})).unwrap();
    assert_eq!(opened["elsewhere"], DELEGATION_NOTE);
    assert_eq!(opened["loading"], false);
    assert!(rig.spawner.spawned().is_empty(), "no second agy on it");

    let read = rig.call(CONVERSATION, json!({"id": ROW})).unwrap();
    assert_eq!(read["open"], false);
    assert_eq!(read["working"], true, "the prompt has no answer yet");
    assert_eq!(read["elsewhere"], DELEGATION_NOTE);
    assert_eq!(
        texts(&read),
        [
            "user: Review it.".to_owned(),
            format!("notice: {DELEGATION_NOTE}")
        ]
    );
    let state = rig.call(STATE, json!({"id": ROW})).unwrap();
    assert_eq!(state["elsewhere"], DELEGATION_NOTE);
    assert_eq!(state["working"], true);

    let (code, message) = refused(rig.call(SEND, json!({"id": ROW, "text": "are you working?"})));
    assert_eq!(
        (code, message.as_str()),
        (ErrorCode::Failed, DELEGATION_NOTE)
    );
    assert!(rig.spawner.spawned().is_empty());
}

#[test]
fn the_log_growing_is_announced_and_the_answer_shows() {
    let (rig, _held, log) = held_rig();
    rig.call(OPEN, json!({"id": ROW})).unwrap();
    let mut file = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
    use std::io::Write;
    writeln!(
        file,
        r#"{{"type":"PLANNER_RESPONSE","created_at":"2026-11-11T16:44:54Z","content":"VERDICT: FIX"}}"#
    )
    .unwrap();
    drop(file);
    rig.change("elsewhere");
    let read = rig.call(CONVERSATION, json!({"id": ROW})).unwrap();
    assert_eq!(read["working"], false);
    assert!(texts(&read).contains(&"assistant: VERDICT: FIX".to_owned()));
}

#[test]
fn once_the_delegation_lets_go_the_session_opens_as_usual() {
    let (rig, held, _log) = held_rig();
    rig.call(OPEN, json!({"id": ROW})).unwrap();
    held.store(false, Ordering::SeqCst);
    rig.change("released");
    let state = rig.call(STATE, json!({"id": ROW})).unwrap();
    assert_eq!(state["elsewhere"], Value::Null);
    rig.loaded();
    assert_eq!(rig.spawner.spawned().len(), 1);
}

#[test]
fn a_delegation_that_takes_the_session_after_chat_opened_it_blocks_the_send_then_reloads() {
    let (rig, held, _log) = held_rig();
    held.store(false, Ordering::SeqCst);
    let first = rig.loaded();
    held.store(true, Ordering::SeqCst);
    let (_, message) = refused(rig.call(SEND, json!({"id": ROW, "text": "hi"})));
    assert_eq!(message, DELEGATION_NOTE);
    assert!(
        first.requests("session/prompt").is_empty(),
        "nothing typed into it"
    );
    held.store(false, Ordering::SeqCst);
    rig.change("released");
    assert!(
        first.killed(),
        "the stale copy is closed, so the next open loads it all"
    );
}
