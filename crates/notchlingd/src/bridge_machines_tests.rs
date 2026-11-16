use super::machines;
use crate::bridge_cluster::PhoneCluster;
use crate::cluster_prompts::{Job, RemotePrompts};
use crate::cluster_sessions::RemoteSessions;
use crate::cluster_switches::LateHandler;
use crate::every_session_tests::sources;
use crate::limits_ops_tests::TempDir;
use notchling_engine::bridge::live::Sources;
use notchling_engine::bridge::model::ClusterMachine;
use notchling_engine::bridge::relay::{Call, Cancelled, HeldList, Relay, Routed};
use notchling_local_api::{ApiError, Caller, ClientKind, ErrorCode, Handler, OpSpec, Tier};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

const NOW: i64 = 1_793_541_465_000;
const M1: &str = "1111111111111111";
const M3: &str = "3333333333333333";
const M5: &str = "5555555555555555";
const PI: &str = "4444444444444444";

fn member(id: &str, name: &str, can: bool, online: bool, ok: bool) -> Value {
    json!({
        "id": id, "name": name, "canBeMaster": can, "online": online,
        "state": "active", "role": "member",
        "ready": {"ok": ok, "reason": if ok { "" } else { "no repo" }, "checks": []},
    })
}

fn status(replication: Value) -> Value {
    json!({
        "me": M1,
        "cluster": {"home": M5, "term": {"n": 3}},
        "members": [
            member(M5, "m5", true, false, true),
            member(M1, "m1", true, true, true),
            member(M3, "m3", true, true, false),
            member(PI, "pi", false, true, true),
            {"id": "6666666666666666", "name": "gone", "canBeMaster": true, "online": true, "state": "removed"},
        ],
        "replication": replication,
    })
}

fn by_name<'a>(list: &'a [ClusterMachine], name: &str) -> &'a ClusterMachine {
    list.iter().find(|m| m.name == name).expect(name)
}

#[test]
fn the_reached_machine_reports_its_own_set() {
    let list = machines(&status(json!([])), Some(NOW - 120_000), NOW);
    let names: Vec<_> = list.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["m5", "m1", "m3", "pi"]);
    let m1 = by_name(&list, "m1");
    assert_eq!(m1.set_at, Some(NOW - 120_000));
    assert!(m1.can_resume && m1.ready && m1.online && m1.can_be_master);
    assert!(!by_name(&list, "m5").online);
}

#[test]
fn a_machine_with_no_set_cannot_resume() {
    let list = machines(&status(json!([])), None, NOW);
    let m1 = by_name(&list, "m1");
    assert_eq!(m1.set_at, None);
    assert!(!m1.can_resume);
}

#[test]
fn a_machine_that_is_not_ready_cannot_resume_even_with_a_set() {
    let replication = json!([{"node": M3, "setAgeS": 60, "sizeMb": 4, "workedAgeS": 10}]);
    let list = machines(&status(replication), None, NOW);
    let m3 = by_name(&list, "m3");
    assert_eq!(m3.set_at, Some(NOW - 60_000));
    assert!(!m3.ready);
    assert!(!m3.can_resume);
}

#[test]
fn replication_gives_the_other_machines_their_set_time() {
    let replication = json!([
        {"node": M3, "setAgeS": 90, "sizeMb": 4, "workedAgeS": 10},
        {"node": PI, "setAgeS": 30, "sizeMb": 4, "workedAgeS": 10},
        {"node": M1, "setAgeS": null, "sizeMb": null, "workedAgeS": 10},
    ]);
    let list = machines(&status(replication), None, NOW);
    assert_eq!(by_name(&list, "m3").set_at, Some(NOW - 90_000));
    assert_eq!(by_name(&list, "pi").set_at, Some(NOW - 30_000));
    assert!(!by_name(&list, "pi").can_resume);
    assert_eq!(by_name(&list, "m1").set_at, None);
}

#[test]
fn a_status_with_no_members_lists_nothing() {
    assert!(machines(&json!({"cluster": null, "members": []}), None, NOW).is_empty());
    assert!(machines(&json!(null), None, NOW).is_empty());
}

type Seen = Arc<Mutex<Vec<(Tier, ClientKind, String, Value)>>>;

struct Scripted {
    seen: Seen,
    status: Value,
    promote: Result<Value, ApiError>,
}

