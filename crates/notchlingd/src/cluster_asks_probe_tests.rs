use super::*;
use crate::cluster_forward::{Forward, NodeForward};
use crate::cluster_testkit::{pair, Opts};
use crate::daemon_wiring::Ops;
use crate::op_group::OpGroup;
use notchling_cluster_net::MemNet;
use notchling_local_api::{Handler, OpSpec};
use std::sync::mpsc::channel;

type Move = Box<dyn Fn(&Rig)>;

fn probed(r: &Rig) -> usize {
    r.admin.probes.load(Ordering::SeqCst)
}

fn primary_far(r: &Rig, name: &str) {
    set_status(r, |s| {
        s["me"] = json!(BOX);
        s["cluster"]["home"] = json!(FAR);
        s["members"][0]["role"] = json!("worker");
        s["members"][2]["role"] = json!("home");
        s["members"][2]["name"] = json!(name);
    });
}

#[test]
fn an_invite_of_another_organization_never_reaches_the_machine() {
    let r = rig("probe-after-org");
    let own = r
        .ask("invite", json!({"node": WORKBOX, "name": "workbox"}))
        .unwrap_err();
    assert_eq!(reason(&own), "other_organization");
    let guess = r
        .ask("invite", json!({"node": NEW, "name": "workbox"}))
        .unwrap_err();
    assert_eq!(
        reason(&guess),
        "other_organization",
        "a wrong node id is no way to learn about a machine of another organization"
    );
    assert_eq!(probed(&r), 0, "no ssh to a machine the caller may not name");
}

#[test]
fn allow_asks_no_machine_and_a_flaky_probe_does_not_stale_the_card() {
    let r = rig("allow-no-probe");
    let id = r.card("invite", json!({"node": NEW, "name": "newbox"}));
    assert_eq!(probed(&r), 1);
    r.admin.probe_down.store(true, Ordering::SeqCst);
    r.apply(&id, json!({"token": TOKEN})).unwrap();
    assert_eq!(
        probed(&r),
        1,
        "Allow reads the registry and the cluster only"
    );
    assert_eq!(r.ran.lock().unwrap().len(), 1);
    assert_eq!(r.row(&id).state.as_str(), "applied");
}

#[test]
fn allow_still_refuses_when_the_registry_or_the_cluster_moved_under_the_card() {
    let moves: Vec<(&str, Move)> = vec![
        (
            "host",
            Box::new(|r| *r.admin.host.lock().unwrap() = Some("elsewhere.example.com".into())),
        ),
        (
            "organization",
            Box::new(|r| {
                r.admin
                    .moved
                    .lock()
                    .unwrap()
                    .push(("newbox".into(), "Work".into()))
            }),
        ),
        (
            "removed from machines",
            Box::new(|r| r.admin.forgot.lock().unwrap().push("newbox".into())),
        ),
        (
            "joined meanwhile",
            Box::new(|r| {
                set_status(r, |s| {
                    s["members"].as_array_mut().unwrap().push(json!({
                        "id": NEW, "name": "newbox", "role": "worker", "canBeMaster": false,
                        "state": "active", "acceptsDelegation": false,
                        "secrets": {"names": [], "any": false}, "addrs": [], "port": 4001}))
                })
            }),
        ),
        (
            "other cluster",
            Box::new(|r| set_status(r, |s| s["cluster"]["id"] = json!("c2"))),
        ),
    ];
    for (what, change) in moves {
        let r = rig("allow-local");
        let id = r.card("invite", json!({"node": NEW, "name": "newbox"}));
        change(&r);
        let stale = r.apply(&id, json!({"token": TOKEN})).unwrap_err();
        assert_eq!(reason(&stale), "stale", "{what}");
        assert!(r.ran.lock().unwrap().is_empty(), "{what}");
        assert_eq!(probed(&r), 1, "{what}: still no second probe");
    }
}

