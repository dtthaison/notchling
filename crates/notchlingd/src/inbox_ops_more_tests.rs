use crate::acting_ops_tests::FakeRoom;
use crate::inbox_ops::InboxOps;
use crate::inbox_ops_tests::{answer, app, caller, changed, item, rig, Calls, NOW};
use crate::op_group::OpGroup;
use notchling_engine::inbox::{self, ActionId, State};
use notchling_local_api::{ops, ClientKind, ErrorCode, Tier};
use serde_json::json;
use std::sync::Arc;

#[test]
fn the_inbox_answers_only_the_app_itself() {
    let r = rig();
    r.ops.raise(item("a"), None);
    let cli = caller(Tier::App, ClientKind::Cli);
    for op in [ops::INBOX_LIST, ops::INBOX_ANSWER, ops::INBOX_SEEN] {
        let args = json!({"id": "a", "action": "deny", "ids": ["a"]});
        let e = r.ops.call(&cli, op, &args).unwrap_err();
        assert_eq!(e.code, ErrorCode::Forbidden, "{op}");
    }
    assert_eq!(r.ops.items()[0].state, State::Open);
    assert!(r.ops.call(&app(), ops::INBOX_LIST, &json!({})).is_ok());
}

#[test]
fn only_the_mac_or_a_phone_may_allow_a_secret() {
    let r = rig();
    r.ops.raise(item("a"), Some(Arc::new(Calls::default())));
    r.ops.raise(item("b"), Some(Arc::new(Calls::default())));
    let e = r.ops.decide("a", ActionId::Allow, "agent").unwrap_err();
    assert_eq!(e.code, ErrorCode::Forbidden);
    assert_eq!(r.ops.items()[1].state, State::Open);
    let denied = r.ops.decide("a", ActionId::Deny, "agent").unwrap();
    assert!(denied.answered);
    let allowed = r.ops.decide("b", ActionId::Allow, "phone").unwrap();
    assert!(allowed.answered);
    assert_eq!(allowed.answer, Some(ActionId::Allow));
}

