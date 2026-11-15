use super::*;
use crate::cluster_redact::JobMasks;
use crate::cluster_testkit::{Dir, FakeCalls};
use crate::grant_broker::{Audit, GrantBroker, Link, Notices, Release, Vault};
use crate::grant_janitor::GrantJanitor;
use crate::grant_roots::PENDING_MS;
use crate::grant_wiring::GrantParts;
use crate::shell_jobs::{Reporter, ShellJobs};
use crate::vault_ops::{Looked, ReleaseFor, Released, Still};
use notchling_engine::cluster::core::{ATTACH_CALL_MS, CALL_MS};
use notchling_engine::vault::{Kind, VaultEntry};
use notchling_local_api::ops;
use std::sync::atomic::{AtomicI64, Ordering};

fn deliver(rig: &Rig, job: &str, name: &str, value: &str) -> Result<Value, ApiError> {
    rig.uses
        .deliver(&from_vault(), &deliver_args(job, name, "V", value))
}

fn use_names(rig: &Rig, pid: u32, names: &[&str]) -> Result<Value, ApiError> {
    rig.uses
        .use_secrets(
            &caller(pid),
            &json!({"names": names, "folder": "/work", "command": "make"}),
        )
        .expect("the worker path answers")
}

fn has(rig: &Rig, job: &str, name: &str) -> bool {
    rig.store.get(job, name, real_now()).unwrap().is_some()
}

fn janitor_at(rig: &Rig, clock: Clock) -> Arc<GrantJanitor> {
    GrantJanitor::new(
        rig.store.clone(),
        (rig.masks.clone(), rig.marks.clone(), rig.roots.clone()),
        rig.fake.clone(),
        clock,
    )
}

fn janitor(rig: &Rig) -> Arc<GrantJanitor> {
    janitor_at(rig, Arc::new(real_now))
}

fn clock_at(start: i64) -> (Arc<AtomicI64>, Clock) {
    let now = Arc::new(AtomicI64::new(start));
    let reading = now.clone();
    (now, Arc::new(move || reading.load(Ordering::SeqCst)))
}

fn from(node: &str, tier: OriginTier) -> Origin {
    Origin {
        node: id(node),
        tier,
    }
}

#[test]
fn a_secret_delivered_before_the_job_starts_reaches_a_worker_built_from_a_sync() {
    let rig = rig();
    deliver(&rig, JOB, "api-key", "NL-MARKER-1").expect("accepted before the job starts here");
    rig.roots.set(JOB, 500);
    let out = use_names(&rig, 700, &["api-key"]).expect("the job uses its grant");
    assert!(
        out["env"]["V"] == "NL-MARKER-1",
        "the value reaches the job"
    );
}

#[test]
fn vault_use_from_a_running_job_answers_on_a_worker_whose_state_has_no_jobs() {
    let rig = rig();
    rig.roots.set(JOB, 500);
    put(&rig, JOB, "api-key", "V", "NL-MARKER-1");
    let out = use_names(&rig, 700, &["api-key"]).expect("answered from the job's grant");
    assert!(
        out["env"]["V"] == "NL-MARKER-1",
        "the value reaches the job"
    );
    assert_eq!(*rig.fake.used.lock().unwrap(), [(JOB.to_string(), true)]);
}

#[test]
fn the_janitor_keeps_the_grants_of_a_job_running_here_though_the_worker_state_has_no_jobs() {
    let rig = rig();
    rig.roots.set(JOB, 500);
    put(&rig, JOB, "api-key", "V", "NL-MARKER-1");
    janitor(&rig).tick().expect("swept");
    assert!(has(&rig, JOB, "api-key"));
}

const _: () = assert!(PENDING_MS > ATTACH_CALL_MS as i64);
const _: () = assert!(PENDING_MS < GRANT_TTL_MS);

#[test]
fn the_pending_window_is_the_attach_call_plus_one_call() {
    assert_eq!(PENDING_MS, (ATTACH_CALL_MS + CALL_MS) as i64);
}