#[test]
fn an_invite_asked_while_the_probe_waits_is_refused_when_the_cluster_changed_and_nothing_is_locked()
{
    let r = rig("probe-race");
    let (entered, entered_rx) = channel();
    let (release_tx, release) = channel();
    *r.admin.gate.lock().unwrap() = Some(Arc::new(Gate {
        entered: Mutex::new(entered),
        release: Mutex::new(release),
    }));
    let asks = r.asks.clone();
    let waiting = std::thread::spawn(move || {
        asks.call(
            &agent_caller(),
            ops::CLUSTER_ADMIN_REQUEST,
            &json!({"action": "invite", "args": {"node": NEW, "name": "newbox"}}),
        )
    });
    entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the probe started");
    set_status(&r, |s| s["cluster"]["id"] = json!("c2"));
    assert_eq!(r.open_count(), 0);
    let other = r.card("failover", json!({"value": "on"}));
    assert_eq!(
        r.consent.get_row(&other).unwrap().state.as_str(),
        "waiting",
        "another request is asked and answered while ssh waits"
    );
    release_tx.send(()).unwrap();
    let late = waiting.join().unwrap().unwrap_err();
    assert_eq!(reason(&late), "stale");
    assert_eq!(r.open_count(), 1, "the invite opened no card");
}

#[test]
fn the_cluster_is_read_again_right_before_the_op_runs() {
    let r = rig("last-look");
    let id = r.card("remove", json!({"node": "build"}));
    let admin = r.admin.clone();
    *r.admin.on_host.lock().unwrap() = Some(Box::new(move || {
        admin.status.lock().unwrap()["cluster"]["id"] = json!("c2");
    }));
    let stale = r.apply(&id, json!({})).unwrap_err();
    assert_eq!(reason(&stale), "stale");
    assert!(
        r.ran.lock().unwrap().is_empty(),
        "the cluster changed after the recheck and before the op"
    );
    assert_ne!(r.row(&id).state.as_str(), "applied");
}

#[test]
fn a_primary_known_only_by_another_name_is_refused_never_taken_for_personal() {
    let r = rig("primary-name");
    primary_far(&r, "workstation");
    for (action, args) in [
        ("token", json!({})),
        ("failover", json!({"value": "on"})),
        ("move_now", json!({})),
        ("move_home", json!({"node": "spare"})),
    ] {
        let refused = r.ask(action, args).unwrap_err();
        assert_eq!(reason(&refused), "unknown_owner", "{action}");
    }
    assert_eq!(r.open_count(), 0);
}

#[test]
fn a_registered_name_that_is_another_node_does_not_own_the_primary() {
    let r = rig("primary-spoof");
    primary_far(&r, "newbox");
    let refused = r.ask("token", json!({})).unwrap_err();
    assert_eq!(
        reason(&refused),
        "unknown_owner",
        "newbox answers as another node"
    );
    assert_eq!(probed(&r), 1);
}

#[test]
fn a_member_machines_does_not_know_is_refused_never_taken_for_personal() {
    let r = rig("member-unknown");
    set_status(&r, |s| s["members"][1]["name"] = json!("laptop-3"));
    for (action, args) in [
        ("remove", json!({"node": BOX})),
        ("member", json!({"node": BOX, "role": "master"})),
        ("allow", json!({"node": BOX, "names": ["deploy-token"]})),
    ] {
        let refused = r.ask(action, args).unwrap_err();
        assert_eq!(reason(&refused), "unknown_owner", "{action}");
    }
    assert_eq!(r.open_count(), 0);
}

#[test]
fn a_cluster_of_this_mac_alone_needs_no_registry_and_no_ssh() {
    for (action, args) in [
        ("token", json!({})),
        ("failover", json!({"value": "on"})),
        ("main", json!({"session": "sess-2"})),
        ("move_now", json!({})),
    ] {
        let r = rig(&format!("single-{action}"));
        set_status(&r, |s| {
            let me = s["members"][0].clone();
            s["members"] = json!([me]);
        });
        r.card(action, args.clone());
        assert_eq!(probed(&r), 0, "{action}: this Mac is not asked over ssh");
        r.as_session("worker", Some("Work"));
        let refused = r.ask(action, args).unwrap_err();
        assert_eq!(reason(&refused), "other_organization", "{action}");
    }
}

#[test]
fn a_name_in_machines_twice_is_refused_for_an_invite_and_for_a_member() {
    let r = rig("twice");
    r.admin.twice.lock().unwrap().push("newbox".into());
    r.admin.twice.lock().unwrap().push("build".into());
    let invite = r
        .ask("invite", json!({"node": NEW, "name": "newbox"}))
        .unwrap_err();
    assert_eq!(reason(&invite), "unknown_owner");
    let remove = r.ask("remove", json!({"node": BOX})).unwrap_err();
    assert_eq!(reason(&remove), "unknown_owner");
    assert_eq!(probed(&r), 0);
    assert_eq!(r.open_count(), 0);
}

