use crate::acting_ops_tests::FakeRoom;
use crate::cluster_move::{Acts, Link, MoveGate, MoveParts, Mover, Restorer, Snaps};
use crate::cluster_move_run::MoveRunner;
use crate::cluster_sessions::LocalRows;
use crate::inbox_ops::InboxOps;
use crate::inbox_ops_tests::item;
use crate::snap_files_tests::node;
use crate::snap_restore::DbHooks;
use crate::snap_restore_testkit::{World, S1};
use crate::snap_testkit::id_of;
use notchling_engine::cluster::core::Takeover;
use notchling_engine::cluster::grants::SecretAllow;
use notchling_engine::cluster::id::fingerprint;
use notchling_engine::cluster::main_sessions::MainSession;
use notchling_engine::cluster::members::Term;
use notchling_engine::cluster::persist;
use notchling_engine::cluster::snapshot::{Manifest, Movable};
use notchling_engine::cluster::standby_store::StandbyStore;
use notchling_engine::cluster::state::{start, Change, Command};
use notchling_engine::cluster::wire::{Origin, OriginTier, WireError};
use notchling_engine::cluster::{Desired, Member, MemberState, NodeId, Tool};
use notchling_local_api::ApiError;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const S2: &str = "bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb";
const WAIT: Duration = Duration::from_secs(30);

type Log = Arc<Mutex<Vec<String>>>;

struct FakeActs {
    who: &'static str,
    log: Log,
}

impl Acts for FakeActs {
    fn stop_claude(&self, id: &str) -> Result<(), String> {
        self.log
            .lock()
            .unwrap()
            .push(format!("{} stop {id}", self.who));
        Ok(())
    }

    fn stop_codex(&self, id: &str) {
        self.log
            .lock()
            .unwrap()
            .push(format!("{} stop-codex {id}", self.who));
    }

    fn start_claude(&self, session: &MainSession) -> Result<(), String> {
        self.log.lock().unwrap().push(format!(
            "{} start {} {}",
            self.who,
            session.id,
            session.respawn.join(" ")
        ));
        Ok(())
    }
}

struct Quiet;

impl LocalRows for Quiet {
    fn rows(&self) -> Result<Vec<Value>, ApiError> {
        Ok(Vec::new())
    }
}

struct StoreSnaps {
    store: StandbyStore,
}

impl Snaps for StoreSnaps {
    fn latest(&self) -> Option<Manifest> {
        self.store.current()
    }

    fn catch_up(&self, _to: &NodeId, _name: &str) -> Result<(), String> {
        Ok(())
    }

    fn final_set(&self, _to: &NodeId, _name: &str) -> Result<Manifest, String> {
        self.store
            .current()
            .ok_or_else(|| "no set was staged".to_string())
    }
}

struct WorldRestorer(Arc<World>);

impl Restorer for WorldRestorer {
    fn restore(
        &self,
        _primary: &NodeId,
        _node: &str,
        sessions: &BTreeSet<String>,
        hooks: &DbHooks,
    ) -> Result<(), String> {
        let ids: Vec<&str> = sessions.iter().map(String::as_str).collect();
        self.0.run_with(&ids, false, true, hooks).map(|_| ())
    }
}

struct FakeLink {
    me: NodeId,
    status: Mutex<Value>,
    peer: OnceLock<Arc<Mover>>,
    took: Mutex<Vec<Takeover>>,
}

fn status_of(me: &NodeId, home: &NodeId, term: u64, other: &NodeId) -> Value {
    json!({
        "me": me,
        "cluster": {"id": "c1", "name": "home", "home": home, "term": {"n": term, "node": home}, "vault": home},
        "members": [
            {"id": me, "name": "mine", "canBeMaster": true, "online": true, "state": "active"},
            {"id": other, "name": "m1", "canBeMaster": true, "online": true, "state": "active"},
        ],
        "main": [S1, S2],
    })
}

impl FakeLink {
    fn new(me: &NodeId, home: &NodeId, term: u64, other: &NodeId) -> Arc<FakeLink> {
        Arc::new(FakeLink {
            me: me.clone(),
            status: Mutex::new(status_of(me, home, term, other)),
            peer: OnceLock::new(),
            took: Mutex::new(Vec::new()),
        })
    }

    fn set_home(&self, home: &NodeId, term: u64) {
        let mut status = self.status.lock().unwrap();
        status["cluster"]["home"] = json!(home);
        status["cluster"]["term"] = json!({"n": term, "node": home});
    }
}

fn wire(e: ApiError) -> WireError {
    WireError::new("refused", e.message)
}

impl Link for FakeLink {
    fn status(&self) -> Value {
        self.status.lock().unwrap().clone()
    }