#[test]
fn a_pending_grant_becomes_the_jobs_grant_when_its_shell_job_starts() {
    let rig = rig();
    deliver(&rig, JOB, "api-key", "NL-MARKER-1").expect("held pending");
    assert!(rig.roots.is_pending(JOB, real_now()));
    let janitor = janitor(&rig);
    janitor.tick().expect("swept");
    assert!(
        has(&rig, JOB, "api-key"),
        "a pending grant survives a sweep"
    );
    let marked = rig.marks.marked();
    assert_eq!(marked.len(), 1);
    assert!(marked[0].keys.is_empty(), "no row to key before it starts");
    assert_eq!(marked[0].ended_ms, None, "a pending job hasn't ended");
    assert!(!rig.masks.is_ended(JOB));
    assert!(
        use_names(&rig, 700, &["api-key"]).is_err(),
        "no process yet"
    );

    rig.roots.set(JOB, 500);
    assert!(!rig.roots.is_pending(JOB, real_now()), "the start ends it");
    assert!(janitor.changed(), "the start is a change");
    janitor.tick().expect("swept");
    assert!(has(&rig, JOB, "api-key"));
    assert_eq!(rig.marks.marked()[0].keys, [format!("shell:{JOB}")]);
    let out = use_names(&rig, 700, &["api-key"]).expect("used");
    assert!(out["env"]["V"] == "NL-MARKER-1");
    assert!(rig.fake.asked.lock().unwrap().is_empty(), "nothing asked");
}

#[test]
fn a_pending_grant_whose_job_never_starts_is_swept_when_the_window_passes() {
    let start = real_now();
    let (now, clock) = clock_at(start);
    let rig = rig_at(clock.clone(), "worker");
    deliver(&rig, JOB, "api-key", "NL-MARKER-1").expect("held pending");
    let janitor = janitor_at(&rig, clock);
    now.store(start + PENDING_MS - 1, Ordering::SeqCst);
    janitor.tick().expect("swept");
    assert!(has(&rig, JOB, "api-key"), "still inside the window");
    assert_eq!(rig.marks.marked()[0].ended_ms, None);
    assert!(rig.roots.is_pending(JOB, start + PENDING_MS - 1));

    now.store(start + PENDING_MS, Ordering::SeqCst);
    assert!(janitor.changed(), "the window passing is a change");
    janitor.tick().expect("swept");
    assert!(!has(&rig, JOB, "api-key"), "the window is over");
    assert!(!rig.roots.is_pending(JOB, start + PENDING_MS));
    assert_eq!(rig.marks.marked()[0].ended_ms, Some(start + PENDING_MS));
    assert!(rig.masks.is_ended(JOB), "its masks end with it");
    assert_eq!(rig.masks.state(JOB), JobMasks::Masked, "and are kept");
    assert!(use_names(&rig, 700, &["api-key"]).is_err());
}

#[test]
fn a_deliver_after_the_window_starts_a_new_window() {
    let start = real_now();
    let (now, clock) = clock_at(start);
    let rig = rig_at(clock.clone(), "worker");
    deliver(&rig, JOB, "one", "NL-MARKER-1").expect("held pending");
    now.store(start + PENDING_MS + 5, Ordering::SeqCst);
    deliver(&rig, JOB, "two", "NL-MARKER-2").expect("held pending again");
    let later = start + PENDING_MS + 5 + PENDING_MS - 1;
    assert!(rig.roots.is_pending(JOB, later));
    now.store(later, Ordering::SeqCst);
    janitor_at(&rig, clock).tick().expect("swept");
    assert!(has(&rig, JOB, "one") && has(&rig, JOB, "two"));
}

#[test]
fn a_revoke_of_a_pending_grant_deletes_it_and_bars_a_deliver_that_arrived_before() {
    let rig = rig();
    deliver(&rig, JOB, "one", "NL-MARKER-1").expect("held pending");
    deliver(&rig, JOB, "two", "NL-MARKER-2").expect("held pending");
    let arrived = rig.uses.arrived();
    rig.uses
        .revoke(&from_vault(), &json!({"job": JOB, "names": ["one"]}))
        .expect("revoked");
    assert!(!has(&rig, JOB, "one") && has(&rig, JOB, "two"));
    assert!(rig.roots.is_pending(JOB, real_now()), "the other is held");
    rig.uses
        .revoke(&from_vault(), &json!({"job": JOB}))
        .expect("revoked");
    assert!(!has(&rig, JOB, "one") && !has(&rig, JOB, "two"));
    assert!(!rig.roots.is_pending(JOB, real_now()), "nothing is held");
    let late = deliver_args(JOB, "three", "V", "NL-MARKER-3");
    let err = rig
        .uses
        .deliver_at(&from_vault(), &late, arrived)
        .expect_err("overtaken by the revoke");
    assert!(!err.message.contains("NL-MARKER"));
    assert!(!has(&rig, JOB, "three"));
    assert!(!rig.roots.is_pending(JOB, real_now()));
    deliver(&rig, JOB, "three", "NL-MARKER-3").expect("a later deliver is held again");
    assert!(rig.roots.is_pending(JOB, real_now()));
}

