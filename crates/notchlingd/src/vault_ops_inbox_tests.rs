use super::tests::{agent, app, pending_use, seed, setup_with, Setup};
use super::*;
use crate::acting_ops_tests::FakeRoom as InboxRoom;
use crate::inbox_ops::InboxOps;
use crate::limits_ops_tests::TempDir;
use std::sync::atomic::Ordering;

struct Rig {
    _tmp: TempDir,
    s: Setup,
    inbox: Arc<InboxOps>,
    inbox_room: Arc<InboxRoom>,
}

fn rig(tag: &str, timing: Option<VaultTiming>) -> Rig {
    let tmp = TempDir::new(tag);
    let inbox_room = Arc::new(InboxRoom::default());
    let inbox = InboxOps::new(tmp.path("inbox"), inbox_room.clone());
    let s = setup_with(tag, true, Some(inbox.clone()), timing);
    Rig {
        _tmp: tmp,
        s,
        inbox,
        inbox_room,
    }
}

fn short(approval: u64, request: u64) -> Option<VaultTiming> {
    Some(VaultTiming {
        approval_wait: Duration::from_millis(approval),
        request_wait: Duration::from_millis(request),
        slice: Duration::from_millis(5),
    })
}

impl Rig {
    fn list(&self) -> Vec<Value> {
        self.inbox.flush();
        let list = self
            .inbox
            .call(&app(), ops::INBOX_LIST, &json!({}))
            .unwrap();
        list["items"].as_array().unwrap().clone()
    }

    fn item(&self, id: &str) -> Value {
        self.list()
            .into_iter()
            .find(|item| item["id"] == id)
            .unwrap_or(Value::Null)
    }

    fn answer(&self, id: &str, action: &str) -> Value {
        self.inbox
            .call(
                &app(),
                ops::INBOX_ANSWER,
                &json!({"id": id, "action": action}),
            )
            .unwrap()
    }

