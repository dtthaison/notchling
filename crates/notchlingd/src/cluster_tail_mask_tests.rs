use super::*;
use crate::cluster_mirror::{Mirror, TailSource};
use crate::cluster_sessions::{agent_row, Peer, RemoteSessions, RowSource};
use crate::cluster_testkit::{Dir, FakeCalls};
use crate::grant_use::tests::{cluster, id, job, Fake};
use notchling_engine::cluster::grant_files::GrantValue;
use notchling_engine::cluster::wire::OriginTier;
use notchling_engine::cluster::{JobState, NodeId};
use serde_json::json;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

const JOB: &str = "j-0123456789ab";

struct Rows {
    path: PathBuf,
    id: String,
}

impl LocalRows for Rows {
    fn rows(&self) -> Result<Vec<Value>, ApiError> {
        let mut row = agent_row(&self.id);
        row["transcriptPath"] = json!(self.path.to_string_lossy());
        Ok(vec![row])
    }
}

struct Rig {
    dir: Dir,
    file: PathBuf,
    fake: Arc<Fake>,
    masks: Arc<TailMasks>,
    closed: Arc<AtomicBool>,
    tails: Arc<MaskedTails>,
    inner: Arc<FakeCalls>,
}

fn rig(id_: &str) -> Rig {
    let dir = Dir::new("tail-mask");
    let file = dir.path().join("job.log");
    std::fs::write(&file, b"").unwrap();
    let fake = Arc::new(Fake::default());
    *fake.me.lock().unwrap() = Some(id("worker"));
    *fake.state.lock().unwrap() = Some(cluster("vault", &[job(JOB, "worker", JobState::Running)]));
    let masks = Arc::new(TailMasks::new());
    let closed = Arc::new(AtomicBool::new(false));
    let seen = closed.clone();
    let inner = Arc::new(FakeCalls::default());
    let tails = Arc::new(MaskedTails::new(
        inner.clone(),
        Arc::new(Rows {
            path: file.clone(),
            id: id_.to_string(),
        }),
        (masks.clone(), fake.clone()),
        Arc::new(move |_: &str| seen.load(Ordering::SeqCst)),
    ));
    Rig {
        dir,
        file,
        fake,
        masks,
        closed,
        tails,
        inner,
    }
}

fn origin() -> Origin {
    Origin {
        node: id("other"),
        tier: OriginTier::App,
    }
}

impl Rig {
    fn append(&self, text: &str) {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&self.file)
            .unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    fn grant(&self, value: &str) {
        let one = vec![("api-key".to_string(), GrantValue::new(value.to_string()))];
        self.masks.add(JOB, one).expect("added");
    }

    fn tail(&self, id_: &str, from: u64) -> Result<Value, ApiError> {
        self.tails
            .call(NODE_TAIL, &json!({"id": id_, "from": from}), &origin())
    }
}

fn served(out: &Value) -> String {
    let data = base64_decode(out["data"].as_str().unwrap()).unwrap();
    String::from_utf8(data).unwrap()
}

fn shell_id() -> String {
    format!("shell:{JOB}")
}

#[test]
fn a_job_without_grants_is_served_as_is() {
    let r = rig(&shell_id());
    r.append("plain NL-MARKER-1 text\n");
    let out = r.tail(&shell_id(), 0).expect("served");
    assert!(
        served(&out) == "plain NL-MARKER-1 text\n",
        "served unchanged"
    );
    assert_eq!(out["next"], 23);
    assert!(r.inner.calls.lock().unwrap().is_empty());
}

#[test]
fn tail_masks_json_escaped_multiline_value() {
    let r = rig(&shell_id());
    r.grant("NL-MARKER-LINE-ONE\nNL-MARKER-LINE-TWO");
    r.append("{\"text\":\"NL-MARKER-LINE-ONE\\nNL-MARKER-LINE-TWO\"}\nNL-MARKER-LINE-ONE\n");
    let out = r.tail(&shell_id(), 0).expect("served");
    let text = served(&out);
    assert!(
        !text.contains("NL-MARKER"),
        "no part of the value is served"
    );
    assert!(text.contains("[secret api-key]"));
}

#[test]
fn a_session_row_is_tied_to_its_job_by_the_session_id() {
    let mut running = job(JOB, "worker", JobState::Running);
    running.session = Some("sess-1".to_string());
    let r = rig("sess-1");
    *r.fake.state.lock().unwrap() = Some(cluster("vault", &[running]));
    r.grant("NL-MARKER-1");
    r.append("said NL-MARKER-1\n");
    assert!(!served(&r.tail("sess-1", 0).unwrap()).contains("NL-MARKER"));
}

