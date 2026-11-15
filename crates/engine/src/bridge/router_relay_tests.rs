use super::agents;
use super::backend::{BridgeBackend, Devices};
use super::live::{Acting, ForeignLive, LiveBackend, Sources};
use super::model::*;
use super::relay::Relay;
use super::router::{Router, RouterParts};
use super::test_support::{
    data, error_code, is_ok, scripted_cluster, MemoryAudit, MemoryCodes, MemoryDevices,
    MemoryLimiter, Plan, RecordingIo, ScriptedRelay, NOW_MS,
};
use super::wire::BridgeFailure;
use crate::sessions::agent::sample;
use crate::sessions::{Agent, AgentGroup, AgentTarget, NodeRef};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};

const PRIMARY: &str = "0123456789abcdef";
const WORKER: &str = "fedcba9876543210";

fn row(node: &str, name: &str, job: &str) -> Agent {
    let mut agent = sample(AgentTarget::Job("ab12".into()), None);
    agent.id = format!("{node}/{job}");
    agent.name = format!("{name} {job}");
    agent.group = AgentGroup::Working;
    agent.host = node.into();
    agent.node = Some(NodeRef {
        id: node.into(),
        name: name.into(),
        online: true,
    });
    agent
}

struct World {
    agents: Vec<Agent>,
    relay: Option<Arc<ScriptedRelay>>,
}

impl Sources for Arc<World> {
    fn claude_sessions(&self) -> Vec<Session> {
        agents::sessions(&self.agents)
    }
    fn foreign_live(&self, _: &[super::foreign::ForeignRow]) -> ForeignLive {
        ForeignLive::default()
    }
    fn relay(&self) -> Option<Arc<dyn Relay>> {
        self.relay.clone().map(|r| r as Arc<dyn Relay>)
    }
    fn now_ms(&self) -> f64 {
        NOW_MS
    }
}

#[derive(Default)]
struct Recorded {
    sent: Vec<(String, String)>,
    created: Vec<String>,
}

struct Acts(Arc<Mutex<Recorded>>);

impl Acting for Acts {
    fn send_message(&self, text: &str, session: &Session) -> Result<(), BridgeFailure> {
        self.0
            .lock()
            .expect("recorded")
            .sent
            .push((session.id.clone(), text.into()));
        Ok(())
    }
    fn send_now(&self, _: &Session) -> Result<(), BridgeFailure> {
        Ok(())
    }
    fn respond(&self, _: &PendingPrompt, _: &PromptResponse) -> Result<(), BridgeFailure> {
        Ok(())
    }
    fn new_session(&self, cwd: &Path, _: Option<&str>) -> Result<(), BridgeFailure> {
        self.0
            .lock()
            .expect("recorded")
            .created
            .push(cwd.to_string_lossy().into_owned());
        Ok(())
    }
    fn take_over(&self, _: &Session, _: bool) -> Result<TakeoverOutcome, BridgeFailure> {
        Err(BridgeFailure::invalid("no"))
    }
}

struct Phone {
    router: Router,
    hub: Arc<super::hub::Hub>,
    backend: Arc<dyn BridgeBackend>,
    connection: Arc<super::connection::Connection>,
    io: Arc<RecordingIo>,
    recorded: Arc<Mutex<Recorded>>,
    audit: Arc<MemoryAudit>,
    hello: Value,
    _dir: crate::sessions::testutil::TempDir,
}

impl Phone {
    fn ask(&self, request: Value) -> Value {
        self.router.handle(&self.connection, &request.to_string());
        self.io.last_json()
    }
}

fn phone(world: &Arc<World>) -> Phone {
    let dir = crate::sessions::testutil::TempDir::new("bridge-relay");
    let recorded = Arc::new(Mutex::new(Recorded::default()));
    let backend: Arc<dyn BridgeBackend> =
        Arc::new(LiveBackend::new(world.clone(), Acts(recorded.clone())));
    let devices = Arc::new(MemoryDevices::default());
    let audit = Arc::new(MemoryAudit::default());
    let hub = Arc::new(super::hub::Hub::new());
    let router = Router::new(RouterParts {
        backend: backend.clone(),
        devices: devices.clone(),
        codes: Arc::new(MemoryCodes::default()),
        limiter: Arc::new(MemoryLimiter::default()),
        audit: audit.clone(),
        hub: Arc::clone(&hub),
        home: dir.root().to_path_buf(),
        images_dir: dir.path("images"),
        now_ms: Arc::new(|| NOW_MS),
    });
    let (_, token) = Devices::register(devices.as_ref(), "phone", "ios").expect("register");
    let io = Arc::new(RecordingIo::default());
    let connection = hub.connect("192.168.1.20", false, io.clone());
    router.handle(
        &connection,
        &json!({"id": 1, "op": "hello", "token": token}).to_string(),
    );
    let hello = io.last_json();
    Phone {
        router,
        hub,
        backend,
        connection,
        io,
        recorded,
        audit,
        hello,
        _dir: dir,
    }
}