    fn start_request(&self) -> (std::thread::JoinHandle<Result<Value, ApiError>>, Value) {
        self.s.room.follows.store(true, Ordering::SeqCst);
        let ops = self.s.ops.clone();
        let asker = std::thread::spawn(move || {
            ops.call(
                &agent(),
                ops::VAULT_REQUEST,
                &json!({"name": "npm", "reason": "to publish"}),
            )
        });
        let event = loop {
            if let Some(e) = self.s.room.pending(topics::VAULT_REQUEST_PENDING) {
                break e;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        (asker, event)
    }
}

fn approval_item_id(event: &Value) -> String {
    format!("vault-approval:{}", event["id"].as_str().unwrap())
}

fn request_item_id(event: &Value) -> String {
    format!("vault-request:{}", event["id"].as_str().unwrap())
}

#[test]
fn approval_raises_item_and_inbox_answer_decides() {
    let r = rig("inbox-approve", None);
    seed(&r.s, true);
    let (user, event) = pending_use(&r.s);
    let id = approval_item_id(&event);
    let item = r.item(&id);
    assert_eq!(item["kind"], "secret_approval");
    assert_eq!(item["title"], "Allow gh for curl \"$GH_URL\"");
    assert_eq!(item["detail"], "/w/app");
    assert_eq!(item["state"], "open");
    assert_eq!(item["open"], json!({"vault_approval": event["id"]}));
    assert_eq!(
        item["actions"],
        json!([
            {"id": "allow_once", "label": "Allow Once", "primary": true},
            {"id": "allow_always", "label": "Always Allow", "primary": false},
            {"id": "deny", "label": "Deny", "primary": false},
        ])
    );
    let created = item["created_ms"].as_i64().unwrap();
    assert_eq!(item["expires_ms"].as_i64().unwrap(), created + 10_010);
    let got = r.answer(&id, "allow_once");
    assert_eq!(got["answered"], true);
    let reply = user.join().unwrap().unwrap();
    assert_eq!(reply["env"]["GH_TOKEN"], "fake-token-value-0001");
    let item = r.item(&id);
    assert_eq!(item["state"], "answered");
    assert_eq!(item["answer"], "allow_once");
    assert_eq!(
        r.s.room.pending(topics::VAULT_APPROVAL_RESOLVED).unwrap()["decision"],
        "once"
    );
}

#[test]
fn inbox_deny_and_always_decide_the_use() {
    let r = rig("inbox-deny", None);
    seed(&r.s, true);
    let (user, event) = pending_use(&r.s);
    let id = approval_item_id(&event);
    r.answer(&id, "deny");
    assert_eq!(
        user.join().unwrap().unwrap_err().code,
        ErrorCode::NeedsApproval
    );
    assert_eq!(r.item(&id)["answer"], "deny");

    let (user, event) = pending_use(&r.s);
    let id = approval_item_id(&event);
    r.answer(&id, "allow_always");
    user.join().unwrap().unwrap();
    assert_eq!(r.item(&id)["answer"], "allow_always");
    assert_eq!(r.s.ops.store.entry("gh").unwrap().unwrap().ask, Some(false));
}

#[test]
fn window_answer_resolves_item() {
    let r = rig("inbox-window", None);
    seed(&r.s, true);
    for (decision, answer) in [
        ("once", "allow_once"),
        ("always", "allow_always"),
        ("deny", "deny"),
    ] {
        r.s.ops
            .call(
                &app(),
                ops::VAULT_SAVE,
                &json!({"entry": {"name": "gh", "kind": "token", "ask": true}, "replacing": "gh"}),
            )
            .unwrap();
        let (user, event) = pending_use(&r.s);
        r.s.ops
            .call(
                &app(),
                ops::VAULT_APPROVAL_ANSWER,
                &json!({"id": event["id"], "decision": decision}),
            )
            .unwrap();
        let _ = user.join().unwrap();
        let item = r.item(&approval_item_id(&event));
        assert_eq!(item["state"], "answered", "{decision}");
        assert_eq!(item["answer"], answer, "{decision}");
    }
}

#[test]
fn approval_timeout_expires_the_item() {
    let r = rig("inbox-timeout", short(120, 10_000));
    seed(&r.s, true);
    let (user, event) = pending_use(&r.s);
    assert!(user.join().unwrap().is_err());
    let item = r.item(&approval_item_id(&event));
    assert_eq!(item["state"], "expired");
    assert_eq!(item["answer"], Value::Null);
}

#[test]
fn the_switch_going_off_withdraws_the_item() {
    let r = rig("inbox-off", None);
    seed(&r.s, true);
    let (user, event) = pending_use(&r.s);
    r.s.switch.set(false);
    assert!(user.join().unwrap().is_err());
    assert_eq!(r.item(&approval_item_id(&event))["state"], "withdrawn");
}

#[test]
fn no_follower_still_refuses_at_once() {
    let r = rig("inbox-nofollow", None);
    seed(&r.s, true);
    let e =
        r.s.ops
            .call(&agent(), ops::VAULT_USE, &json!({"names": ["gh"]}))
            .unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsApproval);
    assert!(r.list().is_empty());
    let e =
        r.s.ops
            .call(&agent(), ops::VAULT_REQUEST, &json!({"name": "npm"}))
            .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert!(r.list().is_empty());
}

#[test]
fn each_approval_gets_its_own_item() {
    let r = rig("inbox-two", None);
    seed(&r.s, true);
    let (first, a) = pending_use(&r.s);
    let (second, b) = pending_use(&r.s);
    assert_ne!(approval_item_id(&a), approval_item_id(&b));
    r.answer(&approval_item_id(&a), "deny");
    r.answer(&approval_item_id(&b), "deny");
    assert!(first.join().unwrap().is_err());
    assert!(second.join().unwrap().is_err());
    assert_eq!(r.list().len(), 2);
}

#[test]
fn request_raises_an_item_the_app_opens() {
    let r = rig("inbox-request", None);
    let (asker, event) = r.start_request();
    let id = request_item_id(&event);
    let item = r.item(&id);
    assert_eq!(item["kind"], "secret_request");
    assert_eq!(item["title"], "Add secret npm");
    assert_eq!(item["detail"], "to publish");
    assert_eq!(item["open"], json!({"vault_request": event["id"]}));
    assert_eq!(
        item["actions"],
        json!([
            {"id": "open", "label": "Open Form", "primary": true},
            {"id": "cancel", "label": "Cancel", "primary": false},
        ])
    );
    r.s.ops
        .call(
            &app(),
            ops::VAULT_SAVE,
            &json!({"entry": {"name": "npm", "kind": "token"}, "value": {"secret": "fake-npm-1"},
                    "request": event["id"]}),
        )
        .unwrap();
    assert!(asker.join().unwrap().is_ok());
    let item = r.item(&id);
    assert_eq!(item["state"], "answered");
    assert_eq!(item["answer"], "open");
}

#[test]
fn request_cancel_from_inbox_cancels_request() {
    let r = rig("inbox-cancel", None);
    let (asker, event) = r.start_request();
    let id = request_item_id(&event);
    let got = r.answer(&id, "cancel");
    assert_eq!(got["answered"], true);
    let e = asker.join().unwrap().unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsApproval);
    assert_eq!(e.message, "The user cancelled.");
    let item = r.item(&id);
    assert_eq!(item["state"], "answered");
    assert_eq!(item["answer"], "cancel");
}

#[test]
fn request_cancel_from_the_window_withdraws_the_item() {
    let r = rig("inbox-cancel-window", None);
    let (asker, event) = r.start_request();
    r.s.ops
        .call(
            &app(),
            ops::VAULT_REQUEST_CANCEL,
            &json!({"id": event["id"]}),
        )
        .unwrap();
    assert!(asker.join().unwrap().is_err());
    assert_eq!(r.item(&request_item_id(&event))["state"], "withdrawn");
}

