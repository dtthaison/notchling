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
fn a_mirror_keeps_only_the_switches_the_cluster_sets() {
    let owner = node(9);
    for (name, kept) in [
        ("acting", true),
        ("bridge", true),
        ("install", true),
        ("vault", false),
        ("accounts", false),
        ("cluster", false),
    ] {
        let mut made = item("s");
        made.kind = inbox::Kind::Consent;
        made.switch = Some(name.to_string());
        let shown = crate::inbox_mirrors::display(&owner, made).unwrap();
        assert_eq!(shown.switch.is_some(), kept, "{name}");
    }
}

#[test]
fn a_mirror_keeps_a_switch_only_on_a_consent_and_no_turn_on_on_a_notice() {
    let owner = node(9);
    for kind in [inbox::Kind::Notice, inbox::Kind::SecretApproval] {
        let mut made = item("s");
        made.kind = kind;
        made.switch = Some("bridge".to_string());
        let shown = crate::inbox_mirrors::display(&owner, made).unwrap();
        assert_eq!(shown.switch, None, "{kind:?}");
    }
    let mut notice = crate::switch_asks::off_notice("acting", "claude", None, 1);
    notice.actions.push(inbox::Action {
        id: ActionId::Done,
        label: "Done".to_string(),
        primary: false,
    });
    let shown = crate::inbox_mirrors::display(&owner, notice).unwrap();
    assert_eq!(
        shown.actions.iter().map(|a| a.id).collect::<Vec<_>>(),
        vec![ActionId::Done]
    );
}

fn with_actions(kind: inbox::Kind, ids: &[ActionId]) -> inbox::Item {
    let mut made = item("f");
    made.kind = kind;
    made.actions = ids
        .iter()
        .map(|id| inbox::Action {
            id: *id,
            label: "Go".to_string(),
            primary: false,
        })
        .collect();
    made
}

fn shown_ids(made: inbox::Item) -> Vec<ActionId> {
    let shown = crate::inbox_mirrors::display(&node(9), made).unwrap();
    shown.actions.iter().map(|a| a.id).collect()
}

