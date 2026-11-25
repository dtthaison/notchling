use super::Place;
use crate::acting_ops::now_ms;
use crate::acting_ops_tests::FakeRoom;
use crate::cluster_inbox_testkit::{cluster, node, Fabric, Spec};
use crate::consent_ops::{ConsentOps, HELD_MS};
use crate::consent_testkit::{agent_caller, item_id_for, verified};
use crate::event_assert::{of, text};
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use crate::op_group::OpGroup;
use crate::switch_asks::{States, SwitchAsks, Turn};
use notchling_engine::consent::ConsentStore;
use notchling_engine::inbox::{ActionId, Kind};
use notchling_events::{MemorySink, Salt};
use notchling_local_api::{ops, AgentIdentity, ApiError, Caller, ErrorCode, Handler, OpSpec};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Board = Arc<Mutex<HashMap<String, bool>>>;

struct Side {
    consent: Arc<ConsentOps>,
    asks: Arc<SwitchAsks>,
    store: Arc<ConsentStore>,
    sink: Arc<MemorySink>,
    clock: Arc<AtomicI64>,
    on: Board,
    turns: Arc<Mutex<Vec<(String, bool)>>>,
}

impl Drop for Side {
    fn drop(&mut self) {
        self.consent.close();
    }
}

fn side(inbox: Option<Arc<InboxOps>>, place: Place) -> Side {
    side_with(inbox, place, Arc::default())
}

fn side_with(inbox: Option<Arc<InboxOps>>, place: Place, on: Board) -> Side {
    let store = Arc::new(ConsentStore::in_memory_for_tests());
    let sink = Arc::new(MemorySink::new());
    let clock = Arc::new(AtomicI64::new(now_ms()));
    let ticks = clock.clone();
    let consent = ConsentOps::with_clock(
        store.clone(),
        Arc::new(FakeRoom::default()),
        inbox.clone(),
        Some(Events::fixed(sink.clone(), Salt([5; 32]))),
        Arc::new(move || ticks.load(Ordering::SeqCst)),
    );
    consent.bind_identity(Arc::new(|_| verified("s-worker")));
    consent.start();
    let turns = Arc::new(Mutex::new(Vec::new()));
    let seen = on.clone();
    let states: States =
        Arc::new(move |name: &str| Some(*seen.lock().unwrap().get(name).unwrap_or(&false)));
    let (flip, log) = (on.clone(), turns.clone());
    let turn: Turn = Arc::new(move |name: &str, value: bool, _extra: &Value| {
        flip.lock().unwrap().insert(name.to_string(), value);
        log.lock().unwrap().push((name.to_string(), value));
        Ok(json!({"switch": name, "on": value}))
    });
    let asks = SwitchAsks::new(consent.clone(), states, turn, inbox);
    asks.set_place(Arc::new(move || place.clone()));
    Side {
        consent,
        asks,
        store,
        sink,
        clock,
        on,
        turns,
    }
}

fn worker(linked: bool) -> Place {
    Place::Worker {
        name: "m9".to_string(),
        linked,
    }
}

fn pair() -> Arc<Fabric> {
    let fabric = cluster(
        &[
            Spec {
                n: 5,
                client: true,
                keeps: true,
            },
            Spec {
                n: 9,
                client: false,
                keeps: false,
            },
        ],
        5,
    );
    fabric.knows_consent.lock().unwrap().insert(node(5));
    fabric
}

fn settle(fabric: &Fabric) {
    let machines: Vec<_> = fabric.machines.lock().unwrap().values().cloned().collect();
    for machine in machines {
        machine.outbox.flush();
        machine.rig.ops.flush();
    }
}

fn bounded(body: impl FnOnce() + Send + 'static) {
    let (done, finished) = std::sync::mpsc::channel();
    let run = std::thread::spawn(move || {
        body();
        let _ = done.send(());
    });
    match finished.recv_timeout(Duration::from_secs(30)) {
        Ok(()) => run.join().unwrap(),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            std::panic::resume_unwind(run.join().unwrap_err())
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("the test hung for 30 s")
        }
    }
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

impl Side {
    fn ask(&self, name: &str) -> Result<Value, ApiError> {
        self.asks.call(
            &agent_caller(),
            ops::SWITCH_REQUEST,
            &json!({"switch": name, "on": true, "consent": {"reason": "please"}}),
        )
    }

    fn state(&self, id: &str) -> String {
        self.store
            .get(id)
            .unwrap()
            .unwrap()
            .state
            .as_str()
            .to_string()
    }

