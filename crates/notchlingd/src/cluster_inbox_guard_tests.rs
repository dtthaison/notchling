use super::{coalesced, InboxCalls, Net, QUEUE_MAX};
use crate::cluster_inbox_testkit::{cluster, node, Fabric, Spec};
use crate::cluster_node::LocalCalls;
use crate::cluster_wiring::node_caller;
use crate::inbox_ops_tests::{answer, app, item, rig, NOW};
use crate::op_group::OpGroup;
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::NodeId;
use notchling_engine::inbox::{self, ActionId, Change, State};
use notchling_local_api::{ops, ErrorCode};
use serde_json::{json, Value};
use std::sync::Arc;

fn spec(n: u8, client: bool, keeps: bool) -> Spec {
    Spec { n, client, keeps }
}

fn three() -> Arc<Fabric> {
    cluster(
        &[
            spec(5, true, true),
            spec(9, false, false),
            spec(8, false, false),
        ],
        5,
    )
}

fn from(n: u8, tier: OriginTier) -> Origin {
    Origin {
        node: node(n),
        tier,
    }
}

fn send(
    fabric: &Fabric,
    to: u8,
    origin: &Origin,
    args: Value,
) -> Result<Value, notchling_local_api::ApiError> {
    fabric
        .machine(&node(to))
        .calls
        .call("node.inbox", &args, origin)
}

fn changed(id: &str) -> Value {
    json!({"event": "changed", "item": item(id)})
}

#[test]
fn a_node_cannot_send_items_owned_by_another_node() {
    let fabric = three();
    let home = fabric.machine(&node(5));
    let theirs = format!("{}/x", node(9));
    let err = send(&fabric, 5, &from(8, OriginTier::Node), changed(&theirs)).unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    let sync = json!({"event": "sync", "items": [item("fine"), item(&theirs)]});
    let err = send(&fabric, 5, &from(8, OriginTier::Node), sync).unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert!(home.ids().is_empty());
}

#[test]
fn a_node_cannot_qualify_its_own_ids_either() {
    let fabric = three();
    let own = format!("{}/x", node(8));
    let err = send(&fabric, 5, &from(8, OriginTier::Node), changed(&own)).unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
    let err = send(&fabric, 5, &from(8, OriginTier::Node), changed("")).unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
    assert!(fabric.machine(&node(5)).ids().is_empty());
}

#[test]
fn removed_touches_only_the_senders_own_items() {
    let fabric = three();
    let home = fabric.machine(&node(5));
    send(&fabric, 5, &from(9, OriginTier::Node), changed("a")).unwrap();
    home.raise("a");
    let theirs = format!("{}/a", node(9));
    let wire = |id: &str| json!({"event": "removed", "id": id});
    let err = send(&fabric, 5, &from(8, OriginTier::Node), wire(&theirs)).unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    send(&fabric, 5, &from(8, OriginTier::Node), wire("a")).unwrap();
    assert_eq!(home.ids(), vec![theirs.clone(), "a".to_string()]);
    send(&fabric, 5, &from(9, OriginTier::Node), wire("a")).unwrap();
    assert_eq!(home.ids(), vec!["a".to_string()]);
    let own = home.rig.ops.items().into_iter().next().unwrap();
    assert_eq!(own.state, State::Open);
}

#[test]
fn a_machine_that_keeps_nothing_refuses_other_nodes_items() {
    let fabric = three();
    let err = send(&fabric, 8, &from(9, OriginTier::Node), changed("a")).unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert!(fabric.machine(&node(8)).ids().is_empty());
}