#[test]
fn a_pending_deliver_from_anyone_but_the_vault_node_that_is_home_is_still_refused() {
    let rig = rig();
    let args = deliver_args(JOB, "api-key", "V", "NL-MARKER-1");
    for origin in [
        from("other", OriginTier::Node),
        from("worker", OriginTier::Node),
        from("vault", OriginTier::Agent),
        from("vault", OriginTier::App),
    ] {
        let err = rig.uses.deliver(&origin, &args).expect_err("refused");
        assert_eq!(err.code, ErrorCode::Forbidden);
        assert!(!err.message.contains("NL-MARKER"));
    }
    *rig.fake.state.lock().unwrap() = Some(worker_state_with_home("other"));
    assert!(
        deliver(&rig, JOB, "api-key", "NL-MARKER-1").is_err(),
        "the vault node isn't home"
    );
    assert!(!has(&rig, JOB, "api-key"));
    assert!(!rig.roots.is_pending(JOB, real_now()));
    assert!(rig.marks.list().is_empty());
    assert_eq!(rig.masks.state(JOB), JobMasks::Never);
}

#[test]
fn a_node_whose_state_lists_the_job_takes_it_as_live_and_not_pending() {
    let rig = rig_running_both();
    deliver(&rig, JOB, "api-key", "NL-MARKER-1").expect("stored");
    assert!(!rig.roots.is_pending(JOB, real_now()), "not held pending");
    assert_eq!(rig.marks.marked()[0].keys, [format!("shell:{JOB}")]);
    rig.roots.set(JOB, 500);
    let out = use_names(&rig, 700, &["api-key"]).expect("used");
    assert!(out["env"]["V"] == "NL-MARKER-1");
    let janitor = janitor(&rig);
    janitor.tick().expect("swept");
    assert!(has(&rig, JOB, "api-key"), "the state keeps it alive");
    *rig.fake.state.lock().unwrap() = Some(cluster("vault", &[job(JOB, "worker", JobState::Done)]));
    janitor.tick().expect("swept");
    assert!(!has(&rig, JOB, "api-key"), "the state ends it");
}

#[test]
fn a_job_that_ends_here_loses_its_grants_at_the_next_sweep_and_takes_no_more() {
    let rig = rig();
    deliver(&rig, JOB, "api-key", "NL-MARKER-1").expect("held pending");
    rig.roots.set(JOB, 500);
    let janitor = janitor(&rig);
    janitor.tick().expect("swept");
    assert!(has(&rig, JOB, "api-key"));
    rig.roots.end(JOB);
    assert!(janitor.changed(), "a local end is a change");
    janitor.tick().expect("swept");
    assert!(!has(&rig, JOB, "api-key"), "the job ended here");
    assert!(rig.masks.is_ended(JOB));
    assert!(rig.marks.marked()[0].ended_ms.is_some());
    let err = deliver(&rig, JOB, "api-key", "NL-MARKER-1").expect_err("ended");
    assert!(err.message.contains("isn't running"));
    let err = use_names(&rig, 700, &["api-key"]).expect_err("no root");
    assert_eq!(err.message, NOT_A_JOB);
}

#[test]
fn grants_of_a_job_nobody_here_knows_are_swept() {
    let rig = rig();
    put(&rig, JOB, "api-key", "V", "NL-MARKER-1");
    janitor(&rig).tick().expect("swept");
    assert!(!has(&rig, JOB, "api-key"));
}

#[test]
fn without_a_cluster_state_even_a_job_running_here_loses_its_grants() {
    let rig = rig();
    rig.roots.set(JOB, 500);
    put(&rig, JOB, "api-key", "V", "NL-MARKER-1");
    *rig.fake.state.lock().unwrap() = None;
    janitor(&rig).tick().expect("swept");
    assert!(!has(&rig, JOB, "api-key"));
}