impl Handler for Scripted {
    fn spec(&self, _: &str) -> Option<OpSpec> {
        None
    }
    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        self.seen
            .lock()
            .unwrap()
            .push((caller.tier, caller.client, op.to_string(), args.clone()));
        match op {
            "cluster.status" => Ok(self.status.clone()),
            "cluster.promote" => self.promote.clone(),
            _ => Ok(json!({})),
        }
    }
}

struct Standby(Option<i64>);

impl Relay for Standby {
    fn cluster(&self) -> Option<Value> {
        None
    }
    fn route(&self, _: &Call<'_>) -> Routed {
        Routed::Local
    }
    fn list(&self, _: &str) -> HeldList {
        HeldList::default()
    }
    fn cancel(&self, _: &str, _: &str) -> Cancelled {
        Cancelled::Unknown
    }
    fn confirm(&self, _: &str, _: &str) -> bool {
        false
    }
    fn stale_ms(&self) -> Option<i64> {
        self.0
    }
}

struct World {
    sources: crate::bridge_backend::DaemonSources,
    seen: Seen,
    _handler: Arc<dyn Handler>,
    _dir: TempDir,
}

fn world(promote: Result<Value, ApiError>, own_set: Option<i64>) -> World {
    let dir = TempDir::new("bridge-machines");
    let seen = Seen::default();
    let handler: Arc<dyn Handler> = Arc::new(Scripted {
        seen: seen.clone(),
        status: status(json!([])),
        promote,
    });
    let late = Arc::new(LateHandler::default());
    late.serve(&handler);
    let room = Arc::new(crate::acting_ops_tests::FakeRoom::default());
    let sessions = Arc::new(RemoteSessions::new(dir.path("mirror")));
    let inline: crate::cluster_prompts::Spawn = Arc::new(|job: Job| {
        job();
        Ok(())
    });
    let prompts = RemotePrompts::with_spawn(room, sessions.clone(), inline);
    let cluster = PhoneCluster::none();
    cluster.attach(late, sessions, prompts);
    cluster.attach_relay(Arc::new(Standby(own_set)));
    let mut source = sources(
        &dir.path("."),
        Arc::new(Mutex::new(json!([]))),
        Arc::new(crate::every_session::EverySession::none()),
    );
    source.cluster = cluster;
    World {
        sources: source,
        seen,
        _handler: handler,
        _dir: dir,
    }
}

#[test]
fn the_backend_lists_machines_from_cluster_status_as_the_phone() {
    let w = world(Ok(json!({})), Some(NOW - 5_000));
    let list = w.sources.cluster_machines();
    assert_eq!(list.len(), 4);
    let seen = w.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        (seen[0].0, seen[0].1, seen[0].2.as_str()),
        (Tier::App, ClientKind::App, "cluster.status")
    );
}

#[test]
fn the_backend_promotes_through_the_daemon_op_and_passes_the_answer() {
    let answer = json!({"needsConfirm": "m5 can't be reached."});
    let w = world(Ok(answer.clone()), None);
    assert_eq!(w.sources.cluster_promote(M1, false).unwrap(), answer);
    assert_eq!(w.sources.cluster_promote(M1, true).unwrap(), answer);
    let seen = w.seen.lock().unwrap();
    assert_eq!(seen[0].2, "cluster.promote");
    assert_eq!(seen[0].0, Tier::App);
    assert_eq!(seen[0].3, json!({"node": M1}));
    assert_eq!(seen[1].3, json!({"node": M1, "confirmed": true}));
}

#[test]
fn a_refused_promote_keeps_the_daemons_words() {
    let w = world(
        Err(ApiError::new(
            ErrorCode::Failed,
            "a promote is already running",
        )),
        None,
    );
    let failure = w.sources.cluster_promote(M1, false).unwrap_err();
    assert!(failure.message.contains("already running"), "{failure:?}");
}

#[test]
fn no_cluster_means_no_machines_and_no_promote() {
    let dir = TempDir::new("bridge-machines-none");
    let source = sources(
        &dir.path("."),
        Arc::new(Mutex::new(json!([]))),
        Arc::new(crate::every_session::EverySession::none()),
    );
    assert!(source.cluster_machines().is_empty());
    let failure = source.cluster_promote(M1, false).unwrap_err();
    assert!(failure.message.contains("cluster"), "{failure:?}");
}