#[test]
fn a_job_that_lost_its_masks_is_not_served() {
    let r = rig(&shell_id());
    r.masks.mark_granted(JOB);
    r.append("anything NL-MARKER-1\n");
    let err = r.tail(&shell_id(), 0).expect_err("not served");
    assert!(err.message.contains(JOB));
    assert!(!err.message.contains("NL-MARKER"));
}

#[test]
fn a_prefix_at_the_end_is_held_until_the_job_is_over_and_its_writer_closed() {
    let r = rig(&shell_id());
    r.grant("NL-MARKER-LONG-VALUE");
    r.append("done NL-MARKER-LONG");
    let held = r.tail(&shell_id(), 0).expect("served");
    assert!(!served(&held).contains("NL-MARKER"));
    assert!(held["next"].as_u64().unwrap() < held["size"].as_u64().unwrap());
    *r.fake.state.lock().unwrap() = Some(cluster("vault", &[job(JOB, "worker", JobState::Done)]));
    let still = r.tail(&shell_id(), 0).expect("served");
    assert!(still["next"].as_u64().unwrap() < still["size"].as_u64().unwrap());
    r.closed.store(true, Ordering::SeqCst);
    let end = r.tail(&shell_id(), 0).expect("served");
    assert!(!served(&end).contains("NL-MARKER"), "the prefix is masked");
    assert_eq!(end["next"], end["size"]);
}

#[test]
fn other_ops_pass_to_the_next_handler() {
    let r = rig(&shell_id());
    r.tails
        .call("node.sessions", &json!({}), &origin())
        .expect("passed");
    assert_eq!(r.inner.calls.lock().unwrap().len(), 1);
}

struct Owner {
    tails: Arc<MaskedTails>,
    asked: Mutex<Vec<u64>>,
}

impl RowSource for Owner {
    fn rows(&self, _node: &NodeId) -> Result<Value, ApiError> {
        let mut row = agent_row(&shell_id());
        row["transcriptPath"] = json!("/owner/job.log");
        Ok(json!([row]))
    }
}

impl TailSource for Owner {
    fn tail(
        &self,
        _node: &NodeId,
        id_: &str,
        from: u64,
        _seen: Option<&str>,
    ) -> Result<Value, ApiError> {
        self.asked.lock().unwrap().push(from);
        self.tails
            .call(NODE_TAIL, &json!({"id": id_, "from": from}), &origin())
    }
}

#[test]
fn mask_holds_across_chunk_boundaries_and_the_mirror_follows_by_source_offset() {
    let r = rig(&shell_id());
    r.grant("NL-MARKER-A-SECRET-TOKEN");
    let owner_node = NodeId::parse("00000000000000a1").unwrap();
    let owner = Arc::new(Owner {
        tails: r.tails.clone(),
        asked: Mutex::new(Vec::new()),
    });
    let remote = Arc::new(RemoteSessions::new(r.dir.path().join("mirror")));
    remote.sync(
        &[Peer {
            id: owner_node.clone(),
            name: "w1".into(),
            online: true,
        }],
        owner.as_ref(),
        0,
    );
    let now = Arc::new(AtomicU64::new(1_000));
    let ticking = now.clone();
    let mirror = Mirror::new(
        r.dir.path().join("mirror"),
        remote.clone(),
        owner.clone(),
        Arc::new(move || ticking.load(Ordering::SeqCst)),
    );
    let qualified = format!("{owner_node}/{}", shell_id());
    mirror.follow(&qualified).expect("following");
    let mirrored = || {
        let path = remote.rows()[0]["transcriptPath"]
            .as_str()
            .unwrap()
            .to_string();
        std::fs::read_to_string(path).unwrap()
    };
    r.append("start NL-MARKER-A-SEC");
    mirror.pull();
    assert!(
        !mirrored().contains("NL-MARKER"),
        "the half value isn't mirrored"
    );
    r.append("RET-TOKEN end\nsecond NL-MARKER-A-SECRET-TOKEN\n");
    mirror.pull();
    let text = mirrored();
    assert!(!text.contains("NL-MARKER"), "no value in the mirror");
    assert!(
        text.starts_with("start [secret api-key] end\n"),
        "masked in place"
    );
    assert!(text.ends_with("second [secret api-key]\n"));
    let asked = owner.asked.lock().unwrap().clone();
    assert!(asked.len() >= 2, "pulled more than once");
    let size = std::fs::metadata(&r.file).unwrap().len();
    assert!(
        asked.iter().all(|from| *from <= size),
        "every ask stayed inside the source file"
    );
    let before = mirrored();
    mirror.pull();
    assert_eq!(mirrored(), before, "a quiet pull changes nothing");
}
