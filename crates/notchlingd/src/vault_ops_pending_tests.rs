//! `vault.pending`: an app that connects (or reconnects) while agents wait sees their approvals
//! and forms, as the events it missed carried them.

use super::tests::{agent, app, err, pending_use, seed, setup};
use super::*;
use crate::vault_caller::Processes;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

/// Every process works in `/w/app` and runs under `zsh`, under `claude`, under launchd.
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
    // What the caller claims stays as it was; beside it, what the kernel says.
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
