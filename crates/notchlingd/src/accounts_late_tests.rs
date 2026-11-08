//! Accounts ops held up by the Keychain answer within the client's 5 s and hand their result over
//! later, through `accounts.result` and the `accounts.done` event. Every value is a fake one.

use crate::accounts_ops_tests::{profile, slot, world, ID1, ID2};
use crate::acting_ops_tests::app;
use crate::op_group::OpGroup;
use notchling_engine::accounts::claude::{LIVE_SERVICE, SAVED_SERVICE};
use notchling_engine::accounts::fake::{MemoryKeychain, ScriptedHttp};
use notchling_engine::accounts::http::PROFILE_URL;
use notchling_engine::accounts::keychain::{KeychainError, SecurityOutput, SecurityRunner};
use notchling_local_api::{ops, ErrorCode};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// A Keychain that waits, as on a macOS prompt, while closed.
pub(crate) struct Gate {
    inner: Arc<MemoryKeychain>,
    closed: Mutex<bool>,
    opened: Condvar,
    pub(crate) waiting: AtomicBool,
}

impl Gate {
    pub(crate) fn over(inner: Arc<MemoryKeychain>) -> Arc<Gate> {
        Arc::new(Gate {
            inner,
            closed: Mutex::new(false),
            opened: Condvar::new(),
            waiting: AtomicBool::new(false),
        })
    }

    pub(crate) fn close(&self) {
        *self.closed.lock().unwrap() = true;
    }

    pub(crate) fn open(&self) {
        *self.closed.lock().unwrap() = false;
        self.opened.notify_all();
    }
}

impl SecurityRunner for Gate {
    fn run(&self, args: &[String], stdin: Option<&[u8]>) -> Result<SecurityOutput, KeychainError> {
        let mut closed = self.closed.lock().unwrap();
        while *closed {
            self.waiting.store(true, Ordering::SeqCst);
            closed = self.opened.wait(closed).unwrap();
        }
        drop(closed);
        self.inner.run(args, stdin)
    }
}

fn signed_in_to_one(
    w: &crate::accounts_ops_tests::World,
    chain: &MemoryKeychain,
    http: &ScriptedHttp,
) {
    std::fs::write(
        w.d.0.join("home/.claude.json"),
        "{\"oauthAccount\": {\"accountUuid\": \"a1a1\", \"organizationUuid\": \"0101\", \"emailAddress\": \"one@example.com\"}}",
    )
    .unwrap();
    chain.put(
        LIVE_SERVICE,
        "me",
        r#"{"claudeAiOauth":{"accessToken":"test-access-1"}}"#,
    );
    http.answer(PROFILE_URL, 200, &profile(ID1, "one@example.com"));
    chain.put(SAVED_SERVICE, ID2, &slot(ID2, "two@example.com", "2"));
}

#[test]
fn a_switch_stuck_on_the_keychain_answers_working_then_hands_its_result_over() {
    let chain = Arc::new(MemoryKeychain::new());
    let http = Arc::new(ScriptedHttp::new());
    let gate = Gate::over(chain.clone());
    let w = world("late-switch", true, gate.clone(), http.clone());
    signed_in_to_one(&w, &chain, &http);
    w.ops.set_reply_wait(Duration::from_millis(200));
    gate.close();

    let began = Instant::now();
    let got = w
        .ops
        .call(&app(), ops::ACCOUNTS_SWITCH, &json!({"id": ID2}))
        .unwrap();
    assert!(began.elapsed() < Duration::from_secs(3), "the call waited");
    assert_eq!(got["state"], "working");
    let ticket = got["ticket"].clone();
    // The list doesn't wait behind it either.
    let list = w.ops.call(&app(), ops::ACCOUNTS_LIST, &json!({})).unwrap();
    assert_eq!(list["on"], true);

    gate.open();
    let result = w
        .ops
        .call(&app(), ops::ACCOUNTS_RESULT, &json!({"ticket": ticket}))
        .unwrap();
    assert_eq!(result["active"], ID2);
    let done = w.room.named("accounts.done");
    assert_eq!(done.len(), 1);
    assert_eq!(
        (done[0]["ticket"].clone(), done[0]["ok"].clone()),
        (ticket.clone(), json!(true))
    );
    assert_eq!(done[0]["result"]["active"], ID2);
    let printed = format!("{result} {done:?}");
    assert!(!printed.contains("test-access"), "{printed}");
    // Read once.
    let e = w
        .ops
        .call(&app(), ops::ACCOUNTS_RESULT, &json!({"ticket": ticket}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
}

#[test]
fn a_late_refusal_reaches_the_app_as_an_error() {
    let chain = Arc::new(MemoryKeychain::new());
    let http = Arc::new(ScriptedHttp::new());
    let gate = Gate::over(chain.clone());
    let w = world("late-refusal", true, gate.clone(), http.clone());
    signed_in_to_one(&w, &chain, &http);
    w.ops.set_reply_wait(Duration::from_millis(100));
    gate.close();
    let got = w
        .ops
        .call(&app(), ops::ACCOUNTS_SWITCH, &json!({"id": "a9a9:0909"}))
        .unwrap();
    let ticket = got["ticket"].clone();
    gate.open();
    let e = w
        .ops
        .call(&app(), ops::ACCOUNTS_RESULT, &json!({"ticket": ticket}))
        .unwrap_err();
    assert!(e.message.starts_with("No saved login"), "{}", e.message);
    let done = w.room.named("accounts.done");
    assert_eq!(done[0]["ok"], false);
    assert!(done[0]["error"]
        .as_str()
        .unwrap_or_default()
        .starts_with("No saved login"));
}

#[test]
fn a_sync_stuck_on_the_keychain_answers_the_list_as_last_read() {
    let chain = Arc::new(MemoryKeychain::new());
    let http = Arc::new(ScriptedHttp::new());
    let gate = Gate::over(chain.clone());
    let w = world("late-sync", true, gate.clone(), http.clone());
    signed_in_to_one(&w, &chain, &http);
    w.ops.set_reply_wait(Duration::from_millis(100));
    gate.close();
    let began = Instant::now();
    let list = w
        .ops
        .call(&app(), ops::ACCOUNTS_LIST, &json!({"sync": true}))
        .unwrap();
    assert!(began.elapsed() < Duration::from_secs(3));
    assert_eq!(list["syncing"], true);
    assert!(list.get("accounts").is_some());
    gate.open();
    let deadline = Instant::now() + Duration::from_secs(5);
    while w.room.named("accounts.done").is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(w.room.named("accounts.done")[0]["result"]["active"], ID1);
    assert_eq!(w.room.named("accounts.changed").len(), 1);
}
