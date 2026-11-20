use super::tests::{agent, app, err, pending_use, seed, setup};
use super::*;
use crate::vault_caller::Processes;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

pub(super) struct FakeProcesses;

impl Processes for FakeProcesses {
    fn parent(&self, pid: u32) -> Option<u32> {
        Some(match pid {
            41 => 42,
            42 => 1,
            _ => 41,
        })
    }
    fn name(&self, pid: u32) -> Option<String> {
        Some(if pid == 41 { "zsh" } else { "claude" }.to_string())
    }
    fn cwd(&self, _pid: u32) -> Option<PathBuf> {
        Some(PathBuf::from("/w/app"))
    }
    fn resolve(&self, _path: &Path) -> Option<PathBuf> {
        None
    }
}

#[test]
fn the_approval_says_what_the_daemon_checked_about_the_caller() {
    let s = setup("verified", true);
    seed(&s, true);
    let (user, event) = pending_use(&s);
    assert_eq!(event["folder"], "/w/app");
    assert_eq!(
        event["verified"],
        json!({"exe": "/fake/peer/notchling", "pid": std::process::id(), "cwd": "/w/app",
               "parents": ["zsh", "claude"], "folder_matches": true})
    );
    let id = event["id"].clone();
    s.ops
        .call(
            &app(),
            ops::VAULT_APPROVAL_ANSWER,
            &json!({"id": id, "decision": "deny"}),
        )
        .unwrap();
    assert!(user.join().unwrap().is_err());
}

fn trusting(session: Option<&'static str>) -> TrustedSession {
    Arc::new(move |_| session.map(str::to_owned))
}

fn pending_request(
    s: &super::tests::Setup,
    args: Value,
) -> (std::thread::JoinHandle<Result<Value, ApiError>>, Value) {
    s.room.follows.store(true, Ordering::SeqCst);
    let ops = s.ops.clone();
    let asker = std::thread::spawn(move || ops.call(&agent(), ops::VAULT_REQUEST, &args));
    let request = loop {
        if let Some(e) = s.room.pending(topics::VAULT_REQUEST_PENDING) {
            break e;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    (asker, request)
}

fn cancel(s: &super::tests::Setup, request: &Value) {
    s.ops
        .call(
            &app(),
            ops::VAULT_REQUEST_CANCEL,
            &json!({"id": request["id"]}),
        )
        .unwrap();
}

#[test]
fn the_pending_events_name_the_asking_session_only_when_it_is_trusted() {
    for (tag, session) in [("pending-trusted", Some("sess-1")), ("pending-no", None)] {
        let s = super::tests::setup_full(tag, true, None, None, Some(trusting(session)));
        seed(&s, true);
        let (user, approval) = pending_use(&s);
        assert_eq!(approval.get("session").and_then(Value::as_str), session);
        s.ops
            .call(
                &app(),
                ops::VAULT_APPROVAL_ANSWER,
                &json!({"id": approval["id"], "decision": "deny"}),
            )
            .unwrap();
        assert!(user.join().unwrap().is_err());
    }
    let s = super::tests::setup_full(
        "pending-request",
        true,
        None,
        None,
        Some(trusting(Some("sess-2"))),
    );
    let (asker, request) = pending_request(&s, json!({"name": "npm", "folder": "/w/app"}));
    assert_eq!(request["session"], "sess-2");
    assert_eq!(request["verified"]["cwd"], "/w/app");
    assert_eq!(request["verified"]["parents"], json!(["zsh", "claude"]));
    cancel(&s, &request);
    assert!(asker.join().unwrap().is_err());
    let s = setup("pending-untrusted", true);
    let (asker, request) = pending_request(&s, json!({"name": "npm"}));
    assert!(request.get("session").is_none());
    cancel(&s, &request);
    assert!(asker.join().unwrap().is_err());
}

#[test]
fn a_reconnecting_app_sees_what_waits_for_it() {
    let s = setup("pending", true);
    assert_eq!(s.ops.spec(ops::VAULT_PENDING), Some(OpSpec::APP));
    let none = s.ops.call(&app(), ops::VAULT_PENDING, &json!({})).unwrap();
    assert_eq!(none, json!({"approvals": [], "requests": []}));
    seed(&s, true);
    let (user, approval) = pending_use(&s);
    s.room.follows.store(true, Ordering::SeqCst);
    let ops = s.ops.clone();
    let asker = std::thread::spawn(move || {
        ops.call(
            &agent(),
            ops::VAULT_REQUEST,
            &json!({"name": "npm", "reason": "to publish"}),
        )
    });
    let request = loop {
        if let Some(e) = s.room.pending(topics::VAULT_REQUEST_PENDING) {
            break e;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let waiting = s.ops.call(&app(), ops::VAULT_PENDING, &json!({})).unwrap();
    assert_eq!(
        waiting,
        json!({"approvals": [approval.clone()], "requests": [request.clone()]})
    );
    assert!(!waiting.to_string().contains("fake-token"), "no value");
    s.ops
        .call(
            &app(),
            ops::VAULT_APPROVAL_ANSWER,
            &json!({"id": approval["id"], "decision": "deny"}),
        )
        .unwrap();
    s.ops
        .call(
            &app(),
            ops::VAULT_REQUEST_CANCEL,
            &json!({"id": request["id"]}),
        )
        .unwrap();
    assert!(user.join().unwrap().is_err());
    assert!(asker.join().unwrap().is_err());
    let after = s.ops.call(&app(), ops::VAULT_PENDING, &json!({})).unwrap();
    assert_eq!(after, json!({"approvals": [], "requests": []}));
    s.switch.set(false);
    assert_eq!(
        err(s.ops.call(&app(), ops::VAULT_PENDING, &json!({}))).code,
        ErrorCode::Forbidden
    );
}
