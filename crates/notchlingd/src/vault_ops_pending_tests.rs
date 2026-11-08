//! `vault.pending`: an app that connects (or reconnects) while agents wait sees their approvals
//! and forms, as the events it missed carried them.

use super::tests::{agent, app, err, pending_use, seed, setup};
use super::*;
use std::sync::atomic::Ordering;

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
