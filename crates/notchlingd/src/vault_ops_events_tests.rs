use super::tests::{agent, app, pending_use, seed, setup, setup_with, Setup};
use super::*;
use crate::acting_ops_tests::FakeRoom as InboxRoom;
use crate::event_assert::{of, results, text};
use crate::inbox_ops::InboxOps;
use crate::limits_ops_tests::TempDir;
use notchling_engine::inbox::ActionId as Action;
use notchling_events::envelope::{Actor, ActorTier};
use notchling_events::scope::Scope;
use notchling_events::{Device, Ref};
use std::sync::atomic::Ordering;

const TOKEN: &str = "fake-token-value-0001";

fn use_gh() -> Value {
    json!({"names": ["gh"], "folder": "/w/app", "command": "FOO=1 /usr/bin/curl \"$GH_URL\""})
}

fn phone_scope(device: &str) -> Scope {
    Scope {
        op_id: None,
        actor: Some(Actor {
            tier: ActorTier::Phone,
            via: None,
            exe: None,
            device: Some(Ref::<Device>::new(device)),
            session: None,
        }),
    }
}

fn inbox_setup(tag: &str) -> (TempDir, Setup, Arc<InboxOps>) {
    let tmp = TempDir::new(tag);
    let inbox = InboxOps::new(tmp.path("inbox"), Arc::new(InboxRoom::default()));
    let s = setup_with(tag, true, Some(inbox.clone()), None);
    (tmp, s, inbox)
}

#[test]
fn secret_not_released_when_commit_fails() {
    let s = setup("ev-commit-fails", true);
    seed(&s, false);
    s.sink.fail_commits(true);
    let e = s.ops.call(&agent(), ops::VAULT_USE, &use_gh()).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "the audit log can't be written");
    assert!(!e.message.contains(TOKEN));
    assert!(of(&s.sink, "vault.secret_used").is_empty());
}

#[test]
fn a_use_commits_before_the_values_leave() {
    let s = setup("ev-commit-first", true);
    seed(&s, false);
    let reply = s.ops.call(&agent(), ops::VAULT_USE, &use_gh()).unwrap();
    assert_eq!(reply["env"]["GH_TOKEN"], TOKEN);
    assert_eq!(s.sink.commits(), 1);
    let used = of(&s.sink, "vault.secret_used");
    assert_eq!(used.len(), 1);
    assert_eq!(text(&used[0], "command_exe").as_deref(), Some("curl"));
    assert_eq!(text(&used[0], "folder").as_deref(), Some("/w/app"));
    assert!(!format!("{:?}", s.sink.events()).contains(TOKEN));
}

#[test]
fn always_allow_is_answered_by_none() {
    let s = setup("ev-always", true);
    seed(&s, false);
    s.ops.call(&agent(), ops::VAULT_USE, &use_gh()).unwrap();
    let used = of(&s.sink, "vault.secret_used");
    assert_eq!(text(&used[0], "approval").as_deref(), Some("always_allow"));
    assert_eq!(text(&used[0], "answered_by").as_deref(), Some("none"));
}

#[test]
fn the_macs_answer_is_recorded_as_asked_and_mac() {
    let s = setup("ev-mac", true);
    seed(&s, true);
    let (user, event) = pending_use(&s);
    s.ops
        .call(
            &app(),
            ops::VAULT_APPROVAL_ANSWER,
            &json!({"id": event["id"], "decision": "once"}),
        )
        .unwrap();
    user.join().unwrap().unwrap();
    let used = of(&s.sink, "vault.secret_used");
    assert_eq!(text(&used[0], "approval").as_deref(), Some("asked"));
    assert_eq!(text(&used[0], "answered_by").as_deref(), Some("mac"));
    assert_eq!(text(&used[0], "result").as_deref(), Some("given"));
    assert!(text(&used[0], "waited_ms").is_some());
}

#[test]
fn answered_by_phone_carries_device() {
    let (_tmp, s, inbox) = inbox_setup("ev-phone");
    seed(&s, true);
    let (user, event) = pending_use(&s);
    let id = format!("vault-approval:{}", event["id"].as_str().unwrap());
    phone_scope("d1").run(|| inbox.decide(&id, Action::AllowOnce, "phone").unwrap());
    user.join().unwrap().unwrap();
    let used = of(&s.sink, "vault.secret_used");
    assert_eq!(text(&used[0], "answered_by").as_deref(), Some("phone"));
    assert_eq!(text(&used[0], "device").as_deref(), Some("d1"));
}

#[test]
fn a_denial_is_pushed_with_who_denied() {
    let (_tmp, s, inbox) = inbox_setup("ev-denied");
    seed(&s, true);
    let (user, event) = pending_use(&s);
    let id = format!("vault-approval:{}", event["id"].as_str().unwrap());
    phone_scope("d2").run(|| inbox.decide(&id, Action::Deny, "phone").unwrap());
    user.join().unwrap().unwrap_err();
    let used = of(&s.sink, "vault.secret_used");
    assert_eq!(results(&s.sink, "vault.secret_used"), ["denied"]);
    assert_eq!(text(&used[0], "answered_by").as_deref(), Some("phone"));
    assert_eq!(s.sink.commits(), 0);
}