    fn switch(&self, to: &NodeId, sessions: &[String]) -> Result<Value, WireError> {
        let term = self.status()["cluster"]["term"]["n"].as_u64().unwrap_or(0);
        let ask = Takeover {
            id: "c1".to_string(),
            name: "home".to_string(),
            term: Term {
                n: term + 1,
                node: to.clone(),
            },
            vault: self.me.clone(),
            members: Vec::new(),
            sessions: sessions.to_vec(),
            restored: None,
        };
        let origin = Origin {
            node: self.me.clone(),
            tier: OriginTier::Node,
        };
        let peer = self.peer.get().expect("a peer");
        let answer = peer
            .switch_here(&serde_json::to_value(&ask).unwrap(), &origin)
            .map_err(wire)?;
        self.set_home(to, term + 1);
        Ok(answer)
    }

    fn call(
        &self,
        _to: &NodeId,
        _op: &str,
        args: Value,
        _within: Duration,
    ) -> Result<Value, WireError> {
        self.peer
            .get()
            .expect("a peer")
            .resume_op(&args)
            .map_err(wire)
    }

    fn take_primary(&self, takeover: Takeover) -> Result<Value, WireError> {
        self.set_home(&self.me, takeover.term.n);
        self.took.lock().unwrap().push(takeover);
        Ok(json!({}))
    }
}

fn in_cluster(dir: &Path, main: BTreeMap<String, MainSession>) {
    let fp = fingerprint(b"m5");
    let home = Member {
        id: NodeId::from_fingerprint(&fp).unwrap(),
        name: "m5".to_string(),
        fingerprint: fp,
        addrs: vec!["100.64.0.2".to_string()],
        port: 47823,
        client: true,
        can_be_master: true,
        reference: false,
        accepts_delegation: false,
        headless: false,
        state: MemberState::Active,
        versions: Some(crate::cluster_versions::own()),
        desired: Desired::default(),
        joined_ms: 0,
        secrets: SecretAllow::default(),
    };
    let (_, entry, mut changes) = start(
        Command::Create {
            id: "c1".to_string(),
            name: "home".to_string(),
            home,
        },
        1,
    )
    .unwrap();
    changes.push(Change::Main {
        main,
        kept: BTreeSet::new(),
        ended: BTreeMap::new(),
    });
    persist::save(dir, &entry, &changes).unwrap();
}

fn session(w: &World, id: &str, flags: &[&str]) -> MainSession {
    MainSession {
        id: id.to_string(),
        tool: Tool::Claude,
        cwd: w.repo.display().to_string(),
        config_dir: None,
        transcript: Some(
            w.at(&format!(".claude/projects/p/{id}.jsonl"))
                .display()
                .to_string(),
        ),
        respawn: flags.iter().map(|f| f.to_string()).collect(),
        since_ms: 1,
    }
}

fn mover(
    link: &Arc<FakeLink>,
    (who, log): (&'static str, &Log),
    (w, cluster): (&Arc<World>, &Path),
    inbox: &Arc<InboxOps>,
) -> Arc<Mover> {
    Mover::new(MoveParts {
        link: link.clone(),
        acts: Arc::new(FakeActs {
            who,
            log: log.clone(),
        }),
        rows: Arc::new(Quiet),
        snaps: Arc::new(StoreSnaps {
            store: StandbyStore::new(&w.cluster, &node()),
        }),
        inbox: inbox.clone(),
        gate: Arc::new(MoveGate::default()),
        cluster: cluster.to_path_buf(),
        clock: Arc::new(crate::snap_files_tests::now_ms),
        ready: Arc::new(|_, _| Ok(())),
        restore: Arc::new(WorldRestorer(w.clone())),
    })
}

struct Pair {
    w: Arc<World>,
    from: Arc<Mover>,
    to: Arc<Mover>,
    to_link: Arc<FakeLink>,
    log: Log,
    from_inbox: Arc<InboxOps>,
    to_inbox: Arc<InboxOps>,
    a: NodeId,
    b: NodeId,
}

struct Spec {
    id: &'static str,
    flags: &'static [&'static str],
    movable: bool,
}

fn pair(tag: &str, specs: &[Spec], before_stage: &dyn Fn(&World)) -> Pair {
    let w = Arc::new(World::new());
    let a = node();
    let b = id_of(2);
    let mains: Vec<MainSession> = specs.iter().map(|s| session(&w, s.id, s.flags)).collect();
    let main: BTreeMap<String, MainSession> =
        mains.iter().map(|m| (m.id.clone(), m.clone())).collect();
    in_cluster(&w.cluster, main);
    before_stage(&w);
    let mut entries = Vec::new();
    let mut transcripts = Vec::new();
    for (spec, main) in specs.iter().zip(&mains) {
        let path = std::path::PathBuf::from(main.transcript.clone().unwrap());
        crate::snap_files_tests::put(&path, b"one\ntwo\n");
        let mut entry = w.entry(spec.id, &w.repo, &[&path], &[]);
        if !spec.movable {
            entry.movable = Movable::No("It has commits that aren't pushed".to_string());
        }
        entries.push(entry);
        transcripts.push(path);
    }
    let db = w.at("data/cluster/cluster.db");
    w.stage(entries, &[], &[&db]);
    for path in &transcripts {
        std::fs::remove_file(path).unwrap();
    }
    let log: Log = Arc::default();
    let from_link = FakeLink::new(&a, &a, 1, &b);
    let to_link = FakeLink::new(&b, &a, 1, &a);
    let from_inbox = InboxOps::new(
        w.at(&format!("{tag}-from-data")),
        Arc::new(FakeRoom::default()),
    );
    let to_inbox = InboxOps::new(
        w.at(&format!("{tag}-to-data")),
        Arc::new(FakeRoom::default()),
    );
    let from = mover(
        &from_link,
        ("from", &log),
        (&w, &w.at(&format!("{tag}-from-cluster"))),
        &from_inbox,
    );
    let to = mover(&to_link, ("to", &log), (&w, &w.cluster), &to_inbox);
    let _ = from_link.peer.set(to.clone());
    Pair {
        w,
        from,
        to,
        to_link,
        log,
        from_inbox,
        to_inbox,
        a,
        b,
    }
}

