use super::tests::{agent, app, ask, ended, pending_use, seed, setup_with, until, Setup};
use super::*;
use crate::acting_ops_tests::FakeRoom as InboxRoom;
use crate::event_assert::results;
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

fn short(approval: u64) -> Option<VaultTiming> {
    Some(VaultTiming {
        approval_wait: Duration::from_millis(approval),
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

    fn start_request(&self) -> Value {
        ask(&self.s, json!({"name": "npm", "reason": "to publish"})).1
    }

    fn closed_item(&self, id: &str) -> Value {
        until(|| Some(self.item(id)).filter(|item| item["state"] != "open"))
    }

    fn request_item(&self, event: &Value) -> Value {
        self.inbox.flush();
        crate::consent_testkit::item_for(&self.inbox, event["id"].as_str().unwrap())
    }

    fn request_item_id(&self, event: &Value) -> String {
        self.request_item(event)["id"].as_str().unwrap().to_string()
    }
}

fn approval_item_id(event: &Value) -> String {
    format!("vault-approval:{}", event["id"].as_str().unwrap())
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
    let r = rig("inbox-timeout", short(120));
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
fn no_follower_refuses_a_use_at_once_but_a_request_is_saved() {
    let r = rig("inbox-nofollow", None);
    seed(&r.s, true);
    let e =
        r.s.ops
            .call(&agent(), ops::VAULT_USE, &json!({"names": ["gh"]}))
            .unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsApproval);
    assert!(r.list().is_empty());
    let reply =
        r.s.ops
            .call(&agent(), ops::VAULT_REQUEST, &json!({"name": "npm"}))
            .unwrap();
    assert_eq!(reply["state"], "waiting");
    assert_eq!(r.list().len(), 1);
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
    let event = r.start_request();
    let item = r.request_item(&event);
    let id = item["id"].as_str().unwrap().to_string();
    assert_eq!(item["kind"], "secret_request");
    assert_eq!(item["title"], "A program asks you to add the secret npm");
    assert_eq!(
        item["detail"],
        "Open the form on your Mac to add it. Its value never reaches the agent."
    );
    assert!(
        !item.to_string().contains("to publish"),
        "the reason stays off the item"
    );
    assert_eq!(
        item["open"],
        json!({"consent": event["id"], "kind": "secret_request", "vault_request": event["id"]})
    );
    assert_eq!(
        item["actions"],
        json!([
            {"id": "deny", "label": "Deny", "primary": false},
            {"id": "open", "label": "Open on Mac", "primary": true},
        ])
    );
    r.s.ops
        .call(
            &app(),
            ops::VAULT_SAVE,
            &json!({"entry": {"name": "npm", "kind": "token"}, "value": {"secret": "fake-npm-1"},
                    "consentId": event["id"]}),
        )
        .unwrap();
    assert_eq!(
        ended(&r.s, event["id"].as_str().unwrap())["state"],
        "applied"
    );
    let item = r.closed_item(&id);
    assert_eq!(item["state"], "answered");
    assert_eq!(item["answer"], "allow");
}

#[test]
fn request_deny_from_inbox_denies_request() {
    let r = rig("inbox-cancel", None);
    let event = r.start_request();
    let id = r.request_item_id(&event);
    let got = r.answer(&id, "deny");
    assert_eq!(got["answered"], true);
    let done = ended(&r.s, event["id"].as_str().unwrap());
    assert_eq!(done["state"], "denied");
    let item = r.closed_item(&id);
    assert_eq!(item["state"], "answered");
    assert_eq!(item["answer"], "deny");
    until(|| (!results(&r.s.sink, "vault.secret_requested").is_empty()).then_some(()));
    assert_eq!(results(&r.s.sink, "vault.secret_requested"), ["cancelled"]);
}

#[test]
fn request_cancel_from_the_form_reads_as_denied_by_the_mac() {
    let r = rig("inbox-cancel-window", None);
    let event = r.start_request();
    let id = r.request_item_id(&event);
    r.s.ops
        .call(
            &app(),
            ops::VAULT_REQUEST_CANCEL,
            &json!({"id": event["id"]}),
        )
        .unwrap();
    assert_eq!(
        ended(&r.s, event["id"].as_str().unwrap())["state"],
        "denied"
    );
    let item = r.closed_item(&id);
    assert_eq!(item["state"], "answered");
    assert_eq!(item["answer"], "deny");
    assert_eq!(item["answered_by"], "mac");
}

#[test]
fn an_op_thread_that_unwinds_after_the_raise_withdraws_its_item() {
    let r = rig("inbox-unwind", None);
    seed(&r.s, true);
    r.s.room.follows.store(true, Ordering::SeqCst);
    let room = r.s.room.clone();
    let _ = std::thread::spawn(move || {
        let _held = room.events.lock().unwrap();
        panic!("poison the room so the next publish unwinds");
    })
    .join();
    let ops = r.s.ops.clone();
    let user = std::thread::spawn(move || {
        let args = json!({"names": ["gh"], "folder": "/w/app", "command": "curl"});
        ops.call(&agent(), ops::VAULT_USE, &args)
    });
    assert!(user.join().is_err());
    let items = r.list();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "secret_approval");
    assert_eq!(items[0]["state"], "withdrawn");
}

#[test]
fn request_timeout_expires_the_item() {
    let r = rig("inbox-request-timeout", short(10_000));
    let (reply, event) = ask(
        &r.s,
        json!({"name": "npm", "consent": {"expiresInMs": 60_000}}),
    );
    let id = r.request_item_id(&event);
    r.s.advance(61_000);
    assert_eq!(
        ended(&r.s, reply["request"]["id"].as_str().unwrap())["state"],
        "expired"
    );
    assert_eq!(r.item(&id)["state"], "expired");
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
    assert_eq!(changed[0]["item"]["blocking"], true);
    assert_eq!(changed[1]["item"]["state"], "answered");
    assert!(changed[1]["item"]["blocking"].is_null());
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
fn request_saved_first_wins_over_a_late_inbox_deny() {
    let r = rig("inbox-late-request", None);
    let event = r.start_request();
    let id = r.request_item_id(&event);
    r.s.ops
        .call(
            &app(),
            ops::VAULT_SAVE,
            &json!({"entry": {"name": "npm", "kind": "token"}, "value": {"secret": "fake-npm-1"},
                    "consentId": event["id"]}),
        )
        .unwrap();
    ended(&r.s, event["id"].as_str().unwrap());
    r.closed_item(&id);
    let late = r.answer(&id, "deny");
    assert_eq!(late["answered"], false);
    let item = r.item(&id);
    assert_eq!(item["state"], "answered");
    assert_eq!(item["answer"], "allow");
}

#[test]
fn opening_the_form_through_the_inbox_changes_nothing() {
    let r = rig("inbox-open", None);
    let event = r.start_request();
    let id = r.request_item_id(&event);
    let out = r.answer(&id, "open");
    assert_eq!(out["answered"], false);
    assert_eq!(r.item(&id)["state"], "open");
    r.answer(&id, "deny");
    assert_eq!(
        ended(&r.s, event["id"].as_str().unwrap())["state"],
        "denied"
    );
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

#[test]
fn an_approval_names_the_session_and_the_worker_it_comes_from() {
    let r = rig("inbox-place", None);
    let grant = ReleaseFor {
        node_name: "worker".to_string(),
        job: "j-1".to_string(),
        command: "make".to_string(),
        folder: "/w/app".to_string(),
    };
    let held =
        r.s.ops
            .raise_approval("t-grant", &["gh"], ("make", "/w/app"), Some(&grant), None);
    let local = r.s.ops.raise_approval(
        "t-local",
        &["gh"],
        ("make", "/w/app"),
        None,
        Some("sess-1".to_string()),
    );
    let bare =
        r.s.ops
            .raise_approval("t-bare", &["gh"], ("make", "/w/app"), None, None);
    r.inbox.flush();
    assert_eq!(
        r.item("vault-approval:t-grant")["place"],
        json!({"machine_name": "worker"})
    );
    assert_eq!(
        r.item("vault-approval:t-local")["place"],
        json!({"session": "sess-1"})
    );
    assert!(r.item("vault-approval:t-bare").get("place").is_none());
    drop((held, local, bare));
}

#[test]
fn a_waiting_approval_is_blocking_for_the_vault_and_cancelling_clears_it() {
    let r = rig("inbox-blocking", None);
    seed(&r.s, true);
    let (user, event) = pending_use(&r.s);
    let id = approval_item_id(&event);
    let item = r.item(&id);
    assert_eq!(item["blocking"], true);
    assert_eq!(item["waiter"], "vault");
    assert_eq!(item["waiting_since"], item["created_ms"]);
    r.s.switch.set(false);
    let _ = user.join();
    let closed = r.closed_item(&id);
    assert!(closed["blocking"].is_null());
    assert!(closed["waiter"].is_null());
    assert!(closed["waiting_since"].is_null());
}

#[test]
fn an_answered_approval_keeps_no_live_mark() {
    let r = rig("inbox-blocking-answered", None);
    seed(&r.s, true);
    let (user, event) = pending_use(&r.s);
    let id = approval_item_id(&event);
    assert_eq!(r.item(&id)["blocking"], true);
    r.answer(&id, "deny");
    let _ = user.join();
    let closed = r.closed_item(&id);
    assert!(closed["blocking"].is_null());
}

#[test]
fn a_raised_approval_is_blocking_in_its_first_event() {
    let r = rig("inbox-first-event", None);
    seed(&r.s, true);
    let (user, event) = pending_use(&r.s);
    r.inbox.flush();
    let first = r.inbox_room.named("inbox.changed");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0]["item"]["blocking"], true);
    assert_eq!(first[0]["item"]["waiter"], "vault");
    r.answer(&approval_item_id(&event), "deny");
    let _ = user.join();
}

#[test]
fn a_finished_approval_leaves_no_presence_behind() {
    let r = rig("inbox-presence-left", None);
    seed(&r.s, true);
    let (user, event) = pending_use(&r.s);
    let id = approval_item_id(&event);
    assert!(r.s.ops.presence.present(&id, crate::acting_ops::now_ms()));
    r.answer(&id, "deny");
    let _ = user.join();
    assert!(!r.s.ops.presence.present(&id, crate::acting_ops::now_ms()));
}

#[test]
fn a_window_deny_with_a_note_puts_the_note_on_the_item_and_in_the_refusal() {
    let r = rig("inbox-window-note", None);
    seed(&r.s, true);
    let (user, event) = pending_use(&r.s);
    r.s.ops
        .call(
            &app(),
            ops::VAULT_APPROVAL_ANSWER,
            &json!({"id": event["id"], "decision": "deny", "note": "use the staging token"}),
        )
        .unwrap();
    let refused = user.join().unwrap().unwrap_err();
    assert!(
        refused.message.contains("use the staging token"),
        "{}",
        refused.message
    );
    let item = r.item(&approval_item_id(&event));
    assert_eq!(item["state"], "answered");
    assert_eq!(item["note"], "use the staging token");
}
