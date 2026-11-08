//! The hook's ops called directly, with the fake room of [`crate::acting_ops_tests`]: who may ask,
//! the held prompts, how they're answered and every way they pass.

use crate::acting_ops_tests::{app, caller, code, sessions, setup, Setup, GONE, PROMPT};
use notchling_local_api::{ops, Caller, ClientKind, ErrorCode};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

fn fixture(name: &str) -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/hook")
        .join(name);
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn hook() -> Caller {
    caller(ClientKind::Hook)
}

#[test]
fn only_the_hook_may_ask_and_it_passes_unless_someone_can_answer() {
    let s = setup(true, PROMPT, sessions());
    let ask = json!({"event": fixture("permission-request.json"), "waitMs": 5000});
    for kind in [ClientKind::App, ClientKind::Cli] {
        let (c, _) = code(s.ops.call(&caller(kind), ops::HOOK_REQUEST, &ask));
        assert_eq!(c, ErrorCode::Forbidden, "{kind:?} gets no decision");
    }
    assert!(s.ops.prompts().list().is_empty());
    assert!(s.room.named("prompt.pending").is_empty());

    let other = json!({"event": fixture("other-tool.json")});
    assert_eq!(
        s.ops.call(&hook(), ops::HOOK_REQUEST, &other).unwrap()["action"],
        "pass"
    );
    s.room.app.store(false, Ordering::SeqCst);
    let out = s.ops.call(&hook(), ops::HOOK_REQUEST, &ask).unwrap();
    assert_eq!(
        out,
        json!({"action": "pass", "reason": "no app is attached"})
    );

    let off = setup(false, PROMPT, sessions());
    let out = off.ops.call(&hook(), ops::HOOK_REQUEST, &ask).unwrap();
    assert_eq!(out, json!({"action": "pass", "reason": "acting is off"}));
    assert!(off.room.named("prompt.pending").is_empty());
}