#[test]
fn load_replaces_the_book_with_the_saved_one() {
    let r = rig();
    r.ops.raise(item("a"), None);
    r.ops.flush();
    let later = NOW + 5_000;
    let room = Arc::new(FakeRoom::default());
    let reopened = InboxOps::with_clock(r.dir.clone(), room.clone(), Arc::new(move || later));
    let list = || reopened.call(&app(), ops::INBOX_LIST, &json!({})).unwrap();
    assert_eq!(list()["items"], json!([]));
    reopened.load();
    assert_eq!(list()["items"][0]["id"], "a");
    assert_eq!(list()["items"][0]["state"], "withdrawn");
    reopened.close();
    let saved: Vec<String> = changed(&room)
        .iter()
        .map(|e| e["item"]["id"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(saved, ["a"]);
}

#[test]
fn a_restart_keeps_the_order_of_items_raised_together() {
    let r = rig();
    for n in 0..=inbox::MAX_DONE {
        let id = r.ops.raise(item(&format!("i{n:03}")), None);
        r.ops.resolve(&id, State::Withdrawn, None, None);
    }
    r.ops.flush();
    let before: Vec<String> = r.ops.items().into_iter().map(|i| i.id).collect();
    let reopened = InboxOps::with_clock(
        r.dir.clone(),
        Arc::new(FakeRoom::default()),
        Arc::new(|| NOW + 5_000),
    );
    reopened.load();
    let after: Vec<String> = reopened.items().into_iter().map(|i| i.id).collect();
    assert_eq!(after, before);
    reopened.tick(NOW + 6_000);
    let kept: Vec<String> = reopened.items().into_iter().map(|i| i.id).collect();
    assert_eq!(kept.len(), inbox::MAX_DONE);
    assert!(!kept.contains(&"i000".to_string()));
    assert!(kept.contains(&format!("i{:03}", inbox::MAX_DONE)));
    reopened.close();
}

#[test]
fn tick_never_expires_an_item_whose_producer_still_waits() {
    let r = rig();
    let calls = Arc::new(Calls::default());
    let mut held = item("held");
    held.expires_ms = Some(NOW + 1_000);
    r.ops.raise(held, Some(calls.clone()));
    let mut loose = item("loose");
    loose.expires_ms = Some(NOW + 1_000);
    r.ops.raise(loose, None);
    r.ops.tick(NOW + 60_000);
    r.ops.flush();
    let state = |id: &str| {
        r.ops
            .items()
            .into_iter()
            .find(|i| i.id == id)
            .map(|i| i.state)
    };
    assert_eq!(state("held"), Some(State::Open));
    assert_eq!(state("loose"), Some(State::Expired));
    let out = answer(&r.ops, "held", "deny");
    assert_eq!(out["answered"], true);
    assert_eq!(state("held"), Some(State::Answered));
    assert_eq!(
        calls.0.lock().unwrap().clone(),
        [("held".to_string(), ActionId::Deny)]
    );
}

#[test]
fn the_producer_expires_its_own_item() {
    let r = rig();
    let mut held = item("held");
    held.expires_ms = Some(NOW + 1_000);
    r.ops.raise(held, Some(Arc::new(Calls::default())));
    r.ops.tick(NOW + 60_000);
    r.ops.resolve("held", State::Expired, None, None);
    r.ops.flush();
    let last = changed(&r.room).last().cloned().unwrap();
    assert_eq!(last["item"]["state"], "expired");
}

#[test]
fn a_producer_answer_records_who_answered() {
    let r = rig();
    r.ops.raise(item("a"), Some(Arc::new(Calls::default())));
    r.ops.resolve(
        "a",
        State::Answered,
        Some(ActionId::AllowAlways),
        Some("mac"),
    );
    r.ops.flush();
    let last = changed(&r.room).last().cloned().unwrap();
    assert_eq!(last["item"]["answer"], "allow_always");
    assert_eq!(last["item"]["answered_by"], "mac");
}

fn in_cluster(cluster_dir: &std::path::Path) {
    use notchling_engine::cluster::grants::SecretAllow;
    use notchling_engine::cluster::id::fingerprint;
    use notchling_engine::cluster::state::{start, Command};
    use notchling_engine::cluster::{Desired, Member, MemberState, NodeId};
    let fp = fingerprint(b"m5");
    let home = Member {
        id: NodeId::from_fingerprint(&fp).unwrap(),
        name: "m5".to_string(),
        fingerprint: fp,
        addrs: vec!["100.64.0.2".to_string()],
        port: 47823,
        client: true,
        can_be_master: true,
        reference: false,
        accepts_delegation: false,
        headless: false,
        state: MemberState::Active,
        versions: Some(crate::cluster_versions::own()),
        desired: Desired::default(),
        joined_ms: 0,
        secrets: SecretAllow::default(),
    };
    let (_, entry, changes) = start(
        Command::Create {
            id: "c1".to_string(),
            name: "home".to_string(),
            home,
        },
        1,
    )
    .unwrap();
    notchling_engine::cluster::persist::save(cluster_dir, &entry, &changes).unwrap();
}

fn stored(db: &std::path::Path) -> Vec<String> {
    let Ok(db) = rusqlite::Connection::open(db) else {
        return Vec::new();
    };
    let Ok(mut query) = db.prepare("SELECT id FROM inbox ORDER BY rowid") else {
        return Vec::new();
    };
    let rows = query
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    rows
}

#[test]
fn an_inbox_already_running_follows_the_node_into_a_cluster() {
    let r = rig();
    let cluster_dir = r._tmp.path("cluster");
    let ops = r.ops.clone().with_cluster_dir(cluster_dir.clone());
    let node_db = r.dir.join("notchling.db");
    let cluster_db = cluster_dir.join("cluster.db");
    let calls = Arc::new(Calls::default());
    ops.raise(item("a"), Some(calls.clone()));
    ops.raise(item("b"), None);
    ops.flush();
    assert_eq!(stored(&node_db), ["a", "b"]);
    ops.join_cluster();
    ops.raise(item("c"), None);
    let out = answer(&ops, "a", "allow");
    assert_eq!(out["answered"], true);
    let again = answer(&ops, "a", "deny");
    assert_eq!(again["answered"], false);
    ops.flush();
    assert!(stored(&node_db).is_empty());
    assert_eq!(stored(&cluster_db), ["a", "b", "c"]);
    assert_eq!(calls.0.lock().unwrap().len(), 1);
    ops.close();
    let saved = inbox::load_from(&inbox::Store::Cluster(cluster_dir), NOW).unwrap();
    let a = saved.items().into_iter().find(|i| i.id == "a").unwrap();
    assert_eq!(a.state, State::Answered);
    assert_eq!(a.answer, Some(ActionId::Allow));
}

#[test]
fn a_restart_in_a_cluster_moves_rows_left_in_notchling_db() {
    let r = rig();
    let cluster_dir = r._tmp.path("cluster");
    let node_db = r.dir.join("notchling.db");
    let cluster_db = cluster_dir.join("cluster.db");
    r.ops.raise(item("a"), None);
    r.ops.raise(item("b"), None);
    r.ops.flush();
    r.ops.close();
    in_cluster(&cluster_dir);
    let reopened = InboxOps::with_clock(
        r.dir.clone(),
        Arc::new(FakeRoom::default()),
        Arc::new(|| NOW + 5),
    )
    .with_cluster_dir(cluster_dir.clone());
    reopened.load();
    assert_eq!(reopened.items().len(), 2);
    reopened.raise(item("c"), None);
    reopened.flush();
    assert!(stored(&node_db).is_empty());
    assert_eq!(stored(&cluster_db), ["a", "b", "c"]);
    reopened.close();
}

#[test]
fn a_restart_without_a_cluster_keeps_the_inbox_in_notchling_db() {
    let r = rig();
    let cluster_dir = r._tmp.path("cluster");
    r.ops.raise(item("a"), None);
    r.ops.flush();
    r.ops.close();
    let reopened = InboxOps::with_clock(
        r.dir.clone(),
        Arc::new(FakeRoom::default()),
        Arc::new(|| NOW + 5),
    )
    .with_cluster_dir(cluster_dir.clone());
    reopened.load();
    reopened.raise(item("b"), None);
    reopened.flush();
    assert_eq!(stored(&r.dir.join("notchling.db")), ["a", "b"]);
    assert!(!cluster_dir.exists());
    reopened.close();
}

#[test]
fn leaving_a_cluster_brings_the_inbox_back_to_notchling_db() {
    let r = rig();
    let cluster_dir = r._tmp.path("cluster");
    let ops = r.ops.clone().with_cluster_dir(cluster_dir.clone());
    ops.raise(item("a"), None);
    ops.join_cluster();
    ops.raise(item("b"), None);
    ops.flush();
    assert_eq!(stored(&cluster_dir.join("cluster.db")), ["a", "b"]);
    ops.leave_cluster();
    ops.raise(item("c"), None);
    ops.flush();
    assert_eq!(stored(&r.dir.join("notchling.db")), ["a", "b", "c"]);
    assert!(stored(&cluster_dir.join("cluster.db")).is_empty());
    ops.close();
}