fn world(relay: Option<Arc<ScriptedRelay>>) -> Arc<World> {
    Arc::new(World {
        agents: vec![row(PRIMARY, "m5", "job-7"), row(WORKER, "w1", "job-9")],
        relay,
    })
}

#[test]
fn hello_lists_can_be_master_machines() {
    let relay = ScriptedRelay::new(Some(scripted_cluster()));
    let phone = phone(&world(Some(relay)));
    assert!(is_ok(&phone.hello), "{}", phone.hello);
    let nodes = data(&phone.hello)["cluster"]["nodes"]
        .as_array()
        .expect("nodes")
        .clone();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[1]["name"], "m1");
    assert_eq!(nodes[1]["fingerprint"], "bb".repeat(32));
    assert_eq!(data(&phone.hello)["cluster"]["primary"], PRIMARY);
    assert_eq!(data(&phone.hello)["cluster"]["term"], 3);
}

#[test]
fn old_phone_without_cluster_field_still_connects() {
    for relay in [None, Some(ScriptedRelay::new(None))] {
        let phone = phone(&world(relay));
        assert!(is_ok(&phone.hello), "{}", phone.hello);
        assert!(data(&phone.hello).get("cluster").is_none());
        assert_eq!(data(&phone.hello)["version"], 1);
        let reply = phone.ask(json!({"id": 2, "op": "sessions.list"}));
        assert!(is_ok(&reply), "{reply}");
        assert!(data(&reply).get("stale").is_none());
    }
}

#[test]
fn standby_relays_to_reachable_primary() {
    let relay = ScriptedRelay::new(Some(scripted_cluster()));
    relay.plan(Plan::Relayed);
    let phone = phone(&world(Some(relay.clone())));
    let session = format!("{PRIMARY}/job-7");
    let reply =
        phone.ask(json!({"id": 2, "op": "session.message", "sessionId": session, "text": "go on"}));
    assert!(is_ok(&reply), "{reply}");
    assert_eq!(data(&reply), json!({"sent": true, "relayed": true}));
    assert_eq!(
        phone.recorded.lock().expect("recorded").sent,
        vec![(session, "go on".to_string())]
    );

    relay.plan(Plan::Done(json!({"started": true, "relayed": true})));
    let reply = phone
        .ask(json!({"id": 3, "op": "session.new", "cwd": phone._dir.root().to_string_lossy()}));
    assert_eq!(data(&reply), json!({"started": true, "relayed": true}));
    assert!(
        phone.recorded.lock().expect("recorded").created.is_empty(),
        "the primary starts it, not this machine"
    );
}

#[test]
fn worker_session_goes_straight_to_worker() {
    let relay = ScriptedRelay::new(Some(scripted_cluster()));
    relay.plan(Plan::Local);
    let phone = phone(&world(Some(relay.clone())));
    let session = format!("{WORKER}/job-9");
    let reply =
        phone.ask(json!({"id": 2, "op": "session.message", "sessionId": session, "text": "hi"}));
    assert_eq!(data(&reply), json!({"sent": true}));
    assert_eq!(phone.recorded.lock().expect("recorded").sent.len(), 1);
    let calls = relay.calls.lock().expect("calls").clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].2, "session.message");
}

#[test]
fn unreachable_primary_holds_in_order() {
    let relay = ScriptedRelay::new(Some(scripted_cluster()));
    relay.plan(Plan::Hold);
    let phone = phone(&world(Some(relay.clone())));
    let session = format!("{PRIMARY}/job-7");
    let subscribed = phone.ask(json!({"id": 2, "op": "subscribe", "held": true}));
    assert!(is_ok(&subscribed), "{subscribed}");

    let first =
        phone.ask(json!({"id": 3, "op": "session.message", "sessionId": session, "text": "one"}));
    assert_eq!(data(&first)["held"]["id"], "held-1");
    assert_eq!(data(&first)["held"]["at"], NOW_MS as i64);
    assert!(data(&first).get("sent").is_none());
    let second =
        phone.ask(json!({"id": 4, "op": "session.message", "sessionId": session, "text": "two"}));
    assert_eq!(data(&second)["held"]["id"], "held-2");
    assert!(phone.recorded.lock().expect("recorded").sent.is_empty());

    let listed = phone.ask(json!({"id": 5, "op": "held.list"}));
    let entries = data(&listed)["entries"]
        .as_array()
        .expect("entries")
        .clone();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["summary"], "one");
    assert_eq!(entries[1]["summary"], "two");
    assert_eq!(entries[0]["sessionId"], session);
    assert_eq!(entries[0]["waiting"], false);

    let pushed: Vec<Value> = phone
        .io
        .texts()
        .iter()
        .filter_map(|t| serde_json::from_str::<Value>(t).ok())
        .filter(|v| v["event"] == "held.changed")
        .collect();
    assert!(!pushed.is_empty(), "held.changed is pushed");
    assert_eq!(
        pushed.last().expect("event")["entries"]
            .as_array()
            .expect("entries")
            .len(),
        2
    );
}