#[test]
fn a_mirror_shows_only_the_actions_a_peers_kind_honestly_offers() {
    use inbox::Kind;
    use ActionId::*;
    let cases: &[(Kind, &[ActionId], &[ActionId])] = &[
        (
            Kind::Consent,
            &[Continue, Allow, Deny, Open],
            &[Allow, Deny],
        ),
        (Kind::SettingChange, &[Allow, Deny, Open], &[Allow, Deny]),
        (Kind::SecretRequest, &[Deny, Open], &[Deny]),
        (
            Kind::SecretApproval,
            &[Continue, AllowOnce, AllowAlways, Deny, MoveHere],
            &[AllowOnce, Deny],
        ),
        (Kind::NodeJoin, &[Allow, Deny], &[]),
        (Kind::PrimaryUnreachable, &[MoveHere, MoveAnyway], &[]),
        (Kind::NoPrimary, &[MoveHere], &[]),
        (Kind::PrimaryBack, &[TakeBack, Keep], &[Keep]),
        (Kind::SessionWaits, &[WaitFor, ContinueHere], &[WaitFor]),
        (Kind::VersionDrift, &[Open], &[]),
        (Kind::VersionCheckFailed, &[Open], &[]),
        (Kind::Notice, &[Allow, Continue, Open, Done], &[Done]),
        (Kind::Question, &[Choice1, Cancel, Open], &[Choice1, Cancel]),
        (Kind::PausedStep, &[Continue, Done], &[Continue, Done]),
        (Kind::OutboxUnsent, &[Send, Discard], &[Send, Discard]),
        (Kind::ShellStuck, &[Stop, KeepWaiting, Done], &[]),
    ];
    for (kind, sent, kept) in cases {
        assert_eq!(
            shown_ids(with_actions(*kind, sent)),
            kept.to_vec(),
            "{kind:?}"
        );
    }
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

fn consent_from_a_peer(id: &str, offers: &[ActionId]) -> inbox::Item {
    let mut made = item(id);
    made.kind = inbox::Kind::Consent;
    made.actions = offers
        .iter()
        .map(|id| inbox::Action {
            id: *id,
            label: "x".into(),
            primary: false,
        })
        .collect();
    made
}

#[test]
fn a_mirror_recomputes_alert_actions_and_never_trusts_a_peers_allow() {
    use inbox::AlertAction;
    let mut made = consent_from_a_peer("g", &[ActionId::Deny]);
    made.alert_actions = vec![AlertAction::Allow, AlertAction::Deny, AlertAction::Change];
    let shown = crate::inbox_mirrors::display(&node(9), made).unwrap();
    assert_eq!(
        shown.alert_actions,
        vec![AlertAction::Deny, AlertAction::Open]
    );
    let mut notice = item("n");
    notice.kind = inbox::Kind::Notice;
    notice.alert_actions = vec![AlertAction::Deny, AlertAction::Dismiss];
    let shown = crate::inbox_mirrors::display(&node(9), notice).unwrap();
    assert!(shown.alert_actions.is_empty());
    let mut approval = item("a");
    approval.alert_actions = vec![AlertAction::Allow];
    let shown = crate::inbox_mirrors::display(&node(9), approval).unwrap();
    assert!(!shown.alert_actions.contains(&AlertAction::Allow));
}

#[test]
fn a_mirror_keeps_only_a_reason_from_the_known_lines() {
    use inbox::{Outcome, OutcomeState};
    let known = "That machine is busy with another change.";
    for (reason, kept) in [
        (known, Some(known)),
        ("It didn't go through.", Some("It didn't go through.")),
        ("Couldn't write ~/.claude/x", None),
        ("That machine is busy with another change. ghp_x", None),
        ("anything a peer likes", None),
    ] {
        let mut made = consent_from_a_peer("g", &[ActionId::Deny]);
        made.outcome = Some(Outcome {
            state: OutcomeState::Failed,
            reason: Some(reason.to_string()),
            at_ms: 1,
        });
        let shown = crate::inbox_mirrors::display(&node(9), made).unwrap();
        let outcome = shown.outcome.unwrap();
        assert_eq!(outcome.state, OutcomeState::Failed);
        assert_eq!(outcome.reason.as_deref(), kept, "{reason}");
    }
}

#[test]
fn a_mirror_cleans_a_peers_note_and_place() {
    use inbox::Place;
    let mut made = consent_from_a_peer("g", &[ActionId::Deny]);
    made.note = Some(format!(
        "see /Users/x/.ssh/id now\u{7} ghp_abcdefghij\u{200b}klmnopqrstuvwxyz0123 {}",
        "n ".repeat(450)
    ));
    made.place = Some(Place {
        branch: Some("fix/inbox\u{7}".to_string()),
        session_name: Some("(~/x)".to_string()),
        session: Some("0123abcd-4567-89ab-cdef-0123456789ab".to_string()),
        ..Place::default()
    });
    let shown = crate::inbox_mirrors::display(&node(9), made).unwrap();
    let note = shown.note.unwrap();
    assert!(note.starts_with("see … now"), "{note}");
    assert!(
        !note.contains("abcdefghij") && !note.contains("klmnop"),
        "{note}"
    );
    assert_eq!(note.chars().count(), 300);
    assert!(!note.chars().any(char::is_control));
    let place = shown.place.unwrap();
    assert_eq!(place.branch.as_deref(), Some("fix/inbox"));
    assert_eq!(place.session_name, None);
    assert_eq!(
        place.session.as_deref(),
        Some("0123abcd-4567-89ab-cdef-0123456789ab")
    );
}

#[test]
fn a_peer_cannot_claim_another_machines_node_or_name() {
    use inbox::Place;
    let mut made = item("g");
    made.source.node = Some(node(7).to_string());
    made.place = Some(Place {
        machine: Some(node(7).to_string()),
        machine_name: Some("the boss".to_string()),
        ..Place::default()
    });
    let shown = crate::inbox_mirrors::display(&node(9), made.clone()).unwrap();
    let place = shown.place.clone().unwrap();
    assert_eq!(shown.source.node, Some(node(9).to_string()));
    assert_eq!(place.machine, Some(node(9).to_string()));
    assert_eq!(place.machine_name, None);
    let mut mirrors = crate::inbox_mirrors::Mirrors::default();
    mirrors
        .changed_named(&node(9), made, Some("build".to_string()))
        .unwrap();
    let place = mirrors.items()[0].place.clone().unwrap();
    assert_eq!(place.machine_name.as_deref(), Some("build"));
}

#[test]
fn the_items_a_home_sends_are_cleaned_like_a_mirror() {
    let mut made = item("h");
    made.kind = inbox::Kind::Notice;
    made.alert_actions = vec![inbox::AlertAction::Allow];
    made.source.node = Some(node(7).to_string());
    let shown = crate::inbox_mirrors::localize(&node(1), &node(9), vec![made]);
    assert!(shown[0].alert_actions.is_empty());
    assert_eq!(shown[0].source.node, Some(node(9).to_string()));
}

struct Names;

impl crate::inbox_mirrors::InboxLink for Names {
    fn me(&self) -> Option<NodeId> {
        Some(node(1))
    }

    fn answer(
        &self,
        _: &NodeId,
        _: &str,
        _: ActionId,
        _: &str,
    ) -> Result<Value, notchling_local_api::ApiError> {
        Err(notchling_local_api::ApiError::new(ErrorCode::Failed, "no"))
    }

    fn home_items(&self) -> crate::inbox_mirrors::Home {
        crate::inbox_mirrors::Home::Off
    }

    fn machine_name(&self, node: &NodeId) -> Option<String> {
        (*node == self::node(9)).then(|| "build".to_string())
    }
}

#[test]
fn an_item_from_another_machine_names_it_in_its_place() {
    let mut mine = item("m");
    let mut theirs = item("t");
    theirs.source.node = Some(node(9).to_string());
    let mut items = vec![mine.clone(), theirs];
    crate::inbox_mirrors::name_machines(&mut items, &Names);
    mine.place = None;
    assert_eq!(items[0], mine);
    let place = items[1].place.clone().unwrap();
    assert_eq!(place.machine, Some(node(9).to_string()));
    assert_eq!(place.machine_name, Some("build".to_string()));
}

#[test]
fn a_mirror_push_names_the_machine_from_the_cluster() {
    let r = rig();
    r.ops.set_link(Arc::new(Names));
    r.ops.mirror_changed(&node(9), item("p")).unwrap();
    r.ops.flush();
    let pushed = r.room.named("inbox.changed");
    let place = &pushed.last().expect("a push")["item"]["place"];
    assert_eq!(place["machine_name"], "build", "{place}");
}

#[test]
fn a_mirror_keeps_the_owners_live_fields_and_clamps_a_future_waiting_since() {
    let owner = node(9);
    let mut made = item("w");
    made.blocking = Some(true);
    made.waiter = Some(inbox::Waiter::Cli);
    made.waiting_since = Some(1_000);
    let shown = crate::inbox_mirrors::display(&owner, made.clone()).unwrap();
    assert_eq!(shown.blocking, Some(true));
    assert_eq!(shown.waiter, Some(inbox::Waiter::Cli));
    assert_eq!(shown.waiting_since, Some(1_000));
    made.waiting_since = Some(i64::MAX);
    let shown = crate::inbox_mirrors::display(&owner, made).unwrap();
    let now = crate::acting_ops::now_ms();
    assert!(
        shown.waiting_since.unwrap() <= now,
        "{:?}",
        shown.waiting_since
    );
    assert_eq!(shown.blocking, Some(true));
}

#[test]
fn a_mirror_drops_live_fields_that_do_not_say_blocking() {
    let owner = node(9);
    let mut made = item("w");
    made.waiter = Some(inbox::Waiter::Ask);
    made.waiting_since = Some(1_000);
    let shown = crate::inbox_mirrors::display(&owner, made).unwrap();
    assert_eq!(shown.blocking, None);
    assert_eq!(shown.waiter, None);
    assert_eq!(shown.waiting_since, None);
}

#[test]
fn a_mirror_drops_live_fields_from_a_closed_item_or_one_without_a_start() {
    let owner = node(9);
    let mut closed = item("w");
    closed.state = State::Answered;
    closed.blocking = Some(true);
    closed.waiter = Some(inbox::Waiter::Cli);
    closed.waiting_since = Some(1_000);
    let shown = crate::inbox_mirrors::display(&owner, closed).unwrap();
    assert_eq!(shown.blocking, None);
    assert_eq!(shown.waiter, None);
    assert_eq!(shown.waiting_since, None);
    let mut open = item("x");
    open.blocking = Some(true);
    open.waiter = Some(inbox::Waiter::Cli);
    let shown = crate::inbox_mirrors::display(&owner, open).unwrap();
    assert_eq!(shown.blocking, None);
    assert_eq!(shown.waiter, None);
}