    fn is_on(&self, name: &str) -> bool {
        *self.on.lock().unwrap().get(name).unwrap_or(&false)
    }

    fn thens(&self) -> Vec<String> {
        of(&self.sink, "agent.consent")
            .iter()
            .filter_map(|e| text(e, "then"))
            .collect()
    }
}

fn opened_at_home(fabric: &Fabric, w: &Side, name: &str) -> (String, String) {
    let machine = fabric.machine(&node(9));
    let reply = w.ask(name).unwrap();
    assert_eq!(reply["state"], "waiting");
    let id = reply["request"]["id"].as_str().unwrap().to_string();
    let item = item_id_for(&machine.rig.ops, &id);
    assert!(!item.is_empty(), "the request raised an item on the Worker");
    settle(fabric);
    (id, format!("{}/{}", node(9), item))
}

#[test]
fn a_worker_request_reaches_the_home_and_lands_through_desired() {
    bounded(a_worker_request_reaches_the_home_and_lands_through_desired_body);
}

fn a_worker_request_reaches_the_home_and_lands_through_desired_body() {
    let fabric = pair();
    let (home, machine) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    let w = side(Some(machine.rig.ops.clone()), worker(true));
    let (id, qualified) = opened_at_home(&fabric, &w, "acting");
    let shown = home
        .rig
        .ops
        .items()
        .into_iter()
        .find(|i| i.id == qualified)
        .expect("the home shows the Worker's request");
    assert_eq!(shown.kind, Kind::Consent);
    let done = home
        .rig
        .ops
        .decide(&qualified, ActionId::Allow, "mac")
        .unwrap();
    assert!(done.answered);
    assert_eq!(w.state(&id), "applying");
    wait_until("the yes recorded", || {
        w.thens().iter().any(|t| t == "allowed")
    });
    assert!(
        w.turns.lock().unwrap().is_empty(),
        "the Worker turns nothing on"
    );
    assert!(!w.is_on("acting"));
    assert_eq!(w.state(&id), "applying");
    w.on.lock().unwrap().insert("acting".into(), true);
    wait_until("applied", || w.state(&id) == "applied");
    assert_eq!(w.thens().last().map(String::as_str), Some("applied"));
    assert!(w.turns.lock().unwrap().is_empty());
}

#[test]
fn a_worker_with_no_app_it_can_reach_refuses_with_no_app() {
    bounded(a_worker_with_no_app_it_can_reach_refuses_with_no_app_body);
}

fn a_worker_with_no_app_it_can_reach_refuses_with_no_app_body() {
    let fabric = pair();
    let machine = fabric.machine(&node(9));
    let unlinked = side(Some(machine.rig.ops.clone()), worker(false));
    for name in crate::switch_asks::NAMES {
        let refused = unlinked.ask(name).unwrap_err();
        assert_eq!(refused.code, ErrorCode::Forbidden, "{name}");
        assert_eq!(refused.data.as_ref().unwrap()["reason"], json!("no_app"));
        assert_eq!(refused.message, "Open Notchling on m9 to allow this.");
    }
    let linked = side(Some(machine.rig.ops.clone()), worker(true));
    for name in ["accounts", "vault", "cluster"] {
        let refused = linked.ask(name).unwrap_err();
        assert_eq!(refused.data.as_ref().unwrap()["reason"], json!("no_app"));
    }
    assert!(unlinked.store.all().unwrap().is_empty());
    assert!(linked.store.all().unwrap().is_empty());
    assert!(machine.ids().is_empty(), "no card on the Worker");
    settle(&fabric);
    assert!(
        fabric.machine(&node(5)).ids().is_empty(),
        "nothing reaches the home"
    );
    assert!(unlinked.turns.lock().unwrap().is_empty());
}

#[test]
fn a_worker_never_turns_a_switch_on_without_the_apps_yes() {
    bounded(a_worker_never_turns_a_switch_on_without_the_apps_yes_body);
}