#[test]
fn request_timeout_expires_the_item() {
    let r = rig("inbox-request-timeout", short(10_000, 120));
    let (asker, event) = r.start_request();
    assert!(asker.join().unwrap().is_err());
    assert_eq!(r.item(&request_item_id(&event))["state"], "expired");
}

#[test]
fn inbox_events_follow_the_item() {
    let r = rig("inbox-events", None);
    seed(&r.s, true);
    let (user, event) = pending_use(&r.s);
    r.answer(&approval_item_id(&event), "deny");
    let _ = user.join().unwrap();
    r.inbox.flush();
    let changed = r.inbox_room.named("inbox.changed");
    assert_eq!(changed.len(), 2);
    assert_eq!(changed[0]["item"]["state"], "open");
    assert_eq!(changed[1]["item"]["state"], "answered");
}

#[test]
fn window_answer_first_wins_over_a_late_inbox_answer() {
    let r = rig("inbox-late", None);
    seed(&r.s, true);
    let (user, event) = pending_use(&r.s);
    let id = approval_item_id(&event);
    r.s.ops
        .call(
            &app(),
            ops::VAULT_APPROVAL_ANSWER,
            &json!({"id": event["id"], "decision": "deny"}),
        )
        .unwrap();
    let late = r.answer(&id, "allow_once");
    assert_eq!(late["answered"], false);
    assert!(user.join().unwrap().is_err());
    let item = r.item(&id);
    assert_eq!(item["state"], "answered");
    assert_eq!(item["answer"], "deny");
}

#[test]
fn request_saved_first_wins_over_a_late_inbox_cancel() {
    let r = rig("inbox-late-request", None);
    let (asker, event) = r.start_request();
    let id = request_item_id(&event);
    r.s.ops
        .call(
            &app(),
            ops::VAULT_SAVE,
            &json!({"entry": {"name": "npm", "kind": "token"}, "value": {"secret": "fake-npm-1"},
                    "request": event["id"]}),
        )
        .unwrap();
    let late = r.answer(&id, "cancel");
    assert_eq!(late["answered"], false);
    assert!(asker.join().unwrap().is_ok());
    let item = r.item(&id);
    assert_eq!(item["state"], "answered");
    assert_eq!(item["answer"], "open");
}

#[test]
fn opening_the_form_through_the_inbox_changes_nothing() {
    let r = rig("inbox-open", None);
    let (asker, event) = r.start_request();
    let id = request_item_id(&event);
    let out = r.answer(&id, "open");
    assert_eq!(out["answered"], false);
    assert_eq!(r.item(&id)["state"], "open");
    r.answer(&id, "cancel");
    assert!(asker.join().unwrap().is_err());
}

#[test]
fn the_item_exists_before_the_pending_event_is_published() {
    let r = rig("inbox-order", None);
    seed(&r.s, true);
    let (user, event) = pending_use(&r.s);
    let id = approval_item_id(&event);
    assert_eq!(
        r.list().iter().filter(|i| i["id"] == id.as_str()).count(),
        1
    );
    r.answer(&id, "deny");
    let _ = user.join().unwrap();
}

#[test]
fn a_clock_past_expiry_never_expires_a_wait_still_open() {
    let tmp = TempDir::new("inbox-clock");
    let inbox_room = Arc::new(InboxRoom::default());
    let later = Arc::new(std::sync::atomic::AtomicI64::new(0));
    let clock = later.clone();
    let inbox = InboxOps::with_clock(
        tmp.path("inbox"),
        inbox_room.clone(),
        Arc::new(move || clock.load(Ordering::SeqCst)),
    );
    let r = Rig {
        s: setup_with("inbox-clock", true, Some(inbox.clone()), None),
        _tmp: tmp,
        inbox,
        inbox_room,
    };
    seed(&r.s, true);
    let (user, event) = pending_use(&r.s);
    let id = approval_item_id(&event);
    let expires = r.item(&id)["expires_ms"].as_i64().unwrap();
    later.store(expires + 60_000, Ordering::SeqCst);
    r.inbox.tick(expires + 60_000);
    assert_eq!(r.item(&id)["state"], "open");
    r.s.ops
        .call(
            &app(),
            ops::VAULT_APPROVAL_ANSWER,
            &json!({"id": event["id"], "decision": "once"}),
        )
        .unwrap();
    assert!(user.join().unwrap().is_ok());
    let item = r.item(&id);
    assert_eq!(item["state"], "answered");
    assert_eq!(item["answer"], "allow_once");
    assert_eq!(item["answered_by"], "mac");
    assert_eq!(item["closed_ms"], expires + 60_000);
}