#[test]
fn held_cancel_and_confirm_answer_for_the_paired_phone() {
    let relay = ScriptedRelay::new(Some(scripted_cluster()));
    relay.plan(Plan::Hold);
    let phone = phone(&world(Some(relay.clone())));
    let session = format!("{PRIMARY}/job-7");
    phone.ask(json!({"id": 2, "op": "session.message", "sessionId": session, "text": "one"}));
    let cancelled = phone.ask(json!({"id": 3, "op": "held.cancel", "heldId": "held-1"}));
    assert_eq!(data(&cancelled), json!({"cancelled": true}));
    let again = phone.ask(json!({"id": 4, "op": "held.cancel", "heldId": "held-1"}));
    assert_eq!(data(&again), json!({"cancelled": false}));
    relay
        .delivered
        .lock()
        .expect("delivered")
        .push("held-9".into());
    let done = phone.ask(json!({"id": 5, "op": "held.cancel", "heldId": "held-9"}));
    assert_eq!(data(&done), json!({"cancelled": false, "delivered": true}));
    let confirm = phone.ask(json!({"id": 6, "op": "held.confirm", "heldId": "nothing"}));
    assert_eq!(data(&confirm), json!({"confirmed": false}));
    let missing = phone.ask(json!({"id": 7, "op": "held.cancel"}));
    assert_eq!(error_code(&missing), Some("invalid"));
}

#[test]
fn held_ops_without_a_relay_are_empty() {
    let phone = phone(&world(None));
    let listed = phone.ask(json!({"id": 2, "op": "held.list"}));
    assert_eq!(data(&listed), json!({"entries": []}));
    let cancel = phone.ask(json!({"id": 3, "op": "held.cancel", "heldId": "x"}));
    assert_eq!(data(&cancel), json!({"cancelled": false}));
}

#[test]
fn a_held_command_is_audited_like_the_command() {
    let relay = ScriptedRelay::new(Some(scripted_cluster()));
    relay.plan(Plan::Hold);
    let phone = phone(&world(Some(relay)));
    let reply = phone.ask(json!({"id": 2, "op": "session.message", "sessionId": format!("{PRIMARY}/job-7"), "text": "hello"}));
    assert!(data(&reply).get("held").is_some(), "{reply}");
    let audited = phone.audit.recent();
    assert_eq!(audited.len(), 1);
    assert_eq!(audited[0].op, "message");
    assert_eq!(audited[0].length, Some(5));
}

#[test]
fn reads_are_answered_from_the_set_marked_stale() {
    let relay = ScriptedRelay::new(Some(scripted_cluster()));
    *relay.stale.lock().expect("stale") = Some(1_793_541_000_000);
    let phone = phone(&world(Some(relay.clone())));
    let list = phone.ask(json!({"id": 2, "op": "sessions.list"}));
    assert_eq!(data(&list)["stale"], 1_793_541_000_000_i64);
    assert_eq!(
        data(&list)["sessions"].as_array().expect("sessions").len(),
        2
    );
    *relay.stale.lock().expect("stale") = None;
    let live = phone.ask(json!({"id": 3, "op": "sessions.list"}));
    assert!(data(&live).get("stale").is_none());
}

#[test]
fn a_connection_that_never_subscribed_to_held_gets_no_push() {
    let relay = ScriptedRelay::new(Some(scripted_cluster()));
    relay.plan(Plan::Hold);
    let phone = phone(&world(Some(relay)));
    phone.ask(json!({"id": 2, "op": "session.message", "sessionId": format!("{PRIMARY}/job-7"), "text": "x"}));
    phone.hub.tick(phone.backend.as_ref());
    let any = phone
        .io
        .texts()
        .iter()
        .filter_map(|t| serde_json::from_str::<Value>(t).ok())
        .any(|v| v["event"] == "held.changed");
    assert!(!any);
}
