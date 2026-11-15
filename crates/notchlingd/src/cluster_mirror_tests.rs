use super::{mirror_path, wipe, Clock, FollowOps, Mirror, TailSource, FOLLOW_MS, MAX_FOLLOWS};
use crate::cluster_sessions::{tail, Peer, RemoteSessions, RowSource, TAIL_MAX};
use crate::cluster_testkit::Dir;
use crate::op_group::OpGroup;
use notchling_engine::cluster::NodeId;
use notchling_local_api::{ops, ApiError, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

fn owner() -> NodeId {
    NodeId::parse("00000000000000a1").unwrap()
}

fn me() -> NodeId {
    NodeId::parse("00000000000000b2").unwrap()
}

struct Owner {
    file: PathBuf,
    has_path: bool,
    asked: Mutex<Vec<(u64, Option<String>)>>,
    broken: Mutex<bool>,
}

impl RowSource for Owner {
    fn rows(&self, _node: &NodeId) -> Result<Value, ApiError> {
        let path = if self.has_path {
            json!(self.file.to_string_lossy())
        } else {
            Value::Null
        };
        Ok(json!([{"id": "s1", "transcriptPath": path}]))
    }
}

impl TailSource for Owner {
    fn tail(
        &self,
        _node: &NodeId,
        id: &str,
        from: u64,
        seen: Option<&str>,
    ) -> Result<Value, ApiError> {
        self.asked
            .lock()
            .unwrap()
            .push((from, seen.map(str::to_string)));
        if *self.broken.lock().unwrap() {
            return Err(ApiError::new(ErrorCode::Failed, "offline"));
        }
        let rows = vec![json!({"id": "s1", "transcriptPath": self.file.to_string_lossy()})];
        let mut args = json!({"id": id, "from": from});
        if let Some(seen) = seen {
            args["gen"] = json!(seen);
        }
        tail(&rows, &args)
    }
}

struct Rig {
    dir: Dir,
    owner: Arc<Owner>,
    remote: Arc<RemoteSessions>,
    mirror: Arc<Mirror>,
    now: Arc<AtomicU64>,
}

fn rig_with(has_path: bool) -> Rig {
    let dir = Dir::new("mirror");
    let file = dir.path().join("owner").join("s1.jsonl");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, b"one\n").unwrap();
    let owner = Arc::new(Owner {
        file,
        has_path,
        asked: Mutex::new(Vec::new()),
        broken: Mutex::new(false),
    });
    let remote = Arc::new(RemoteSessions::new(dir.path().join("cluster/mirror")));
    remote.sync(
        &[Peer {
            id: owner_id(),
            name: "w1".into(),
            online: true,
        }],
        owner.as_ref(),
        0,
    );
    let now = Arc::new(AtomicU64::new(1_000));
    let clock: Clock = {
        let now = now.clone();
        Arc::new(move || now.load(Ordering::SeqCst))
    };
    let mirror = Mirror::new(
        dir.path().join("cluster/mirror"),
        remote.clone(),
        owner.clone(),
        clock,
    );
    Rig {
        dir,
        owner,
        remote,
        mirror,
        now,
    }
}

fn owner_id() -> NodeId {
    owner()
}

fn rig() -> Rig {
    rig_with(true)
}

fn qualified() -> String {
    format!("{}/s1", owner())
}

impl Rig {
    fn mirrored(&self) -> PathBuf {
        self.dir
            .path()
            .join("cluster/mirror")
            .join(owner().as_str())
            .join("s1.jsonl")
    }

    fn append(&self, text: &str) {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&self.owner.file)
            .unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }
}

#[test]
fn the_mirror_file_is_named_by_the_owners_file_name_only() {
    let dir = Path::new("/m");
    let node = owner();
    assert_eq!(
        mirror_path(dir, &node, "/home/x/.claude/projects/p/abc.jsonl"),
        Some(dir.join(node.as_str()).join("abc.jsonl"))
    );
    assert_eq!(mirror_path(dir, &node, ""), None);
    assert_eq!(mirror_path(dir, &node, "/"), None);
    assert_eq!(mirror_path(dir, &node, ".."), None);
    assert_eq!(mirror_path(dir, &node, "/a/.."), None);
}

