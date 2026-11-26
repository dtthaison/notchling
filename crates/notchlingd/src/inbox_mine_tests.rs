use crate::inbox_ops_tests::{item, rig};
use notchling_engine::inbox::Source;
use notchling_local_api::topics;
use serde_json::json;

fn owned(id: &str, session: Option<&str>) -> notchling_engine::inbox::Item {
    let mut it = item(id);
    it.source = Source {
        node: None,
        session: session.map(str::to_owned),
        job: None,
    };
    it
}

#[test]
fn a_session_hears_its_own_items_on_mine_inbox_and_nobody_else_does() {
    let r = rig();
    r.ops.raise(owned("a", Some("s-a")), None);
    r.ops.raise(owned("b", None), None);
    r.ops.flush();
    let scoped = r.room.scoped.lock().unwrap().clone();
    assert_eq!(scoped.len(), 1, "{scoped:?}");
    let (owner, topic, event, data) = &scoped[0];
    assert_eq!(owner, "s-a");
    assert_eq!(topic, topics::MINE_INBOX);
    assert_eq!(event, "inbox.changed");
    assert_eq!(data["item"]["id"], json!("a"));
    assert_eq!(data["state"], json!("open"));
    assert!(data["item"].get("source").is_none());
}

#[test]
fn a_mirrored_item_never_reaches_a_local_sessions_mine_inbox() {
    let r = rig();
    r.ops
        .mirror_changed(
            &crate::cluster_inbox_testkit::node(7),
            owned("m", Some("s-a")),
        )
        .unwrap();
    r.ops.flush();
    assert!(!r.room.named("inbox.changed").is_empty());
    assert!(r.room.scoped.lock().unwrap().is_empty());
}

#[test]
fn a_live_change_of_an_own_item_reaches_its_session() {
    use notchling_engine::inbox::{Live, Waiter};
    let r = rig();
    r.ops.raise(owned("a", Some("s-a")), None);
    r.ops.flush();
    let before = r.room.scoped.lock().unwrap().len();
    assert!(r.ops.set_live(
        "a",
        Some(Live {
            waiter: Waiter::Cli,
            waiting_since: 1,
        })
    ));
    r.ops.flush();
    let scoped = r.room.scoped.lock().unwrap().clone();
    assert_eq!(scoped.len(), before + 1);
    assert_eq!(scoped.last().unwrap().3["item"]["blocking"], json!(true));
}

fn heard_by(
    r: &crate::inbox_ops_tests::Rig,
    session: &str,
    wire: &[&str],
) -> Vec<serde_json::Value> {
    r.room
        .scoped
        .lock()
        .unwrap()
        .iter()
        .filter(|(owner, topic, _, _)| owner == session && wire.contains(&topic.as_str()))
        .map(|(_, _, _, data)| data.clone())
        .collect()
}

fn wire_of(topic: &str) -> Vec<&'static str> {
    let parsed = crate::watch_cli::parse(&["--topic".to_owned(), topic.to_owned()]).unwrap();
    crate::watch_cli::wire_topics(&parsed.topics, false)
}

#[test]
fn an_agent_following_inbox_hears_its_own_item_change() {
    let r = rig();
    r.ops.raise(owned("a", Some("s-a")), None);
    r.ops.flush();
    let heard = heard_by(&r, "s-a", &wire_of("inbox"));
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert_eq!(heard[0]["item"]["id"], json!("a"));
    assert_eq!(heard[0]["state"], json!("open"));
}

#[test]
fn an_agent_following_inbox_never_hears_another_session_or_a_mirrored_item() {
    let r = rig();
    r.ops.raise(owned("a", Some("s-a")), None);
    r.ops.raise(owned("b", Some("s-b")), None);
    r.ops.raise(owned("c", None), None);
    r.ops
        .mirror_changed(
            &crate::cluster_inbox_testkit::node(7),
            owned("m", Some("s-a")),
        )
        .unwrap();
    r.ops.flush();
    let heard = heard_by(&r, "s-a", &wire_of("inbox"));
    let ids: Vec<_> = heard.iter().map(|d| d["item"]["id"].clone()).collect();
    assert_eq!(ids, vec![json!("a")], "{heard:?}");
    assert!(!wire_of("inbox").contains(&topics::INBOX));
}

#[test]
fn the_app_inbox_topic_still_gets_every_item() {
    let r = rig();
    r.ops.raise(owned("a", Some("s-a")), None);
    r.ops.raise(owned("c", None), None);
    r.ops
        .mirror_changed(
            &crate::cluster_inbox_testkit::node(7),
            owned("m", Some("s-a")),
        )
        .unwrap();
    r.ops.flush();
    let ids: Vec<_> = r
        .room
        .named("inbox.changed")
        .iter()
        .map(|d| d["item"]["id"].clone())
        .collect();
    assert!(
        ids.contains(&json!("a")) && ids.contains(&json!("c")),
        "{ids:?}"
    );
    assert!(
        ids.iter()
            .any(|id| id.as_str().is_some_and(|s| s.ends_with("/m"))),
        "{ids:?}"
    );
}