fn a_worker_never_turns_a_switch_on_without_the_apps_yes_body() {
    let fabric = pair();
    let (home, machine) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    let w = side(Some(machine.rig.ops.clone()), worker(true));
    let (id, qualified) = opened_at_home(&fabric, &w, "bridge");
    let phone = home.rig.ops.decide(&qualified, ActionId::Allow, "phone");
    assert!(
        !phone.is_ok_and(|o| o.answered),
        "a phone's Allow is not taken"
    );
    assert_eq!(w.state(&id), "waiting");
    let refused = w
        .consent
        .decide(&id, true, None, "phone", None)
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Forbidden);
    assert_eq!(w.state(&id), "waiting");
    home.rig
        .ops
        .decide(&qualified, ActionId::Allow, "mac")
        .unwrap();
    assert_eq!(w.state(&id), "applying");
    wait_until("the yes recorded", || {
        w.thens().iter().any(|t| t == "allowed")
    });
    w.clock.fetch_add(HELD_MS, Ordering::SeqCst);
    wait_until("withdrawn", || w.state(&id) == "withdrawn");
    assert!(!w.is_on("bridge"), "a yes alone never turns it on");
    assert!(w.turns.lock().unwrap().is_empty());
}

#[test]
fn a_home_allow_for_a_switch_already_on_is_withdrawn() {
    bounded(a_home_allow_for_a_switch_already_on_is_withdrawn_body);
}

fn a_home_allow_for_a_switch_already_on_is_withdrawn_body() {
    let fabric = pair();
    let (home, machine) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    let w = side(Some(machine.rig.ops.clone()), worker(true));
    let (id, qualified) = opened_at_home(&fabric, &w, "acting");
    w.on.lock().unwrap().insert("acting".into(), true);
    let done = home.rig.ops.decide(&qualified, ActionId::Allow, "mac");
    assert!(!done.is_ok_and(|o| o.answered), "no member.set follows");
    wait_until("withdrawn", || w.state(&id) == "withdrawn");
    wait_until("withdrawn recorded", || {
        w.thens().last().map(String::as_str) == Some("withdrawn")
    });
    assert!(!w.thens().iter().any(|t| t == "applied"));
    assert!(w.turns.lock().unwrap().is_empty());
}

#[test]
fn a_stale_allow_closes_the_request_at_once_so_a_second_answer_is_refused() {
    bounded(a_stale_allow_closes_the_request_at_once_so_a_second_answer_is_refused_body);
}

fn a_stale_allow_closes_the_request_at_once_so_a_second_answer_is_refused_body() {
    let fabric = pair();
    let machine = fabric.machine(&node(9));
    let w = side(Some(machine.rig.ops.clone()), worker(true));
    let (id, _) = opened_at_home(&fabric, &w, "acting");
    w.consent.close();
    w.on.lock().unwrap().insert("acting".into(), true);
    let stale = w.consent.decide(&id, true, None, "mac", None).unwrap();
    assert_eq!(stale["answered"], json!(false));
    assert_eq!(w.state(&id), "withdrawn", "claimed before any thread runs");
    w.on.lock().unwrap().insert("acting".into(), false);
    let again = w.consent.decide(&id, true, None, "mac", None).unwrap();
    assert_eq!(again["answered"], json!(false), "no member.set follows");
    let deny = w.consent.decide(&id, false, None, "mac", None).unwrap();
    assert_eq!(deny["answered"], json!(false));
    assert_eq!(w.state(&id), "withdrawn");
    assert!(w.turns.lock().unwrap().is_empty());
}

#[test]
fn a_switch_that_lands_right_after_the_home_allow_is_applied() {
    bounded(a_switch_that_lands_right_after_the_home_allow_is_applied_body);
}

fn a_switch_that_lands_right_after_the_home_allow_is_applied_body() {
    let fabric = pair();
    let (home, machine) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    let w = side(Some(machine.rig.ops.clone()), worker(true));
    let (id, qualified) = opened_at_home(&fabric, &w, "bridge");
    let done = home
        .rig
        .ops
        .decide(&qualified, ActionId::Allow, "mac")
        .unwrap();
    assert!(done.answered);
    w.on.lock().unwrap().insert("bridge".into(), true);
    wait_until("applied", || w.state(&id) == "applied");
    assert_eq!(w.thens().last().map(String::as_str), Some("applied"));
    assert!(w.turns.lock().unwrap().is_empty());
}

#[test]
fn a_switch_off_on_a_worker_with_no_app_offers_no_turn_on_anywhere() {
    bounded(a_switch_off_on_a_worker_with_no_app_offers_no_turn_on_anywhere_body);
}