#[test]
fn following_pulls_the_owners_transcript_into_a_private_file() {
    let r = rig();
    let out = r.mirror.follow(&qualified()).unwrap();
    assert_eq!(out["following"], true);
    assert_eq!(out["until"], 1_000 + FOLLOW_MS);
    r.mirror.pull();
    assert_eq!(std::fs::read(r.mirrored()).unwrap(), b"one\n");
    r.append("two\n");
    r.mirror.pull();
    assert_eq!(std::fs::read(r.mirrored()).unwrap(), b"one\ntwo\n");
    let row = r.remote.rows().remove(0);
    assert_eq!(row["transcriptPath"], json!(r.mirrored().to_string_lossy()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&r.mirrored()), 0o600);
        assert_eq!(mode(r.mirrored().parent().unwrap()), 0o700);
    }
}

#[test]
fn mirror_appends_and_resets_on_truncation() {
    let r = rig();
    r.mirror.follow(&qualified()).unwrap();
    r.mirror.pull();
    r.append("two\n");
    r.mirror.pull();
    assert_eq!(std::fs::read(r.mirrored()).unwrap(), b"one\ntwo\n");
    std::fs::write(&r.owner.file, b"x\n").unwrap();
    r.mirror.pull();
    assert_eq!(
        std::fs::read(r.mirrored()).unwrap(),
        b"x\n",
        "a shorter file rewrites the mirror"
    );
    let replaced = r.dir.path().join("owner").join("new.jsonl");
    std::fs::write(&replaced, b"a replacement that is longer than before\n").unwrap();
    std::fs::rename(&replaced, &r.owner.file).unwrap();
    r.mirror.pull();
    assert_eq!(
        std::fs::read(r.mirrored()).unwrap(),
        b"a replacement that is longer than before\n"
    );
}

#[test]
fn a_big_transcript_comes_in_rounds_within_one_pass() {
    let r = rig();
    let body: Vec<u8> = (0..TAIL_MAX * 2 + 10)
        .map(|i| b'a' + (i % 26) as u8)
        .collect();
    std::fs::write(&r.owner.file, &body).unwrap();
    r.mirror.follow(&qualified()).unwrap();
    r.mirror.pull();
    assert_eq!(std::fs::read(r.mirrored()).unwrap(), body);
}

#[test]
fn follow_expires_after_ten_minutes() {
    let r = rig();
    r.mirror.follow(&qualified()).unwrap();
    r.mirror.pull();
    r.now.store(1_000 + FOLLOW_MS - 1, Ordering::SeqCst);
    r.append("two\n");
    r.mirror.pull();
    assert_eq!(std::fs::read(r.mirrored()).unwrap(), b"one\ntwo\n");
    r.append("three\n");
    r.now.store(1_000 + FOLLOW_MS, Ordering::SeqCst);
    r.mirror.pull();
    assert_eq!(
        std::fs::read(r.mirrored()).unwrap(),
        b"one\ntwo\n",
        "a lapsed follow pulls nothing"
    );
    assert_eq!(r.mirror.following(), 0);
}

#[test]
fn following_again_renews_the_lease() {
    let r = rig();
    r.mirror.follow(&qualified()).unwrap();
    r.now.store(1_000 + FOLLOW_MS - 10, Ordering::SeqCst);
    let again = r.mirror.follow(&qualified()).unwrap();
    assert_eq!(again["until"], 1_000 + FOLLOW_MS - 10 + FOLLOW_MS);
    r.now.store(1_000 + FOLLOW_MS + 5, Ordering::SeqCst);
    r.mirror.pull();
    assert_eq!(r.mirror.following(), 1);
    assert_eq!(std::fs::read(r.mirrored()).unwrap(), b"one\n");
}

