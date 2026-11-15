use super::*;
use crate::cluster_testkit::Dir;
use notchling_engine::cluster::grants::{encode_env, SecretAllow};
use notchling_engine::cluster::state::{apply, start};
use notchling_engine::cluster::{
    fingerprint, Command, Desired, Job, JobSpec, Member, MemberState, Term, Tool,
};
use notchling_local_api::{ClientKind, Tier};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

pub(crate) const JOB: &str = "j-0123456789ab";
pub(crate) const OTHER_JOB: &str = "j-ba9876543210";

pub(crate) fn id(name: &str) -> NodeId {
    NodeId::from_fingerprint(&fingerprint(name.as_bytes())).expect("a node id")
}

pub(crate) fn member(name: &str) -> Member {
    Member {
        id: id(name),
        name: name.to_string(),
        fingerprint: fingerprint(name.as_bytes()),
        addrs: Vec::new(),
        port: 47823,
        client: false,
        can_be_master: false,
        reference: false,
        accepts_delegation: false,
        headless: false,
        state: MemberState::Active,
        versions: None,
        desired: Desired::default(),
        joined_ms: 0,
        secrets: SecretAllow::new(["any".to_string()], true),
    }
}

pub(crate) fn job(id_: &str, node: &str, state: JobState) -> Job {
    Job {
        id: id_.to_string(),
        key: id_.to_string(),
        spec: JobSpec {
            node: id(node),
            tool: Tool::Shell,
            cwd: "/work".to_string(),
            args: json!({"command": "make"}),
            secrets: Vec::new(),
        },
        state,
        term: Term {
            n: 1,
            node: id("vault"),
        },
        created_ms: 0,
        session: None,
        detail: None,
    }
}

pub(crate) fn cluster(home: &str, jobs: &[Job]) -> ClusterState {
    let mut members = BTreeMap::new();
    for name in ["vault", "worker", "other"] {
        members.insert(id(name), member(name));
    }
    ClusterState {
        id: "c".to_string(),
        name: "c".to_string(),
        term: Term {
            n: 1,
            node: id(home),
        },
        home: id(home),
        vault: id("vault"),
        members,
        jobs: jobs.iter().map(|j| (j.id.clone(), j.clone())).collect(),
        seq: 1,
    }
}

pub(crate) fn sync_from(home: &str) -> Command {
    Command::Sync {
        id: "c".to_string(),
        name: "c".to_string(),
        term: Term {
            n: 1,
            node: id(home),
        },
        home: id(home),
        vault: id("vault"),
        members: ["vault", "worker", "other"].map(member).to_vec(),
    }
}

pub(crate) fn worker_state_with_home(home: &str) -> ClusterState {
    let (mut state, _, _) = start(sync_from(home), 0).expect("a node joins from a sync");
    apply(&mut state, sync_from(home), 1).expect("the home syncs again");
    assert!(state.jobs.is_empty(), "a sync carries no jobs");
    state
}

pub(crate) fn worker_state() -> ClusterState {
    worker_state_with_home("vault")
}

#[derive(Default)]
pub(crate) struct Fake {
    pub state: Mutex<Option<ClusterState>>,
    pub me: Mutex<Option<NodeId>>,
    pub asked: Mutex<Vec<(NodeId, Value)>>,
    pub reply: Mutex<Option<Result<Value, ApiError>>>,
    pub then: Mutex<Option<Box<dyn Fn() + Send>>>,
    pub used: Mutex<Vec<(String, bool)>>,
}

impl View for Fake {
    fn state(&self) -> Option<ClusterState> {
        self.state.lock().unwrap().clone()
    }

    fn me(&self) -> Option<NodeId> {
        self.me.lock().unwrap().clone()
    }
}

impl Asker for Fake {
    fn request(&self, vault: &NodeId, args: Value) -> Result<Value, ApiError> {
        self.asked.lock().unwrap().push((vault.clone(), args));
        if let Some(then) = self.then.lock().unwrap().as_ref() {
            then();
        }
        self.reply
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| Ok(json!({"delivered": []})))
    }
}

impl UseAudit for Fake {
    fn used(&self, _vault: &NodeId, job: &str, ok: bool) {
        self.used.lock().unwrap().push((job.to_string(), ok));
    }
}

#[derive(Default)]
pub(crate) struct Tree(pub BTreeMap<u32, u32>);