#[test]
fn an_unanswered_approval_expires() {
    let timing = VaultTiming {
        approval_wait: Duration::from_millis(60),
        request_wait: Duration::from_millis(60),
        slice: Duration::from_millis(5),
    };
    let s = setup_with("ev-expired", true, None, Some(timing));
    seed(&s, true);
    let (user, _) = pending_use(&s);
    user.join().unwrap().unwrap_err();
    let used = of(&s.sink, "vault.secret_used");
    assert_eq!(results(&s.sink, "vault.secret_used"), ["expired"]);
    assert_eq!(text(&used[0], "answered_by").as_deref(), Some("none"));
}

#[test]
fn request_saved_commits() {
    let s = setup("ev-request", true);
    s.room.follows.store(true, Ordering::SeqCst);
    let ops = s.ops.clone();
    let asker = std::thread::spawn(move || {
        ops.call(
            &agent(),
            ops::VAULT_REQUEST,
            &json!({"name": "npm", "reason": "to publish", "folder": "/w/app"}),
        )
    });
    let event = loop {
        if let Some(e) = s.room.pending(topics::VAULT_REQUEST_PENDING) {
            break e;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    s.ops
        .call(
            &app(),
            ops::VAULT_SAVE,
            &json!({"entry": event["entry"], "value": {"secret": "fake-npm-0003"}, "request": event["id"]}),
        )
        .unwrap();
    asker.join().unwrap().unwrap();
    let asked = of(&s.sink, "vault.secret_requested");
    assert_eq!(asked.len(), 1);
    assert_eq!(text(&asked[0], "result").as_deref(), Some("saved"));
    assert_eq!(text(&asked[0], "name").as_deref(), Some("npm"));
    assert_eq!(text(&asked[0], "answered_by").as_deref(), Some("mac"));
    assert_eq!(text(&asked[0], "folder").as_deref(), Some("/w/app"));
    assert_eq!(s.sink.commits(), 1);
    assert!(!format!("{:?}", s.sink.events()).contains("fake-npm-0003"));
}

#[test]
fn a_cancelled_request_is_pushed() {
    let s = setup("ev-request-cancel", true);
    s.room.follows.store(true, Ordering::SeqCst);
    let ops = s.ops.clone();
    let asker =
        std::thread::spawn(move || ops.call(&agent(), ops::VAULT_REQUEST, &json!({"name": "npm"})));
    let event = loop {
        if let Some(e) = s.room.pending(topics::VAULT_REQUEST_PENDING) {
            break e;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    s.ops
        .call(
            &app(),
            ops::VAULT_REQUEST_CANCEL,
            &json!({"id": event["id"]}),
        )
        .unwrap();
    asker.join().unwrap().unwrap_err();
    assert_eq!(results(&s.sink, "vault.secret_requested"), ["cancelled"]);
    assert_eq!(s.sink.commits(), 0);
}

#[test]
fn entry_changed_on_save_and_remove() {
    let s = setup("ev-entry", true);
    let entry = json!({"name": "gh", "kind": "token", "url": "https://api.github.com"});
    let save = |entry: &Value, value: Option<Value>| {
        let mut args = json!({"entry": entry});
        if let Some(value) = value {
            args["value"] = value;
        }
        s.ops.call(&app(), ops::VAULT_SAVE, &args).unwrap();
    };
    let actions = || {
        of(&s.sink, "vault.entry_changed")
            .iter()
            .filter_map(|e| text(e, "action"))
            .collect::<Vec<_>>()
    };
    save(&entry, Some(json!({"secret": TOKEN})));
    let mut asking = entry.clone();
    asking["ask"] = json!(true);
    s.ops
        .call(
            &app(),
            ops::VAULT_SAVE,
            &json!({"entry": asking, "replacing": "gh"}),
        )
        .unwrap();
    s.ops
        .call(
            &app(),
            ops::VAULT_SAVE,
            &json!({"entry": entry, "replacing": "gh"}),
        )
        .unwrap();
    s.ops
        .call(&app(), ops::VAULT_REMOVE, &json!({"name": "gh"}))
        .unwrap();
    assert_eq!(
        actions(),
        ["saved", "ask_first_on", "ask_first_off", "removed"]
    );
    assert!(!format!("{:?}", s.sink.events()).contains(TOKEN));
}

#[test]
fn always_allow_pushes_ask_first_off() {
    let (_tmp, s, inbox) = inbox_setup("ev-always-entry");
    seed(&s, true);
    let (user, event) = pending_use(&s);
    let id = format!("vault-approval:{}", event["id"].as_str().unwrap());
    inbox.decide(&id, Action::AllowAlways, "mac").unwrap();
    user.join().unwrap().unwrap();
    let last = of(&s.sink, "vault.entry_changed")
        .iter()
        .filter_map(|e| text(e, "action"))
        .next_back();
    assert_eq!(last.as_deref(), Some("ask_first_off"));
}