#[test]
fn only_a_known_session_of_another_node_can_be_followed() {
    let r = rig();
    for id in [
        "s1".to_string(),
        format!("{}/s1", me()),
        format!("{}/nope", owner()),
        format!("{}/s1", NodeId::parse("00000000000000c3").unwrap()),
        String::new(),
    ] {
        assert!(r.mirror.follow(&id).is_err(), "{id}");
    }
    assert!(r.mirror.follow_args(&json!({})).is_err());
    assert!(r.mirror.follow_args(&json!({"id": 4})).is_err());
    assert_eq!(r.mirror.following(), 0);
    let none = rig_with(false);
    assert!(
        none.mirror.follow(&qualified()).is_err(),
        "a session with no transcript has nothing to mirror"
    );
}

#[test]
fn follows_are_capped() {
    let r = rig();
    let rows: Vec<Value> = (0..MAX_FOLLOWS + 1)
        .map(|n| json!({"id": format!("s{n}"), "transcriptPath": format!("/o/s{n}.jsonl")}))
        .collect();
    struct Many(Vec<Value>);
    impl RowSource for Many {
        fn rows(&self, _node: &NodeId) -> Result<Value, ApiError> {
            Ok(Value::Array(self.0.clone()))
        }
    }
    r.remote.mark_dirty(&owner());
    r.remote.sync(
        &[Peer {
            id: owner(),
            name: "w1".into(),
            online: true,
        }],
        &Many(rows),
        2,
    );
    for n in 0..MAX_FOLLOWS {
        r.mirror.follow(&format!("{}/s{n}", owner())).unwrap();
    }
    let err = r
        .mirror
        .follow(&format!("{}/s{}", owner(), MAX_FOLLOWS))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Failed);
    assert!(
        r.mirror.follow(&format!("{}/s0", owner())).is_ok(),
        "a renewal still works"
    );
}

#[cfg(unix)]
#[test]
fn a_link_in_the_mirror_folder_is_never_written_through() {
    let r = rig();
    let target = r.dir.path().join("precious.txt");
    std::fs::write(&target, b"keep").unwrap();
    std::fs::create_dir_all(r.mirrored().parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, r.mirrored()).unwrap();
    r.mirror.follow(&qualified()).unwrap();
    r.mirror.pull();
    assert_eq!(std::fs::read(&target).unwrap(), b"keep");
}

#[test]
fn a_failing_owner_keeps_the_follow_and_the_file() {
    let r = rig();
    r.mirror.follow(&qualified()).unwrap();
    r.mirror.pull();
    *r.owner.broken.lock().unwrap() = true;
    r.append("two\n");
    r.mirror.pull();
    assert_eq!(std::fs::read(r.mirrored()).unwrap(), b"one\n");
    assert_eq!(r.mirror.following(), 1);
    *r.owner.broken.lock().unwrap() = false;
    r.mirror.pull();
    assert_eq!(std::fs::read(r.mirrored()).unwrap(), b"one\ntwo\n");
}

#[test]
fn cluster_follow_is_an_app_op_that_starts_a_follow() {
    let r = rig();
    let group = FollowOps::new(r.mirror.clone());
    assert_eq!(group.spec(ops::CLUSTER_FOLLOW), Some(OpSpec::APP));
    assert_eq!(group.spec("sessions.list"), None);
    let caller = Caller {
        tier: Tier::App,
        client: ClientKind::App,
        peer: None,
        pid: None,
    };
    let out = group
        .call(&caller, ops::CLUSTER_FOLLOW, &json!({"id": qualified()}))
        .unwrap();
    assert_eq!(out["following"], true);
    assert_eq!(r.mirror.following(), 1);
    assert!(group
        .call(&caller, ops::CLUSTER_FOLLOW, &json!({"id": "bare"}))
        .is_err());
}

#[test]
fn wipe_removes_the_mirror_folder_and_never_follows_a_link() {
    let dir = Dir::new("mirror-wipe");
    let cluster = dir.path().join("cluster");
    wipe(&cluster).unwrap();
    let folder = cluster.join("mirror").join(owner().as_str());
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("s1.jsonl"), b"x").unwrap();
    std::fs::write(cluster.join("cluster.db"), b"db").unwrap();
    wipe(&cluster).unwrap();
    assert!(!cluster.join("mirror").exists());
    assert!(
        cluster.join("cluster.db").exists(),
        "the database is persist's"
    );
    #[cfg(unix)]
    {
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("keep"), b"k").unwrap();
        std::os::unix::fs::symlink(&elsewhere, cluster.join("mirror")).unwrap();
        wipe(&cluster).unwrap();
        assert!(elsewhere.join("keep").exists());
        assert!(!cluster.join("mirror").exists());
    }
}