#[test]
fn the_thread_notices_a_local_end_without_waiting_for_the_slow_sweep() {
    let rig = rig();
    rig.roots.set(JOB, 500);
    put(&rig, JOB, "api-key", "V", "NL-MARKER-1");
    let janitor = janitor(&rig);
    janitor.spawn();
    std::thread::sleep(Duration::from_millis(300));
    assert!(has(&rig, JOB, "api-key"), "running here");
    rig.roots.end(JOB);
    let until = std::time::Instant::now() + Duration::from_secs(10);
    while has(&rig, JOB, "api-key") && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
    janitor.close();
    assert!(!has(&rig, JOB, "api-key"), "swept after the local end");
}

struct Starts {
    reply: Result<Value, ApiError>,
    seen: Mutex<Vec<String>>,
}

impl LocalCalls for Starts {
    fn call(&self, op: &str, _args: &Value, _origin: &Origin) -> Result<Value, ApiError> {
        self.seen.lock().unwrap().push(op.to_string());
        self.reply.clone()
    }
}

fn starts(reply: Result<Value, ApiError>) -> Arc<Starts> {
    Arc::new(Starts {
        reply,
        seen: Mutex::new(Vec::new()),
    })
}

fn from_home() -> Origin {
    from("vault", OriginTier::App)
}

#[test]
fn a_claude_start_op_from_the_home_turns_the_pending_grant_into_the_jobs_grant() {
    let rig = rig();
    deliver(&rig, JOB, "api-key", "NL-MARKER-1").expect("held pending");
    let inner = starts(Ok(json!({"jobId": "c1"})));
    let calls = UseCalls::new(inner.clone(), rig.uses.clone());
    let args = json!({"folder": "/work", "job": JOB});
    calls
        .call(ops::SESSION_NEW, &args, &from_home())
        .expect("started");
    assert_eq!(*inner.seen.lock().unwrap(), [ops::SESSION_NEW]);
    assert!(rig.roots.is_live(JOB));
    assert!(!rig.roots.is_pending(JOB, real_now()));
    assert_eq!(rig.roots.keys(JOB), ["c1".to_string()]);
    let janitor = janitor(&rig);
    janitor.tick().expect("swept");
    assert!(has(&rig, JOB, "api-key"), "a started job keeps its grant");
    assert_eq!(rig.marks.marked()[0].keys, ["c1".to_string()]);
    assert_eq!(rig.masks.job_for_key("c1"), Some(JOB.to_string()));
    assert_eq!(rig.marks.marked()[0].ended_ms, None);
}

#[test]
fn an_antigravity_start_op_is_noted_the_same_way() {
    let rig = rig();
    deliver(&rig, JOB, "api-key", "NL-MARKER-1").expect("held pending");
    let calls = UseCalls::new(starts(Ok(json!({"id": "agy:1"}))), rig.uses.clone());
    let args = json!({"draft_folder": "/work", "job": JOB, "text": "go"});
    calls
        .call(ops::AGY_SEND, &args, &from_home())
        .expect("started");
    assert!(rig.roots.is_live(JOB));
    assert_eq!(rig.roots.keys(JOB), ["agy:1".to_string()]);
}

#[test]
fn a_start_op_that_fails_ends_the_job_and_removes_its_grants() {
    let rig = rig();
    deliver(&rig, JOB, "api-key", "NL-MARKER-1").expect("held pending");
    let failing = starts(Err(ApiError::new(ErrorCode::Failed, "no trust")));
    let calls = UseCalls::new(failing, rig.uses.clone());
    let args = json!({"folder": "/work", "job": JOB});
    let err = calls
        .call(ops::SESSION_NEW, &args, &from_home())
        .expect_err("the op's own error comes back");
    assert_eq!(err.message, "no trust");
    assert!(!has(&rig, JOB, "api-key"));
    assert!(!rig.roots.is_live(JOB));
    assert!(!rig.roots.is_pending(JOB, real_now()));
    assert!(rig.roots.is_ended(JOB));
    let err = deliver(&rig, JOB, "api-key", "NL-MARKER-1").expect_err("ended");
    assert!(err.message.contains("isn't running"));
}