fn a_switch_off_on_a_worker_with_no_app_offers_no_turn_on_anywhere_body() {
    let fabric = pair();
    let (home, machine) = (fabric.machine(&node(5)), fabric.machine(&node(9)));
    let w = side(Some(machine.rig.ops.clone()), worker(true));
    for name in ["acting", "vault"] {
        w.on.lock().unwrap().insert(name.into(), true);
        w.asks
            .call(&agent_caller(), ops::SWITCH_OFF, &json!({"switch": name}))
            .unwrap();
        assert!(!w.is_on(name));
    }
    machine.rig.ops.flush();
    let notices: Vec<_> = machine
        .rig
        .ops
        .items()
        .into_iter()
        .filter(|i| i.id.starts_with("switch-off:"))
        .collect();
    assert_eq!(notices.len(), 2, "the Worker still tells the user");
    settle(&fabric);
    for notice in notices {
        assert!(notice.actions.is_empty(), "no Turn On on a headless Worker");
        let qualified = format!("{}/{}", node(9), notice.id);
        let shown = home
            .rig
            .ops
            .items()
            .into_iter()
            .find(|i| i.id == qualified)
            .expect("the home shows the notice");
        assert!(shown.actions.iter().all(|a| a.id != ActionId::Continue));
        let forced = home.rig.ops.decide(&qualified, ActionId::Continue, "mac");
        assert!(!forced.is_ok_and(|o| o.answered));
        let local = machine
            .rig
            .ops
            .decide(&notice.id, ActionId::Continue, "mac");
        assert!(!local.is_ok_and(|o| o.answered));
    }
    settle(&fabric);
    assert!(!w.is_on("acting"));
    assert!(!w.is_on("vault"));
    assert_eq!(
        w.turns.lock().unwrap().iter().filter(|(_, on)| *on).count(),
        0,
        "the Worker never turned a switch on"
    );
}

struct Served(Arc<SwitchAsks>);

impl Handler for Served {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        self.0.spec(op)
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        self.0.call(caller, op, args)
    }

    fn identity_of(&self, _pid: u32) -> AgentIdentity {
        verified("s-served")
    }

    fn known_ops(&self) -> &[&'static str] {
        &[ops::SWITCH_REQUEST, ops::SWITCH_OFF]
    }
}

#[test]
fn switch_request_and_switch_off_each_record_agent_called() {
    bounded(switch_request_and_switch_off_each_record_agent_called_body);
}