#[test]
fn a_removed_node_loses_its_mirror_with_the_rest_of_its_cluster_state() {
    use crate::cluster_testkit::{eventually, pair, Opts};
    use notchling_cluster_net::MemNet;
    use notchling_engine::cluster::LocalCmd;
    let net = MemNet::new();
    let mut home = Opts::named("home");
    home.client = true;
    let (a, b) = pair(&net, home, Opts::named("worker"));
    let folder = b.cluster_dir().join("mirror").join(a.id().as_str());
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("s1.jsonl"), b"x").unwrap();
    a.node.local(LocalCmd::Remove { node: b.id() }).unwrap();
    eventually("the mirror is wiped", || {
        !b.cluster_dir().join("mirror").exists()
    });
    assert!(b.audit_text().contains("step=wiped"), "{}", b.audit_text());
}

struct OwnerRows(Vec<Value>);

impl crate::cluster_sessions::LocalRows for OwnerRows {
    fn rows(&self) -> Result<Vec<Value>, ApiError> {
        Ok(self.0.clone())
    }
}

struct Nothing;

impl crate::cluster_node::LocalCalls for Nothing {
    fn call(
        &self,
        op: &str,
        _args: &Value,
        _origin: &notchling_engine::cluster::wire::Origin,
    ) -> Result<Value, ApiError> {
        Err(ApiError::new(
            ErrorCode::Failed,
            format!("{op} isn't served here"),
        ))
    }
}

#[test]
fn a_followed_transcript_crosses_the_node_wire() {
    use crate::cluster_sessions::{peers_of, NodeSource, SessionCalls};
    use crate::cluster_testkit::{pair, Opts};
    use notchling_cluster_net::MemNet;
    let dir = Dir::new("mirror-wire");
    let transcript = dir.path().join("s1.jsonl");
    std::fs::write(&transcript, b"one\ntwo\n").unwrap();
    let rows = vec![json!({"id": "s1", "transcriptPath": transcript.to_string_lossy()})];
    let net = MemNet::new();
    let mut home = Opts::named("home");
    home.client = true;
    let mut worker = Opts::named("worker");
    let room = Arc::new(crate::acting_ops_tests::FakeRoom::default());
    worker.local = Some(Arc::new(SessionCalls::new(
        Arc::new(Nothing),
        Arc::new(OwnerRows(rows)),
        room.clone(),
    )));
    let (a, b) = pair(&net, home, worker);
    let source = Arc::new(NodeSource::new(a.node.clone()));
    let mirror_dir = dir.path().join("mirror");
    let remote = Arc::new(RemoteSessions::new(mirror_dir.clone()));
    remote.sync(&peers_of(&a.node.status()), source.as_ref(), 0);
    let clock: Clock = Arc::new(|| 1);
    let mirror = Mirror::new(mirror_dir.clone(), remote.clone(), source.clone(), clock);
    mirror.follow(&format!("{}/s1", b.id())).unwrap();
    mirror.pull();
    let copy = mirror_dir.join(b.id().as_str()).join("s1.jsonl");
    assert_eq!(std::fs::read(&copy).unwrap(), b"one\ntwo\n");
    assert_eq!(
        remote.rows()[0]["transcriptPath"],
        json!(copy.to_string_lossy())
    );
    use crate::acting_ops::Room;
    assert!(!room.can_answer_prompts());
    source
        .ask(
            &b.id(),
            crate::cluster_forward::NODE_FOLLOW,
            json!({"topic": "prompts"}),
        )
        .unwrap();
    assert!(
        room.can_answer_prompts(),
        "a client's follow reaches the node's room"
    );
}