#[test]
fn a_start_op_that_does_not_come_from_the_home_with_a_job_is_not_noted() {
    let rig = rig();
    let calls = UseCalls::new(starts(Ok(json!({"jobId": "c1"}))), rig.uses.clone());
    let args = json!({"folder": "/work", "job": JOB});
    for origin in [
        from("other", OriginTier::App),
        from("vault", OriginTier::Agent),
        from("vault", OriginTier::Node),
    ] {
        calls
            .call(ops::SESSION_NEW, &args, &origin)
            .expect("still runs");
    }
    for bad in [json!({"folder": "/work"}), json!({"job": "j-nothex"})] {
        calls
            .call(ops::SESSION_NEW, &bad, &from_home())
            .expect("still runs");
    }
    assert!(!rig.roots.is_live(JOB));
    assert!(rig.roots.rooted().is_empty());
    *rig.fake.state.lock().unwrap() = None;
    calls
        .call(ops::SESSION_NEW, &args, &from_home())
        .expect("still runs");
    assert!(!rig.roots.is_live(JOB), "no cluster, nothing to trust");
}

#[test]
fn other_ops_pass_through_untouched() {
    let rig = rig();
    let inner = Arc::new(FakeCalls::default());
    let calls = UseCalls::new(inner.clone(), rig.uses.clone());
    calls
        .call("sessions.list", &json!({"job": JOB}), &from_home())
        .expect("passed");
    assert_eq!(inner.calls.lock().unwrap().len(), 1);
    assert!(!rig.roots.is_live(JOB));
}

#[test]
fn a_job_known_only_by_a_start_is_ended_by_the_state_when_the_state_says_so() {
    let rig = rig();
    assert!(rig.roots.begin(JOB));
    put(&rig, JOB, "api-key", "V", "NL-MARKER-1");
    let janitor = janitor(&rig);
    janitor.tick().expect("swept");
    assert!(has(&rig, JOB, "api-key"));
    *rig.fake.state.lock().unwrap() = Some(cluster("vault", &[job(JOB, "worker", JobState::Done)]));
    janitor.tick().expect("swept");
    assert!(!has(&rig, JOB, "api-key"));
    assert!(rig.roots.is_ended(JOB));
}

struct Quiet;

impl Reporter for Quiet {
    fn report(&self, _job: &str, _state: JobState, _detail: Option<String>) {}
}

struct Pids(Mutex<BTreeMap<u32, u32>>);

impl Processes for Pids {
    fn parent(&self, pid: u32) -> Option<u32> {
        self.0.lock().unwrap().get(&pid).copied()
    }
    fn name(&self, _pid: u32) -> Option<String> {
        None
    }
    fn cwd(&self, _pid: u32) -> Option<PathBuf> {
        None
    }
    fn resolve(&self, path: &Path) -> Option<PathBuf> {
        Some(path.to_path_buf())
    }
}

struct Closing(Arc<ShellJobs>);

impl Drop for Closing {
    fn drop(&mut self) {
        self.0.close();
    }
}

struct Wire(Arc<UseCalls>);

impl Link for Wire {
    fn call(&self, node: &NodeId, op: &str, args: Value) -> Result<Value, ApiError> {
        assert_eq!(*node, id("worker"), "a deliver goes to the job's node");
        self.0.call(op, &args, &from_vault())
    }
}

struct Marker;

fn token(name: &str) -> VaultEntry {
    VaultEntry {
        ask: Some(false),
        expires: None,
        how_to: None,
        kind: Kind::Token,
        name: name.to_string(),
        note: String::new(),
        secret_var: "TOKEN".to_string(),
        updated: String::new(),
        url: None,
        user_var: String::new(),
    }
}

impl Vault for Marker {
    fn lookup(&self, names: &[String]) -> Result<Vec<Looked>, ApiError> {
        Ok(names
            .iter()
            .map(|_| Looked {
                exists: true,
                asks: false,
            })
            .collect())
    }

    fn release(
        &self,
        _who: &ReleaseFor,
        names: &[String],
        _still: &Still<'_>,
    ) -> Result<Released, ApiError> {
        let values = names
            .iter()
            .map(|name| {
                let env = BTreeMap::from([("TOKEN".to_string(), "NL-MARKER-7".to_string())]);
                (token(name), encode_env(&env).expect("env"))
            })
            .collect();
        Ok(Released {
            values,
            asked: false,
            by: None,
            device: None,
            waited_ms: None,
        })
    }
}