fn run(p: &Pair) -> Arc<MoveRunner> {
    let dir = p.w.at("journal");
    std::fs::create_dir_all(&dir).unwrap();
    let runner = MoveRunner::with(
        p.from.clone(),
        dir,
        (true, Duration::from_millis(5), Duration::from_millis(5)),
    );
    runner.start(&p.b, false).expect("the move starts");
    assert!(runner.wait_done(WAIT));
    runner
}

fn one() -> Vec<Spec> {
    vec![Spec {
        id: S1,
        flags: &["--name", "tester", "--model", "x"],
        movable: true,
    }]
}

#[test]
fn move_stops_then_resumes_with_same_flags() {
    let p = pair("a", &one(), &|_| {});
    let runner = run(&p);

    assert_eq!(
        *p.log.lock().unwrap(),
        vec![
            format!("from stop {S1}"),
            format!("to start {S1} --name tester --model x"),
        ]
    );
    assert_eq!(runner.status()["step"], "done");
    let took = p.to_link.took.lock().unwrap();
    assert_eq!(took.len(), 1);
    assert_eq!(took[0].term.n, 2);
    assert_eq!(took[0].term.node, p.b);
    let restored = took[0].restored.as_ref().expect("the restored state");
    assert_eq!(restored.main[S1].respawn.len(), 4);
    assert_eq!(
        std::fs::read(p.w.at(&format!(".claude/projects/p/{S1}.jsonl"))).unwrap(),
        b"one\ntwo\n"
    );
    p.from_inbox.close();
    p.to_inbox.close();
}

#[test]
fn unpushed_session_stays_as_worker_hosted() {
    let p = pair(
        "b",
        &[
            Spec {
                id: S1,
                flags: &["--name", "one"],
                movable: true,
            },
            Spec {
                id: S2,
                flags: &["--name", "two"],
                movable: false,
            },
        ],
        &|_| {},
    );
    let runner = run(&p);

    let status = runner.status();
    assert_eq!(status["step"], "done");
    assert_eq!(status["moving"], json!([S1]));
    assert_eq!(status["staying"][0][0], S2);
    let log = p.log.lock().unwrap().join("\n");
    assert!(log.contains(&format!("from stop {S1}")));
    assert!(log.contains(&format!("to start {S1}")));
    assert!(!log.contains(S2), "{log}");
    p.from_inbox.close();
    p.to_inbox.close();
}

#[test]
fn inbox_survives_move() {
    let p = pair("c", &one(), &|w| {
        let theirs = InboxOps::new(w.at("c-theirs"), Arc::new(FakeRoom::default()))
            .with_cluster_dir(w.cluster.clone());
        theirs.load();
        theirs.raise(item("from-item"), None);
        theirs.flush();
        theirs.close();
    });
    let cluster = p.w.cluster.clone();
    let db = cluster.join("cluster.db");
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", db.display()));
    }
    let own = p.to_inbox.clone().with_cluster_dir(cluster.clone());
    own.load();
    own.raise(item("to-own"), None);
    own.flush();

    let origin = Origin {
        node: p.a.clone(),
        tier: OriginTier::Node,
    };
    let ask = Takeover {
        id: "c1".to_string(),
        name: "home".to_string(),
        term: Term {
            n: 2,
            node: p.b.clone(),
        },
        vault: p.a.clone(),
        members: Vec::new(),
        sessions: vec![S1.to_string()],
        restored: None,
    };
    p.to.switch_here(&serde_json::to_value(&ask).unwrap(), &origin)
        .expect("the switch");

    let ids: Vec<String> = own.items().into_iter().map(|i| i.id).collect();
    assert!(ids.contains(&"to-own".to_string()), "{ids:?}");
    assert!(ids.contains(&"from-item".to_string()), "{ids:?}");
    assert!(p.to_link.took.lock().unwrap()[0].restored.is_some());
    own.close();
}