#[test]
fn this_mac_with_an_unreadable_organizations_document_is_refused_not_personal() {
    let r = rig("own-closed");
    r.admin.closed.store(true, Ordering::SeqCst);
    for (action, args) in [
        ("token", json!({})),
        ("failover", json!({"value": "on"})),
        ("main", json!({"session": "sess-2"})),
        ("move_now", json!({})),
    ] {
        let refused = r.ask(action, args).unwrap_err();
        assert_eq!(reason(&refused), "unknown_owner", "{action}");
    }
    assert_eq!(r.open_count(), 0);
}

struct Record(Mutex<Vec<(String, Value)>>);

impl OpGroup for Record {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        APP_OPS.contains(&op).then_some(OpSpec::APP)
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        self.0.lock().unwrap().push((op.to_string(), args.clone()));
        Ok(json!({"ran": op}))
    }
}

#[test]
fn a_cited_apply_on_a_client_runs_where_the_same_call_without_an_id_would() {
    let net = MemNet::new();
    let mut client = Opts::named("client");
    client.client = true;
    let (home, client) = pair(&net, Opts::named("home"), client);
    let forward = Arc::new(NodeForward::new(client.node.clone()));
    for (action, args, op, _) in table() {
        if action == "create" {
            continue;
        }
        let r = rig(&format!("client-{action}"));
        let record = Arc::new(Record(Mutex::new(Vec::new())));
        let ops = Ops::new(Arc::new(Mutex::new(json!([]))))
            .with_cluster_asks(r.asks.clone())
            .with_forward(forward.clone())
            .with_group(record.clone());
        let id = r.card(action, args);
        let applied = r.row(&id).change["apply"]["args"].clone();
        let mut cited = applied.clone();
        cited["consentId"] = json!(id);
        assert!(forward.forward(&app_caller(), op, &applied).is_none());
        assert!(forward.forward(&app_caller(), op, &cited).is_none());
        let plain = ops.call(&app_caller(), op, &applied).unwrap();
        let done = ops.call(&app_caller(), op, &cited).unwrap();
        assert_eq!(plain, done, "{action}");
        assert_eq!(
            *record.0.lock().unwrap(),
            vec![(op.to_string(), applied.clone()), (op.to_string(), applied)],
            "{action}: the op got the same args with and without the id"
        );
        assert_eq!(r.row(&id).state.as_str(), "applied", "{action}");
    }
    assert!(
        home.calls.calls.lock().unwrap().is_empty(),
        "the primary was handed nothing by the forward step"
    );
}

struct Spy(Mutex<Vec<String>>);

impl crate::cluster_forward::Forward for Spy {
    fn forward(
        &self,
        _caller: &Caller,
        op: &str,
        _args: &Value,
    ) -> Option<Result<Value, ApiError>> {
        self.0.lock().unwrap().push(op.to_string());
        Some(Ok(json!({"forwarded": true})))
    }
}

#[test]
fn a_cited_id_is_rechecked_here_before_anything_is_forwarded_to_the_primary() {
    use notchling_local_api::Handler;
    let r = rig("order");
    let spy = Arc::new(Spy(Mutex::new(Vec::new())));
    let ops = crate::daemon_wiring::Ops::new(Arc::new(Mutex::new(json!([]))))
        .with_cluster_asks(r.asks.clone())
        .with_forward(spy.clone());
    let plain = ops
        .call(&app_caller(), ops::CLUSTER_REMOVE, &json!({"node": BOX}))
        .unwrap();
    assert_eq!(plain["forwarded"], true, "the spy is on the path");
    spy.0.lock().unwrap().clear();

    let id = r.card("remove", json!({"node": "build"}));
    set_status(&r, |s| s["cluster"]["id"] = json!("c2"));
    let stale = ops
        .call(
            &app_caller(),
            ops::CLUSTER_REMOVE,
            &json!({"node": BOX, "consentId": id}),
        )
        .unwrap_err();
    assert_eq!(reason(&stale), "stale");
    let ghost = ops
        .call(
            &app_caller(),
            ops::CLUSTER_REMOVE,
            &json!({"node": BOX, "consentId": "ghost"}),
        )
        .unwrap_err();
    assert_eq!(reason(&ghost), "needs_consent");
    assert!(spy.0.lock().unwrap().is_empty(), "nothing was forwarded");
    assert_eq!(r.row(&id).state.as_str(), "withdrawn");
}