impl Audit for Marker {
    fn released(&self, _release: &Release<'_>) -> Result<(), String> {
        Ok(())
    }

    fn refused(&self, _node: &NodeId, _who: &ReleaseFor, _name: &str) {}
}

impl Notices for Marker {
    fn waiting(&self, _job: &Job, _node_name: &str, _vault_name: &str) {}

    fn clear(&self, _job: &str) {}
}

#[test]
fn a_secret_attached_at_dispatch_reaches_a_shell_job_through_the_worker_wiring() {
    let cluster_dir = Dir::new("worker-wiring-cluster");
    let data = Dir::new("worker-wiring-data");
    let work = Dir::new("worker-wiring-work");
    let parts = GrantParts::new(cluster_dir.path());
    let shells = ShellJobs::new(data.path().to_path_buf(), Arc::new(Quiet))
        .with_grants(parts.shell())
        .with_grace(Duration::from_millis(300));
    let _closing = Closing(shells.clone());
    let worker = Arc::new(Fake::default());
    *worker.me.lock().unwrap() = Some(id("worker"));
    *worker.state.lock().unwrap() = Some(worker_state());
    let pids = Arc::new(Pids(Mutex::new(BTreeMap::new())));
    let uses = Arc::new(GrantUse::new(
        parts.store.clone(),
        parts.parts(),
        worker.clone(),
        (worker.clone(), worker.clone()),
        (pids.clone(), Arc::new(real_now)),
    ));
    let calls = Arc::new(UseCalls::new(Arc::new(FakeCalls::default()), uses.clone()));
    let janitor = GrantJanitor::new(
        parts.store.clone(),
        parts.parts(),
        worker.clone(),
        Arc::new(real_now),
    );

    let mut attached = job(JOB, "worker", JobState::Dispatching);
    attached.spec.secrets = vec!["api-key".to_string()];
    let home = Arc::new(Fake::default());
    *home.me.lock().unwrap() = Some(id("vault"));
    *home.state.lock().unwrap() = Some(cluster("vault", &[attached.clone()]));
    let marker = Arc::new(Marker);
    let broker = GrantBroker::new(
        (marker.clone(), Arc::new(Wire(calls)), marker.clone()),
        marker,
        home,
        Arc::new(real_now),
    );
    let reply = broker
        .attach(&json!({
            "job": attached,
            "member": member("worker"),
            "vaultName": "vault",
            "vaultPrimary": true,
        }))
        .expect("the home attaches before it starts the tool");
    assert_eq!(reply["delivered"], json!(["api-key"]));
    assert!(parts.roots.is_pending(JOB, real_now()), "not started yet");
    assert!(parts
        .store
        .get(JOB, "api-key", real_now())
        .unwrap()
        .is_some());

    let cwd = work.path().to_string_lossy().into_owned();
    let started = shells
        .run(&json!({"job": JOB, "cwd": cwd, "command": "sleep 60"}))
        .expect("the start op runs here");
    let root = started["pid"].as_u64().expect("a pid") as u32;
    pids.0.lock().unwrap().insert(7000, root);
    assert!(!parts.roots.is_pending(JOB, real_now()), "started");
    janitor.tick().expect("swept");
    assert!(
        parts
            .store
            .get(JOB, "api-key", real_now())
            .unwrap()
            .is_some(),
        "the running job keeps its grant"
    );
    let used = uses
        .use_secrets(
            &caller(7000),
            &json!({"names": ["api-key"], "folder": cwd, "command": "make"}),
        )
        .expect("the worker path answers")
        .expect("the job's own grant");
    assert!(
        used["env"]["TOKEN"] == "NL-MARKER-7",
        "the value reaches it"
    );
    assert!(worker.asked.lock().unwrap().is_empty(), "nothing asked");
    assert_eq!(parts.masks.state(JOB), JobMasks::Masked);

    shells.close();
    assert!(
        parts
            .store
            .get(JOB, "api-key", real_now())
            .unwrap()
            .is_none(),
        "the job's end deletes its grant"
    );
    let refused = uses
        .use_secrets(
            &caller(7000),
            &json!({"names": ["api-key"], "folder": cwd, "command": "make"}),
        )
        .expect("the worker path answers");
    assert!(refused.is_err(), "nothing for an ended job");
}
