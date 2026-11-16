use crate::acting_ops_tests::FakeRoom;
use crate::cluster_fence::{fence_path, read_fence};
use crate::cluster_fence_keeper::{Boot, CodexTurns, Feed, Keeper, KeeperParts};
use crate::cluster_move::Acts;
use crate::cluster_move_tests::member;
use crate::inbox_ops::InboxOps;
use notchling_engine::cluster::claims::Claim;
use notchling_engine::cluster::failover::{ArmingId, Failover};
use notchling_engine::cluster::fence::{FenceFile, Gate};
use notchling_engine::cluster::fence_gates::SetView;
use notchling_engine::cluster::id::fingerprint;
use notchling_engine::cluster::main_sessions::MainSession;
use notchling_engine::cluster::members::Term;
use notchling_engine::cluster::persist;
use notchling_engine::cluster::state::{start, Change, Command};
use notchling_engine::cluster::{NodeId, Tool};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub(crate) const S1: &str = "aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa";
pub(crate) const S2: &str = "bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb";
pub(crate) const BOOT: &str = "boot-kit";

pub(crate) struct Dir(pub(crate) PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn node(name: &str) -> NodeId {
    NodeId::from_fingerprint(&fingerprint(name.as_bytes())).expect("a node id")
}

pub(crate) fn conversation(session: &str) -> String {
    format!("conv-{session}")
}

pub(crate) fn claim(session: &str, to: &str, n: u64) -> Claim {
    Claim {
        session: session.to_string(),
        node: node(to),
        n,
        term: Term {
            n: 2,
            node: node(to),
        },
        base_set: "set".to_string(),
        base_len: 10,
        touched: false,
    }
}

pub(crate) fn mains(list: &[(&str, Tool)]) -> Vec<(String, Tool)> {
    list.iter()
        .map(|(id, tool)| (id.to_string(), *tool))
        .collect()
}

pub(crate) struct Seed {
    pub failover: Failover,
    pub holder: bool,
    pub main: Vec<(String, Tool)>,
    pub claims: Vec<Claim>,
    pub home: &'static str,
    pub transcripts: BTreeMap<String, String>,
}

impl Default for Seed {
    fn default() -> Seed {
        Seed {
            failover: Failover::Off,
            holder: false,
            main: mains(&[(S1, Tool::Claude)]),
            claims: Vec::new(),
            home: "m5",
            transcripts: BTreeMap::new(),
        }
    }
}

pub(crate) struct FakeFeed {
    pub me: NodeId,
    pub ready: Mutex<BTreeSet<NodeId>>,
    pub latest: Mutex<Option<SetView>>,
    pub acked: Mutex<BTreeMap<NodeId, String>>,
    pub waiting: Mutex<BTreeMap<String, String>>,
    pub live: Mutex<BTreeSet<String>>,
}

impl Feed for FakeFeed {
    fn me(&self) -> Option<NodeId> {
        Some(self.me.clone())
    }

    fn ready(&self) -> BTreeSet<NodeId> {
        self.ready.lock().unwrap().clone()
    }

    fn latest_set(&self) -> Option<SetView> {
        self.latest.lock().unwrap().clone()
    }

    fn acked(&self, node: &NodeId) -> Option<String> {
        self.acked.lock().unwrap().get(node).cloned()
    }

    fn waiting(&self) -> BTreeMap<String, String> {
        self.waiting.lock().unwrap().clone()
    }

    fn live(&self, id: &str) -> bool {
        self.live.lock().unwrap().contains(id)
    }
}

pub(crate) type StopHook = Box<dyn Fn(&str) + Send + Sync>;

#[derive(Default)]
pub(crate) struct FakeActs {
    pub stops: Mutex<Vec<String>>,
    pub on_stop: Mutex<Option<StopHook>>,
    pub fail: Mutex<bool>,
}

impl Acts for FakeActs {
    fn stop_claude(&self, id: &str) -> Result<(), String> {
        if let Some(hook) = self.on_stop.lock().unwrap().as_ref() {
            hook(id);
        }
        self.stops.lock().unwrap().push(format!("claude {id}"));
        if *self.fail.lock().unwrap() {
            return Err("claude stop failed".to_string());
        }
        Ok(())
    }

    fn stop_codex(&self, id: &str) {
        self.stops.lock().unwrap().push(format!("codex {id}"));
    }

    fn start_claude(&self, _session: &MainSession) -> Result<(), String> {
        Err("never started here".to_string())
    }
}

pub(crate) struct FakeCodex {
    pub running: Mutex<Vec<String>>,
    pub stopped: Mutex<Vec<String>>,
}

impl FakeCodex {
    pub(crate) fn running(ids: &[&str]) -> FakeCodex {
        FakeCodex {
            running: Mutex::new(ids.iter().map(|id| id.to_string()).collect()),
            stopped: Mutex::new(Vec::new()),
        }
    }
}

impl CodexTurns for FakeCodex {
    fn running(&self) -> Vec<String> {
        self.running.lock().unwrap().clone()
    }

    fn stop(&self, session: &str) {
        self.stopped.lock().unwrap().push(session.to_string());
    }
}

pub(crate) struct World {
    pub _dir: Dir,
    pub cluster: PathBuf,
    pub me: NodeId,
    pub feed: Arc<FakeFeed>,
    pub acts: Arc<FakeActs>,
    pub inbox: Arc<InboxOps>,
    pub keeper: Arc<Keeper>,
}

pub(crate) fn main_session(id: &str, tool: Tool) -> MainSession {
    MainSession {
        id: id.to_string(),
        tool,
        cwd: "/work".to_string(),
        config_dir: None,
        transcript: (tool == Tool::Claude)
            .then(|| format!("/home/.claude/projects/p/{}.jsonl", conversation(id))),
        respawn: Vec::new(),
        since_ms: 1,
    }
}

pub(crate) fn persist_state(cluster: &std::path::Path, seed: &Seed) {
    let (me, other) = (node("m5"), node("m1"));
    let fp = fingerprint(b"m5");
    let mut home = member(&me, "m5");
    home.fingerprint = fp;
    let (_, entry, mut changes) = start(
        Command::Create {
            id: "c1".to_string(),
            name: "home".to_string(),
            home,
        },
        1,
    )
    .expect("a cluster");
    changes.push(Change::Member(member(&other, "m1")));
    changes.push(Change::Failover(seed.failover));
    if seed.home == "m1" {
        changes.push(Change::Cluster {
            id: "c1".to_string(),
            name: "home".to_string(),
            term: Term { n: 2, node: other },
            home: node("m1"),
            vault: me.clone(),
        });
    }
    if seed.holder {
        changes.push(Change::Holders(BTreeMap::from([(
            node("m1"),
            ArmingId {
                term: Term { n: 1, node: me },
                seq: 1,
            },
        )])));
    }
    changes.push(Change::Claims(
        seed.claims
            .iter()
            .map(|c| (c.session.clone(), c.clone()))
            .collect(),
    ));
    changes.push(Change::Main {
        main: seed
            .main
            .iter()
            .map(|(id, tool)| {
                let mut session = main_session(id, *tool);
                if let Some(path) = seed.transcripts.get(id) {
                    session.transcript = Some(path.clone());
                }
                (id.to_string(), session)
            })
            .collect(),
        kept: BTreeSet::new(),
        ended: BTreeMap::new(),
    });
    std::fs::create_dir_all(cluster).expect("cluster dir");
    persist::save(cluster, &entry, &changes).expect("saved");
}

pub(crate) fn world(tag: &str, seed: Seed) -> World {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("nl-keeper-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cluster = dir.join("cluster");
    std::fs::create_dir_all(&cluster).expect("dirs");
    persist_state(&cluster, &seed);
    let (me, other) = (node("m5"), node("m1"));
    let feed = Arc::new(FakeFeed {
        me: me.clone(),
        ready: Mutex::new(BTreeSet::from([other.clone()])),
        latest: Mutex::new(None),
        acked: Mutex::new(BTreeMap::new()),
        waiting: Mutex::new(BTreeMap::new()),
        live: Mutex::new(BTreeSet::new()),
    });
    let acts = Arc::new(FakeActs::default());
    let inbox = InboxOps::new(dir.join("data"), Arc::new(FakeRoom::default()));
    let clock = Arc::new(AtomicU64::new(1_000));
    let read = clock.clone();
    let keeper = Keeper::new(
        cluster.clone(),
        KeeperParts {
            feed: feed.clone(),
            acts: acts.clone(),
            inbox: inbox.clone(),
            boot: Boot {
                id: BOOT.to_string(),
                now: Arc::new(move || read.load(Ordering::SeqCst)),
            },
        },
    );
    World {
        _dir: Dir(dir),
        cluster,
        me,
        feed,
        acts,
        inbox,
        keeper,
    }
}

impl World {
    pub(crate) fn fence(&self) -> FenceFile {
        read_fence(&self.cluster).expect("a fence file")
    }

    pub(crate) fn gate(&self, key: &str) -> Option<Gate> {
        read_fence(&self.cluster).and_then(|f| f.sessions.get(key).cloned())
    }

    pub(crate) fn path(&self) -> PathBuf {
        fence_path(&self.cluster)
    }

    pub(crate) fn notice(&self, id: &str) -> Option<String> {
        self.inbox
            .items()
            .into_iter()
            .find(|item| item.id == id)
            .map(|item| format!("{} {}", item.title, item.detail))
    }

    pub(crate) fn reseed(&self, seed: &Seed) {
        for name in ["cluster.db", "cluster.db-wal", "cluster.db-shm"] {
            let _ = std::fs::remove_file(self.cluster.join(name));
        }
        persist_state(&self.cluster, seed);
    }
}