fn switch_request_and_switch_off_each_record_agent_called_body() {
    use notchling_local_api::{
        Allowlist, Client, ClientKind, ConnectOptions, Limits, Server, ServerConfig,
    };
    let s = side(None, Place::Mac);
    s.on.lock().unwrap().insert("install".into(), true);
    let dir = std::path::PathBuf::from(format!("/tmp/nl-sw-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("s.sock");
    let events = Arc::new(MemorySink::new());
    let server = Server::bind(
        ServerConfig {
            socket: socket.clone(),
            allowlist: Allowlist::empty(),
            events: Some(events.clone()),
            version: "9.9.9".into(),
            limits: Limits::default(),
        },
        Arc::new(Served(s.asks.clone())),
    )
    .unwrap();
    let (handle, _thread) = server.spawn().unwrap();
    let options = ConnectOptions {
        check_server: false,
        ..ConnectOptions::default()
    };
    let mut agent = Client::connect_with(&socket, ClientKind::Cli, "0.1.0", &options).unwrap();
    let asked = agent
        .call(ops::SWITCH_REQUEST, json!({"switch": "acting", "on": true}))
        .unwrap();
    assert_eq!(asked["state"], "waiting");
    let off = agent
        .call(ops::SWITCH_OFF, json!({"switch": "install"}))
        .unwrap();
    assert_eq!(off["changed"], json!(true));
    let calls = of(&events, "agent.called");
    handle.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
    let seen: Vec<(Option<String>, Option<String>)> = calls
        .iter()
        .map(|e| (text(e, "op"), text(e, "result")))
        .collect();
    assert_eq!(
        seen,
        [
            (
                Some(ops::SWITCH_REQUEST.to_string()),
                Some("pending".to_string())
            ),
            (Some(ops::SWITCH_OFF.to_string()), Some("ok".to_string())),
        ]
    );
    assert_eq!(text(&calls[0], "tool").as_deref(), Some("claude"));
    assert!(
        text(&calls[0], "consent").is_some(),
        "the request's id is on the line"
    );
}

struct Panel(Board);

impl crate::cluster_switches::SwitchPanel for Panel {
    fn get(&self, switch: &str) -> Option<bool> {
        Some(*self.0.lock().unwrap().get(switch).unwrap_or(&false))
    }

    fn set(&self, switch: &str, on: bool) -> Result<(), ApiError> {
        self.0.lock().unwrap().insert(switch.to_string(), on);
        Ok(())
    }
}

struct Follows(Arc<crate::cluster_switches::HeadlessSwitches>);

impl crate::cluster_node::Hooks for Follows {
    fn desired(&self, desired: notchling_engine::cluster::Desired) {
        self.0.follow(desired);
    }
}

fn the_app() -> Caller {
    Caller {
        tier: notchling_local_api::Tier::App,
        client: notchling_local_api::ClientKind::App,
        peer: None,
        pid: None,
    }
}

#[test]
fn a_mac_allow_on_a_workers_card_turns_the_switch_on_and_applies_the_row() {
    bounded(a_mac_allow_on_a_workers_card_turns_the_switch_on_and_applies_the_row_body);
}

fn a_mac_allow_on_a_workers_card_turns_the_switch_on_and_applies_the_row_body() {
    use crate::cluster_inbox_testkit::cluster_of;
    use crate::cluster_ops::{follow_switch, ClusterOps};
    use crate::cluster_testkit::{eventually, pair as joined, test_log, Opts};
    use crate::switches::{SwitchFlag, CLUSTER, CLUSTER_OFF};
    use notchling_engine::consent::member_set_args;

    let board: Board = Arc::default();
    let switches = Arc::new(crate::cluster_switches::HeadlessSwitches::new(
        Arc::new(Panel(board.clone())),
        Arc::new(FakeRoom::default()),
        test_log().1,
    ));
    let mut worker_opts = Opts::named("p3hworker");
    worker_opts.hooks = Some(Arc::new(Follows(switches)));
    let net = notchling_cluster_net::MemNet::new();
    let (home_node, worker_node) = joined(&net, Opts::named("p3hhome"), worker_opts);
    let flag = SwitchFlag::off(CLUSTER, CLUSTER_OFF);
    follow_switch(&flag, &home_node.node);
    flag.set(true);
    let cluster_ops = ClusterOps::new(
        Ok(home_node.node.clone()),
        flag.clone(),
        home_node.room.clone(),
        home_node.log.clone(),
    );
    assert_eq!(
        cluster_ops.spec(ops::CLUSTER_MEMBER_SET),
        Some(OpSpec::APP),
        "only the app sets a member's switches"
    );

    let (home_id, worker_id) = (home_node.id(), worker_node.id());
    let fabric = cluster_of(
        &[
            (
                home_id.clone(),
                Spec {
                    n: 5,
                    client: true,
                    keeps: true,
                },
            ),
            (
                worker_id.clone(),
                Spec {
                    n: 9,
                    client: false,
                    keeps: false,
                },
            ),
        ],
        &home_id,
    );
    fabric.knows_consent.lock().unwrap().insert(home_id.clone());
    let (home, machine) = (fabric.machine(&home_id), fabric.machine(&worker_id));
    let w = side_with(Some(machine.rig.ops.clone()), worker(true), board);

    let reply = w.ask("acting").unwrap();
    let id = reply["request"]["id"].as_str().unwrap().to_string();
    let item = item_id_for(&machine.rig.ops, &id);
    settle(&fabric);
    let qualified = format!("{worker_id}/{item}");
    let shown = home
        .rig
        .ops
        .items()
        .into_iter()
        .find(|i| i.id == qualified)
        .expect("the home shows the Worker's request");
    assert_eq!(shown.switch.as_deref(), Some("acting"));
    assert!(shown.open.is_none());

    let status = cluster_ops
        .call(&the_app(), ops::CLUSTER_STATUS, &json!({}))
        .unwrap();
    assert_eq!(member_set_args(&shown, ActionId::Deny, &status), None);
    let done = home
        .rig
        .ops
        .decide(&qualified, ActionId::Allow, "mac")
        .unwrap();
    assert!(done.answered);
    assert_eq!(w.state(&id), "applying");
    let args = member_set_args(&shown, ActionId::Allow, &status).expect("a member.set call");
    assert_eq!(args["node"], json!(worker_id.to_string()));
    assert_eq!(args["consentId"], json!(id));
    assert_eq!(args["desired"]["acting"], json!(true));
    cluster_ops
        .call(&the_app(), ops::CLUSTER_MEMBER_SET, &args)
        .unwrap();

    eventually("the switch on on the Worker", || w.is_on("acting"));
    eventually("the row applied", || w.state(&id) == "applied");
    assert_eq!(w.thens().last().map(String::as_str), Some("applied"));
    assert!(
        w.turns.lock().unwrap().is_empty(),
        "the switch came through desired, not the Worker's own turn"
    );
    assert!(!w.is_on("bridge"));
}