#[test]
fn node_inbox_needs_the_node_tier_and_another_node() {
    let fabric = three();
    for tier in [OriginTier::App, OriginTier::Agent] {
        let err = send(&fabric, 5, &from(9, tier), changed("a")).unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
    }
    let err = send(&fabric, 5, &from(5, OriginTier::Node), changed("a")).unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    let err = send(
        &fabric,
        5,
        &from(9, OriginTier::Node),
        json!({"event": "explode"}),
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
    assert!(fabric.machine(&node(5)).ids().is_empty());
}

#[test]
fn mirrors_carry_only_the_items_own_fields() {
    let fabric = three();
    let (home, worker) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    let mut made = item("a");
    made.open = Some(json!({"vault_approval": "ticket-1"}));
    worker.rig.ops.raise(made.clone(), None);
    worker.outbox.flush();
    let wired = fabric.sent_to(&node(5));
    let (_, op, args) = wired.last().unwrap();
    assert_eq!(op, "node.inbox");
    assert_eq!(args["event"], "sync");
    let sent = &args["items"][0];
    let mut want: Vec<String> = serde_json::to_value(&made)
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    want.sort();
    let mut got: Vec<String> = sent.as_object().unwrap().keys().cloned().collect();
    got.sort();
    assert_eq!(got, want);
    assert!(sent["open"].is_null());
    let shown = home.rig.ops.items().into_iter().next().unwrap();
    assert_eq!(shown.open, None);
    assert_eq!(shown.title, made.title);
    assert_eq!(shown.detail, made.detail);
    assert_eq!(shown.actions, made.actions);
}

#[test]
fn a_mirror_never_decides_without_a_link() {
    let r = rig();
    let owner: NodeId = node(7);
    r.ops.mirror_changed(&owner, item("a")).unwrap();
    let id = format!("{owner}/a");
    let err = r.ops.decide(&id, ActionId::Allow, "mac").unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert_eq!(err.message, crate::inbox_ops::NO_SUCH_ITEM);
    let kept = r.ops.items().into_iter().next().unwrap();
    assert_eq!(kept.state, State::Open);
}

#[test]
fn an_allow_by_someone_else_is_refused_before_it_leaves() {
    let fabric = three();
    let (home, worker) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    worker.raise("a");
    worker.outbox.flush();
    let id = format!("{}/a", node(9));
    let err = home
        .rig
        .ops
        .decide(&id, ActionId::Allow, "agent")
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert!(fabric.sent_to(&node(9)).is_empty());
}

#[test]
fn an_answer_from_the_wrong_kind_of_caller_is_refused_by_the_owner() {
    let fabric = three();
    let worker = fabric.machine(&node(9));
    worker.raise("a");
    let agent_tier = node_caller(&from(5, OriginTier::Agent));
    let err = worker
        .rig
        .ops
        .call(
            &agent_tier,
            ops::INBOX_ANSWER,
            &json!({"id": "a", "action": "allow"}),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    let node_app = node_caller(&from(5, OriginTier::App));
    let err = worker
        .rig
        .ops
        .call(&node_app, ops::INBOX_SEEN, &json!({"ids": ["a"]}))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    let out = worker
        .rig
        .ops
        .call(
            &node_app,
            ops::INBOX_ANSWER,
            &json!({"id": "a", "action": "allow", "by": "agent"}),
        )
        .unwrap();
    assert_eq!(out["answered"], true);
    let kept = worker.rig.ops.items().into_iter().next().unwrap();
    assert_eq!(kept.answered_by.as_deref(), Some("mac"));
}

#[test]
fn by_is_taken_from_a_node_but_never_from_the_app() {
    let fabric = three();
    let worker = fabric.machine(&node(9));
    worker.raise("a");
    worker.raise("b");
    let node_app = node_caller(&from(5, OriginTier::App));
    let args = |id: &str| json!({"id": id, "action": "allow", "by": "phone"});
    worker
        .rig
        .ops
        .call(&node_app, ops::INBOX_ANSWER, &args("a"))
        .unwrap();
    worker
        .rig
        .ops
        .call(&app(), ops::INBOX_ANSWER, &args("b"))
        .unwrap();
    let by = |id: &str| {
        worker
            .rig
            .ops
            .items()
            .into_iter()
            .find(|i| i.id == id)
            .unwrap()
            .answered_by
    };
    assert_eq!(by("a").as_deref(), Some("phone"));
    assert_eq!(by("b").as_deref(), Some("mac"));
}

#[test]
fn a_node_listing_gets_this_nodes_items_without_the_homes() {
    let fabric = cluster(
        &[
            spec(5, true, true),
            spec(2, true, false),
            spec(9, false, false),
        ],
        5,
    );
    let (home, client) = (fabric.machine(&node(5)), fabric.machine(&node(2)));
    home.raise("h1");
    client.raise("x1");
    let node_app = node_caller(&from(5, OriginTier::App));
    let listed = client
        .rig
        .ops
        .call(&node_app, ops::INBOX_LIST, &json!({}))
        .unwrap();
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);
}

#[test]
fn mirrors_are_never_saved_to_disk() {
    let r = rig();
    r.ops.mirror_changed(&node(7), item("a")).unwrap();
    r.ops.raise(item("own"), None);
    r.ops.flush();
    let saved = inbox::load(&r.dir, NOW).unwrap();
    let ids: Vec<String> = saved.items().into_iter().map(|i| i.id).collect();
    assert_eq!(ids, vec!["own".to_string()]);
}

#[test]
fn unread_counts_mirrors_and_seen_marks_them() {
    let r = rig();
    let owner = node(7);
    r.ops.mirror_changed(&owner, item("a")).unwrap();
    r.ops.raise(item("own"), None);
    let listed = r.ops.call(&app(), ops::INBOX_LIST, &json!({})).unwrap();
    assert_eq!(listed["unread"], 2);
    let id = format!("{owner}/a");
    let out = r
        .ops
        .call(&app(), ops::INBOX_SEEN, &json!({"ids": [id]}))
        .unwrap();
    assert_eq!(out["unread"], 1);
    r.ops.mirror_changed(&owner, item("a")).unwrap();
    let kept = r
        .ops
        .items()
        .into_iter()
        .find(|i| i.id.contains('/'))
        .unwrap();
    assert!(kept.seen);
}

#[test]
fn an_answer_to_a_qualified_id_of_this_node_means_its_own_item() {
    let fabric = three();
    let worker = fabric.machine(&node(9));
    worker.raise("a");
    let out = answer(&worker.rig.ops, &format!("{}/a", node(9)), "allow");
    assert_eq!(out["answered"], true);
    assert_eq!(worker.raised.0.lock().unwrap().len(), 1);
}

#[test]
fn coalescing_keeps_the_last_change_of_each_item() {
    let mut open = item("a");
    open.seen = false;
    let mut closed = open.clone();
    closed.state = State::Answered;
    let out = coalesced(vec![
        Change::Raised(open),
        Change::Raised(item("b")),
        Change::Updated(closed.clone()),
        Change::Removed("c".to_string()),
    ]);
    assert_eq!(out.len(), 3);
    assert!(out.contains(&Change::Updated(closed)));
}

#[test]
fn a_flooded_queue_falls_back_to_a_full_sync() {
    let fabric = three();
    let worker = fabric.machine(&node(9));
    worker.raise("a");
    worker.outbox.flush();
    let burst: Vec<Change> = (0..=QUEUE_MAX)
        .map(|n| Change::Removed(format!("gone-{n}")))
        .collect();
    worker.outbox.push(burst);
    worker.outbox.flush();
    let wired = fabric.sent_to(&node(5));
    let (_, _, last) = wired.last().unwrap();
    assert_eq!(last["event"], "sync");
}

#[test]
fn only_an_inner_op_passes_through_the_inbox_calls() {
    let fabric = three();
    let calls: &InboxCalls = &fabric.machine(&node(5)).calls;
    let err = calls
        .call("nope.nothing", &json!({}), &from(9, OriginTier::Node))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::UnknownOp);
    let net = crate::cluster_inbox_testkit::Wire {
        me: node(5),
        fabric: fabric.clone(),
    };
    assert!(net.keeps());
}
