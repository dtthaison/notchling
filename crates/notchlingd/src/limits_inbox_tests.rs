use crate::acting_ops_tests::FakeRoom;
use crate::inbox_ops::InboxOps;
use crate::limits_ops_tests::{rig, Claude, NoJobs, NOW};
use crate::op_group::OpGroup;
use notchling_engine::limits::mode::{LimitMode, CLAUDE};
use serde_json::{json, Value};
use std::sync::Arc;

fn inbox_app() -> notchling_local_api::Caller {
    notchling_local_api::Caller {
        tier: notchling_local_api::Tier::App,
        client: notchling_local_api::ClientKind::App,
        peer: None,
        pid: None,
    }
}

fn inbox_items(inbox: &crate::inbox_ops::InboxOps) -> Vec<Value> {
    inbox.flush();
    let list = inbox
        .call(
            &inbox_app(),
            notchling_local_api::ops::INBOX_LIST,
            &json!({}),
        )
        .unwrap();
    list["items"].as_array().unwrap().clone()
}

#[test]
fn limit_notice_raises_done_item() {
    let (tmp, flag, _room, ops) = rig();
    let inbox = InboxOps::new(tmp.path("inbox"), Arc::new(FakeRoom::default()));
    let ops = ops.with_inbox(inbox.clone());
    let claude = Arc::new(Claude::default());
    *claude.me.lock().unwrap() = 92.0;
    ops.config().set_mode(&CLAUDE, LimitMode::Notify).unwrap();
    ops.set_claude(claude.clone(), Arc::new(NoJobs));
    flag.set(true);
    assert!(ops.tick_at(NOW));
    let items = inbox_items(&inbox);
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item["kind"], "limit_notice");
    assert!(item["title"]
        .as_str()
        .unwrap()
        .contains("at 92% of its 5-hour limit"));
    assert_eq!(item["state"], "open");
    assert_eq!(
        item["actions"],
        json!([{"id": "done", "label": "Done", "primary": false}])
    );
    let id = item["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("limit:"));
    let got = inbox
        .call(
            &inbox_app(),
            notchling_local_api::ops::INBOX_ANSWER,
            &json!({"id": id, "action": "done"}),
        )
        .unwrap();
    assert_eq!(got["answered"], true);
}

#[test]
fn two_limit_notices_are_two_items() {
    let (tmp, _flag, _room, ops) = rig();
    let inbox = InboxOps::new(tmp.path("inbox"), Arc::new(FakeRoom::default()));
    let ops = ops.with_inbox(inbox.clone());
    let say = ops.announcer();
    say("one");
    say("one");
    assert_eq!(inbox_items(&inbox).len(), 2);
}

#[test]
fn announcers_sharing_a_millisecond_raise_distinct_items() {
    let (tmp, _flag, _room, ops) = rig();
    let inbox = InboxOps::new(tmp.path("inbox"), Arc::new(FakeRoom::default()));
    let ops = ops.with_inbox(inbox.clone());
    let (claude, codex) = (ops.announcer(), ops.announcer());
    claude("Claude: at 92%");
    codex("Codex: at 92%");
    let items = inbox_items(&inbox);
    assert_eq!(items.len(), 2);
    assert_ne!(items[0]["id"], items[1]["id"]);
    assert!(items.iter().all(|i| i["created_ms"] == (NOW as i64) * 1000));
}

#[test]
fn a_long_limit_notice_is_cut_with_an_ellipsis() {
    let (tmp, _flag, _room, ops) = rig();
    let inbox = InboxOps::new(tmp.path("inbox"), Arc::new(FakeRoom::default()));
    let ops = ops.with_inbox(inbox.clone());
    ops.announcer()(&"x".repeat(300));
    let title = inbox_items(&inbox)[0]["title"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(title.chars().count(), 120);
    assert!(title.ends_with('…'));
}