impl Processes for Tree {
    fn parent(&self, pid: u32) -> Option<u32> {
        self.0.get(&pid).copied()
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

pub(crate) struct Rig {
    pub dir: Dir,
    pub fake: Arc<Fake>,
    pub store: Arc<GrantStore>,
    pub masks: Arc<TailMasks>,
    pub marks: Arc<GrantMarks>,
    pub roots: Arc<JobRoots>,
    pub uses: Arc<GrantUse>,
}

pub(crate) fn real_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

pub(crate) fn rig_at(clock: Clock, me: &str) -> Rig {
    rig_full(clock, me, Arc::new(|| OnDisk::Nothing))
}

pub(crate) fn rig_full(clock: Clock, me: &str, disk: Disk) -> Rig {
    let dir = Dir::new("grant-use");
    let fake = Arc::new(Fake::default());
    *fake.me.lock().unwrap() = Some(id(me));
    *fake.state.lock().unwrap() = Some(worker_state());
    let store = Arc::new(GrantStore::new(dir.path()));
    let masks = Arc::new(TailMasks::new());
    let marks = Arc::new(GrantMarks::new(dir.path()));
    let roots = Arc::new(JobRoots::new());
    let tree = Tree(BTreeMap::from([(700, 600), (600, 500), (900, 800)]));
    let uses = Arc::new(
        GrantUse::new(
            store.clone(),
            (masks.clone(), marks.clone(), roots.clone()),
            fake.clone(),
            (fake.clone(), fake.clone()),
            (Arc::new(tree), clock),
        )
        .with_disk(disk),
    );
    Rig {
        dir,
        fake,
        store,
        masks,
        marks,
        roots,
        uses,
    }
}

pub(crate) fn rig() -> Rig {
    rig_at(Arc::new(real_now), "worker")
}

pub(crate) fn rig_knowing_jobs(jobs: &[Job]) -> Rig {
    let rig = rig();
    *rig.fake.state.lock().unwrap() = Some(cluster("vault", jobs));
    rig
}

pub(crate) fn rig_running_both() -> Rig {
    rig_knowing_jobs(&[
        job(JOB, "worker", JobState::Running),
        job(OTHER_JOB, "worker", JobState::Running),
    ])
}

pub(crate) fn caller(pid: u32) -> Caller {
    Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
        peer: None,
        pid: Some(pid),
    }
}

pub(crate) fn from_vault() -> Origin {
    Origin {
        node: id("vault"),
        tier: OriginTier::Node,
    }
}

pub(crate) fn env_json(var: &str, value: &str) -> String {
    let env = BTreeMap::from([(var.to_string(), value.to_string())]);
    encode_env(&env).expect("env").expose().to_string()
}

pub(crate) fn deliver_args(job: &str, name: &str, var: &str, value: &str) -> Value {
    json!({
        "job": job,
        "name": name,
        "value": env_json(var, value),
        "expiresMs": real_now() + 3_600_000,
    })
}

pub(crate) fn put(rig: &Rig, job: &str, name: &str, var: &str, value: &str) {
    rig.store
        .put(&GrantFile {
            job: job.to_string(),
            name: name.to_string(),
            expires_ms: real_now() + 3_600_000,
            value: GrantValue::new(env_json(var, value)),
        })
        .expect("stored");
}

fn use_args(names: &[&str]) -> Value {
    json!({"names": names, "folder": "/work", "command": "make"})
}

fn answer(rig: &Rig, pid: u32, names: &[&str]) -> Result<Value, ApiError> {
    rig.uses
        .use_secrets(&caller(pid), &use_args(names))
        .expect("the worker path answers")
}

#[test]
fn use_answers_from_the_jobs_grants() {
    let rig = rig();
    rig.roots.set(JOB, 500);
    put(&rig, JOB, "api-key", "API_TOKEN", "NL-MARKER-1");
    let out = answer(&rig, 700, &["API-KEY"]).expect("answered");
    assert!(
        out["env"]["API_TOKEN"] == "NL-MARKER-1",
        "the variable has its value"
    );
    let hidden = out["hide"].as_object().expect("hide map");
    assert!(hidden.contains_key("NL-MARKER-1"), "the value is hidden");
    assert!(
        hidden.values().all(|m| m == "[secret:api-key]"),
        "the mask names the secret"
    );
    assert!(rig.fake.asked.lock().unwrap().is_empty(), "nothing asked");
    assert_eq!(*rig.fake.used.lock().unwrap(), [(JOB.to_string(), true)]);
}

#[test]
fn caller_outside_any_job_is_refused() {
    let rig = rig();
    rig.roots.set(JOB, 500);
    put(&rig, JOB, "api-key", "API_TOKEN", "NL-MARKER-1");
    let err = answer(&rig, 4242, &["api-key"]).expect_err("refused");
    assert_eq!(err.message, NOT_A_JOB);
    assert_eq!(err.code, ErrorCode::Forbidden);
    let none = rig.uses.use_secrets(
        &Caller {
            pid: None,
            ..caller(1)
        },
        &use_args(&["api-key"]),
    );
    assert_eq!(none.expect("path").expect_err("refused").message, NOT_A_JOB);
    assert!(rig.fake.asked.lock().unwrap().is_empty(), "nothing asked");
}

#[test]
fn caller_in_another_job_gets_only_its_own() {
    let rig = rig();
    rig.roots.set(JOB, 500);
    rig.roots.set(OTHER_JOB, 800);
    put(&rig, JOB, "api-key", "API_TOKEN", "NL-MARKER-1");
    put(&rig, OTHER_JOB, "other-key", "OTHER_TOKEN", "NL-MARKER-2");
    let out = answer(&rig, 900, &["other-key"]).expect("its own");
    assert!(out["env"].get("API_TOKEN").is_none(), "not the other job's");
    assert!(out["env"]["OTHER_TOKEN"] == "NL-MARKER-2", "its own value");
    let err = answer(&rig, 900, &["api-key"]).expect_err("not its grant");
    assert!(
        !err.message.contains("NL-MARKER"),
        "the refusal has no value"
    );
    let asked = rig.fake.asked.lock().unwrap();
    assert_eq!(asked.len(), 1, "the missing name was asked for once");
    assert_eq!(asked[0].1["job"], OTHER_JOB);
}

#[test]
fn missing_name_requests_from_home_once() {
    let rig = rig();
    rig.roots.set(JOB, 500);
    put(&rig, JOB, "held", "HELD", "NL-MARKER-1");
    let store = rig.store.clone();
    *rig.fake.then.lock().unwrap() = Some(Box::new(move || {
        store
            .put(&GrantFile {
                job: JOB.to_string(),
                name: "fresh".to_string(),
                expires_ms: real_now() + 3_600_000,
                value: GrantValue::new(env_json("FRESH", "NL-MARKER-2")),
            })
            .expect("stored");
    }));
    let out = answer(&rig, 700, &["held", "fresh"]).expect("answered");
    assert!(out["env"]["HELD"] == "NL-MARKER-1", "the held name");
    assert!(out["env"]["FRESH"] == "NL-MARKER-2", "the requested name");
    let asked = rig.fake.asked.lock().unwrap();
    assert_eq!(asked.len(), 1, "one request");
    assert_eq!(asked[0].0, id("vault"));
    assert_eq!(asked[0].1["names"], json!(["fresh"]));
    assert_eq!(asked[0].1["job"], JOB);
    assert_eq!(asked[0].1["folder"], "/work");
}

#[test]
fn waiting_and_failed_requests_say_why_and_give_nothing() {
    let rig = rig();
    rig.roots.set(JOB, 500);
    let why = "vault, which holds the vault";
    *rig.fake.reply.lock().unwrap() = Some(Ok(json!({"waiting": why})));
    let err = answer(&rig, 700, &["api-key"]).expect_err("waits");
    assert_eq!(
        err.message,
        format!("secret isn't available yet: waiting for {why}")
    );
    *rig.fake.reply.lock().unwrap() = Some(Err(ApiError::new(
        ErrorCode::Forbidden,
        "api-key isn't allowed on worker",
    )));
    let err = answer(&rig, 700, &["api-key"]).expect_err("refused");
    assert_eq!(err.message, "api-key isn't allowed on worker");
    *rig.fake.reply.lock().unwrap() = Some(Ok(json!({"delivered": ["api-key"]})));
    let err = answer(&rig, 700, &["api-key"]).expect_err("not stored");
    assert!(err.message.contains("wasn't delivered"));
    assert!(rig.fake.used.lock().unwrap().iter().all(|(_, ok)| !ok));
}

#[test]
fn expired_grant_is_not_used() {
    let later = real_now() + 2 * 3_600_000;
    let rig = rig_at(Arc::new(move || later), "worker");
    rig.roots.set(JOB, 500);
    put(&rig, JOB, "api-key", "API_TOKEN", "NL-MARKER-1");
    let err = answer(&rig, 700, &["api-key"]).expect_err("expired");
    assert!(err.message.contains("wasn't delivered"));
    assert_eq!(rig.fake.asked.lock().unwrap().len(), 1, "asked afresh");
}

#[test]
fn an_ended_or_unknown_job_gets_nothing() {
    let rig = rig();
    rig.roots.set(JOB, 500);
    put(&rig, JOB, "api-key", "API_TOKEN", "NL-MARKER-1");
    *rig.fake.state.lock().unwrap() = Some(cluster("vault", &[job(JOB, "worker", JobState::Done)]));
    let err = answer(&rig, 700, &["api-key"]).expect_err("not running");
    assert!(err.message.contains("isn't running"));
    rig.roots.end(JOB);
    let err = answer(&rig, 700, &["api-key"]).expect_err("no root");
    assert_eq!(err.message, NOT_A_JOB);
}

#[test]
fn the_vault_node_and_a_node_in_no_cluster_are_left_to_the_vault() {
    let rig = rig_at(Arc::new(real_now), "vault");
    assert!(rig
        .uses
        .use_secrets(&caller(700), &use_args(&["api-key"]))
        .is_none());
    let rig = rig_at(Arc::new(real_now), "worker");
    *rig.fake.state.lock().unwrap() = None;
    assert!(rig
        .uses
        .use_secrets(&caller(700), &use_args(&["api-key"]))
        .is_none());
}

#[test]
fn bad_arguments_are_refused_before_anything_is_read() {
    let rig = rig();
    rig.roots.set(JOB, 500);
    for args in [
        json!({"names": []}),
        json!({"names": [3]}),
        json!({"folder": "/work"}),
        json!({"names": ["../x"]}),
    ] {
        let got = rig.uses.use_secrets(&caller(700), &args).expect("path");
        assert!(got.is_err(), "refused");
    }
    assert!(rig.fake.asked.lock().unwrap().is_empty());
}

#[path = "grant_deliver_tests.rs"]
mod deliver;
#[path = "grant_fix_tests.rs"]
mod fixes;
#[path = "grant_worker_tests.rs"]
mod worker;