/// Runs a hook request on its own thread and waits for its prompt to be pending.
fn held(s: &Arc<Setup>, event: Value, wait_ms: u64) -> (std::thread::JoinHandle<Value>, String) {
    let ops = s.clone();
    let args = json!({"event": event, "pid": std::process::id(), "waitMs": wait_ms});
    let before = s.room.named("prompt.pending").len();
    let thread =
        std::thread::spawn(move || ops.ops.call(&hook(), ops::HOOK_REQUEST, &args).unwrap());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(p) = s.room.named("prompt.pending").get(before) {
            return (thread, p["id"].as_str().unwrap().to_string());
        }
        assert!(std::time::Instant::now() < deadline, "never pending");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_apps_permission_decision_reaches_the_hook() {
    let s = Arc::new(setup(true, PROMPT, sessions()));
    let (thread, id) = held(&s, fixture("permission-request.json"), 10_000);
    let listed = s.ops.call(&app(), ops::PROMPTS_LIST, &json!({})).unwrap();
    assert_eq!(listed[0]["id"], json!(id));
    let out = s
        .ops
        .call(
            &app(),
            ops::SESSION_PERMISSION,
            &json!({"id": id, "decision": "always"}),
        )
        .unwrap();
    assert_eq!(out, json!({"decided": "always", "promptId": id}));
    assert_eq!(
        thread.join().unwrap(),
        json!({"action": "allow", "always": true})
    );
    let resolved = s.room.named("prompt.resolved");
    assert_eq!(resolved[0]["by"], "app");
    assert_eq!(resolved[0]["outcome"], "always");
    // Once only.
    assert!(s
        .ops
        .call(
            &app(),
            ops::SESSION_PERMISSION,
            &json!({"id": id, "decision": "deny"})
        )
        .is_err());
}

#[test]
fn a_held_question_is_read_by_row_and_answered_by_prompt_id_only() {
    let s = Arc::new(setup(true, PROMPT, sessions()));
    let (thread, id) = held(&s, fixture("ask-user-question.json"), 10_000);
    let read = s
        .ops
        .call(
            &caller(ClientKind::Cli),
            ops::SESSION_QUESTION,
            &json!({"id": "job-a"}),
        )
        .unwrap();
    assert_eq!(read["promptId"], json!(id));
    assert_eq!(read["items"][1]["question"], "Where?");
    assert_eq!(
        code(s.ops.call(
            &app(),
            ops::SESSION_PERMISSION,
            &json!({"id": id, "decision": "allow"})
        ))
        .0,
        ErrorCode::BadRequest,
        "a question isn't allowed, it's answered"
    );
    // A row id never reaches a held prompt: only its id does (no question given, nothing typed).
    assert_eq!(
        code(s.ops.call(
            &app(),
            ops::SESSION_ANSWER,
            &json!({"id": "job-a", "answers": [[1, 3], [2]]}),
        ))
        .0,
        ErrorCode::BadRequest
    );
    assert_eq!(s.ops.prompts().list().len(), 1, "still held");
    let out = s
        .ops
        .call(
            &app(),
            ops::SESSION_ANSWER,
            &json!({"id": id, "answers": [[1, 3], [2]]}),
        )
        .unwrap();
    assert_eq!(out, json!({"answered": "hook", "promptId": id}));
    assert_eq!(
        thread.join().unwrap(),
        json!({"action": "answer", "answers": {"Which pets?": "Cat, Fish", "Where?": "Away"}})
    );
    assert!(
        s.writes.lock().unwrap().is_empty(),
        "nothing typed for a held prompt"
    );
}

#[test]
fn a_held_hook_passes_at_its_deadline_or_when_the_app_leaves() {
    let s = Arc::new(setup(true, PROMPT, sessions()));
    let (thread, _) = held(&s, fixture("permission-request.json"), 300);
    assert_eq!(
        thread.join().unwrap(),
        json!({"action": "pass", "reason": "timeout"})
    );
    assert_eq!(s.room.named("prompt.resolved")[0]["by"], "timeout");
    assert!(s.ops.prompts().list().is_empty());

    let (thread, _) = held(&s, fixture("permission-request.json"), 10_000);
    s.room.app.store(false, Ordering::SeqCst);
    assert_eq!(thread.join().unwrap()["reason"], "no_app");
}

#[test]
fn turning_acting_off_passes_every_held_hook_at_once() {
    let s = Arc::new(setup(true, PROMPT, sessions()));
    let (first, _) = held(&s, fixture("permission-request.json"), 60_000);
    let (second, _) = held(&s, fixture("ask-user-question.json"), 60_000);
    let started = std::time::Instant::now();
    s.ops
        .call(&app(), ops::SETTINGS_SET_ACTING, &json!({"on": false}))
        .unwrap();
    for thread in [first, second] {
        assert_eq!(
            thread.join().unwrap(),
            json!({"action": "pass", "reason": "acting is off"})
        );
    }
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(s.ops.prompts().list().is_empty());
    let resolved = s.room.named("prompt.resolved");
    assert_eq!(resolved.len(), 2);
    assert!(resolved.iter().all(|r| r["by"] == "acting_off"));
}

#[test]
fn a_hook_claude_code_gave_up_on_is_let_go() {
    let s = Arc::new(setup(true, PROMPT, sessions()));
    let ops = s.clone();
    let args = json!({"event": fixture("permission-request.json"), "waitMs": 10_000});
    let gone = Caller {
        pid: Some(GONE),
        ..hook()
    };
    let out = ops.ops.call(&gone, ops::HOOK_REQUEST, &args).unwrap();
    assert_eq!(out["reason"], "hook_gone");
}

#[test]
fn a_hook_is_its_process_never_its_word() {
    let s = setup(true, PROMPT, sessions());
    let ask = json!({"event": fixture("permission-request.json"), "pid": 4243, "waitMs": 50});
    // No peer pid, or one no Claude Code session started (whatever `pid` says): refused.
    for pid in [None, Some(1), Some(31_337)] {
        let posing = Caller { pid, ..hook() };
        let (c, m) = code(s.ops.call(&posing, ops::HOOK_REQUEST, &ask));
        assert_eq!(c, ErrorCode::Forbidden, "{pid:?}: {m}");
    }
    // The right process, but the event names a session its Claude Code isn't.
    let mut other = fixture("permission-request.json");
    other["session_id"] = json!("someone-else");
    let (c, _) = code(
        s.ops
            .call(&hook(), ops::HOOK_REQUEST, &json!({"event": other})),
    );
    assert_eq!(c, ErrorCode::Forbidden);
    assert!(s.room.named("prompt.pending").is_empty());
    // The real one is held under its own pid.
    let out = s.ops.call(&hook(), ops::HOOK_REQUEST, &ask).unwrap();
    assert_eq!(out["reason"], "timeout");
    let pending = s.room.named("prompt.pending");
    assert_eq!(pending.len(), 1);
}
