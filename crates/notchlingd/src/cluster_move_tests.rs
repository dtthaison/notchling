use crate::acting_ops_tests::FakeRoom;
use crate::cluster_move::{trust_check, Acts, Link, MoveGate, MoveParts, Mover, Restorer, Snaps};
use crate::cluster_move_run::MoveRunner;
use crate::cluster_node::wire_error;
use crate::cluster_sessions::LocalRows;
use crate::inbox_ops::InboxOps;
use crate::inbox_ops_tests::item;
use crate::snap_files_tests::node;
use crate::snap_restore::chosen_set;
use crate::snap_restore_fs::adopt_staged;
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
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

pub(crate) const S2: &str = "bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb";
pub(crate) const WAIT: Duration = Duration::from_secs(30);

pub(crate) type Log = Arc<Mutex<Vec<String>>>;
pub(crate) type Hook = Box<dyn Fn(&str) + Send + Sync>;

pub(crate) struct FakeActs {
    who: &'static str,
    log: Log,
    pub(crate) on_stop: Mutex<Option<Hook>>,
    pub(crate) on_start: Mutex<Option<Hook>>,
    pub(crate) swaps: AtomicUsize,
    pub(crate) held: OnceLock<Arc<crate::cluster_relay::HeldRelay>>,
}

impl Acts for FakeActs {
    fn db_replaced(&self) {
        self.swaps.fetch_add(1, Ordering::SeqCst);
        if let Some(held) = self.held.get() {
            held.carry();
        }
    }

    fn stop_claude(&self, id: &str) -> Result<(), String> {
        if let Some(hook) = self.on_stop.lock().unwrap().as_ref() {
            hook(id);
        }
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
        if let Some(hook) = self.on_start.lock().unwrap().as_ref() {
            hook(&session.id);
        }
        self.log.lock().unwrap().push(format!(
            "{} start {} {}",
            self.who,
            session.id,
            session.respawn.join(" ")
        ));
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct Rows(pub(crate) Mutex<Vec<Value>>);

impl LocalRows for Rows {
    fn rows(&self) -> Result<Vec<Value>, ApiError> {
        Ok(self.0.lock().unwrap().clone())
    }
}

pub(crate) struct StoreSnaps {
    store: StandbyStore,
    pub(crate) holds: Mutex<Vec<(NodeId, bool)>>,
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

    fn hold(&self, to: &NodeId, on: bool) {
        self.holds.lock().unwrap().push((to.clone(), on));
    }
}

pub(crate) struct WorldRestorer {
    w: Arc<World>,
    pub(crate) restores: AtomicUsize,
    pub(crate) fail: Mutex<Option<String>>,
}

impl Restorer for WorldRestorer {
    fn check(
        &self,
        primary: &NodeId,
        node: &str,
        set: Option<&str>,
        sessions: &BTreeSet<String>,
    ) -> Result<(), String> {
        let manifest = chosen_set(&self.w.cluster, primary, set, node)?;
        trust_check(&self.w.home, &manifest, sessions, node)
    }

    fn restore(
        &self,
        _primary: &NodeId,
        _node: &str,
        sessions: &BTreeSet<String>,
        set: Option<&str>,
        stage: &Path,
    ) -> Result<(), String> {
        self.restores.fetch_add(1, Ordering::SeqCst);
        if let Some(why) = self.fail.lock().unwrap().clone() {
            return Err(why);
        }
        let ids: Vec<&str> = sessions.iter().map(String::as_str).collect();
        self.w.run_staged(&ids, set, stage).map(|_| ())
    }
}

pub(crate) enum Fault {
    LoseAnswer,
    DropEarly,
    Fail(&'static str),
}

pub(crate) struct FakeLink {
    me: NodeId,
    cluster: PathBuf,
    status: Mutex<Value>,
    peer: OnceLock<Arc<Mover>>,
    pub(crate) took: Mutex<Vec<Takeover>>,
    pub(crate) switch_faults: Mutex<VecDeque<Fault>>,
    pub(crate) call_faults: Mutex<VecDeque<Fault>>,
    pub(crate) take_fails: Mutex<Option<String>>,
    pub(crate) nulls: Mutex<u32>,
    pub(crate) fences: Mutex<Vec<Option<String>>>,
    pub(crate) switches: AtomicUsize,
    pub(crate) waits: Mutex<Vec<Duration>>,
    pub(crate) on_take: Mutex<Option<Hook>>,
    pub(crate) late: Mutex<Vec<std::thread::JoinHandle<()>>>,
    pub(crate) entered: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

pub(crate) fn member(id: &NodeId, name: &str) -> Member {
    Member {
        id: id.clone(),
        name: name.to_string(),
        fingerprint: fingerprint(name.as_bytes()),
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
    }
}

fn status_of(me: &NodeId, home: &NodeId, term: u64, other: &NodeId) -> Value {
    let versions = serde_json::to_value(crate::cluster_versions::own()).unwrap();
    json!({
        "me": me,
        "cluster": {"id": "c1", "name": "home", "home": home, "term": {"n": term, "node": home}, "vault": home},
        "members": [
            {"id": me, "name": "mine", "canBeMaster": true, "online": true, "state": "active", "versions": versions},
            {"id": other, "name": "m1", "canBeMaster": true, "online": true, "state": "active", "versions": versions},
        ],
        "main": [S1, S2],
    })
}

pub(crate) fn origin(node: &NodeId) -> Origin {
    Origin {
        node: node.clone(),
        tier: OriginTier::Node,
    }
}

pub(crate) fn takeover(from: &NodeId, to: &NodeId, n: u64, sessions: &[&str]) -> Takeover {
    Takeover {
        id: "c1".to_string(),
        name: "home".to_string(),
        term: Term {
            n,
            node: to.clone(),
        },
        vault: from.clone(),
        members: vec![member(from, "mine"), member(to, "m1")],
        sessions: sessions.iter().map(|s| s.to_string()).collect(),
        set: None,
        claims: Vec::new(),
        restored: None,
        staged: false,
    }
}

impl FakeLink {
    fn new(me: &NodeId, home: &NodeId, other: &NodeId, cluster: &Path) -> Arc<FakeLink> {
        Arc::new(FakeLink {
            me: me.clone(),
            cluster: cluster.to_path_buf(),
            status: Mutex::new(status_of(me, home, 1, other)),
            peer: OnceLock::new(),
            took: Mutex::new(Vec::new()),
            switch_faults: Mutex::default(),
            call_faults: Mutex::default(),
            take_fails: Mutex::new(None),
            nulls: Mutex::new(0),
            fences: Mutex::new(Vec::new()),
            switches: AtomicUsize::new(0),
            waits: Mutex::new(Vec::new()),
            on_take: Mutex::new(None),
            late: Mutex::new(Vec::new()),
            entered: Mutex::new(None),
        })
    }

    pub(crate) fn set_home(&self, home: &NodeId, term: u64) {
        let mut status = self.status.lock().unwrap();
        status["cluster"]["home"] = json!(home);
        status["cluster"]["term"] = json!({"n": term, "node": home});
    }

    pub(crate) fn edit(&self, change: impl FnOnce(&mut Value)) {
        change(&mut self.status.lock().unwrap());
    }

    fn peer(&self) -> &Arc<Mover> {
        self.peer.get().expect("a peer")
    }
}

impl Link for FakeLink {
    fn status(&self) -> Value {
        let mut nulls = self.nulls.lock().unwrap();
        if *nulls > 0 {
            *nulls -= 1;
            return json!({"cluster": Value::Null, "members": []});
        }
        drop(nulls);
        self.status.lock().unwrap().clone()
    }

    fn switch(
        &self,
        to: &NodeId,
        sessions: &[String],
        set: Option<&str>,
    ) -> Result<Value, WireError> {
        self.switches.fetch_add(1, Ordering::SeqCst);
        let fault = self.switch_faults.lock().unwrap().pop_front();
        if let Some(Fault::Fail(code)) = fault {
            return Err(WireError::new(code, format!("scripted {code}")));
        }
        let term = self.status.lock().unwrap()["cluster"]["term"]["n"]
            .as_u64()
            .unwrap_or(0);
        let ids: Vec<&str> = sessions.iter().map(String::as_str).collect();
        let mut ask = takeover(&self.me, to, term + 1, &ids);
        ask.set = set.map(str::to_string);
        let answer = self
            .peer()
            .switch_here(&serde_json::to_value(&ask).unwrap(), &origin(&self.me))
            .map_err(wire_error)?;
        if matches!(fault, Some(Fault::LoseAnswer)) {
            return Err(WireError::new("timeout", "the answer was lost"));
        }
        self.set_home(to, term + 1);
        Ok(answer)
    }

    fn call(
        &self,
        _to: &NodeId,
        _op: &str,
        args: Value,
        within: Duration,
    ) -> Result<Value, WireError> {
        self.waits.lock().unwrap().push(within);
        let fault = self.call_faults.lock().unwrap().pop_front();
        if let Some(Fault::Fail(code)) = fault {
            return Err(WireError::new(code, format!("scripted {code}")));
        }
        if matches!(fault, Some(Fault::DropEarly)) {
            let (peer, from) = (self.peer().clone(), origin(&self.me));
            let late = std::thread::spawn(move || {
                let _ = peer.resume_op(&args, &from);
            });
            self.late.lock().unwrap().push(late);
            if let Some(entered) = self.entered.lock().unwrap().take() {
                let _ = entered.recv_timeout(WAIT);
            }
            return Err(WireError::new("offline", "the link dropped"));
        }
        let answer = self
            .peer()
            .resume_op(&args, &origin(&self.me))
            .map_err(wire_error)?;
        if matches!(fault, Some(Fault::LoseAnswer)) {
            return Err(WireError::new("timeout", "the answer was lost"));
        }
        Ok(answer)
    }

    fn take_primary(&self, takeover: Takeover) -> Result<Value, WireError> {
        if let Some(why) = self.take_fails.lock().unwrap().clone() {
            return Err(WireError::new("refused", why));
        }
        let status = self.status.lock().unwrap().clone();
        let held = status["cluster"]["home"].as_str() == Some(self.me.as_str());
        if held && status["cluster"]["term"]["n"].as_u64() == Some(takeover.term.n) {
            return Ok(json!({}));
        }
        if let Some(hook) = self.on_take.lock().unwrap().as_ref() {
            hook("");
        }
        if takeover.staged {
            adopt_staged(&self.cluster).map_err(|why| WireError::new("failed", why))?;
        }
        self.set_home(&self.me, takeover.term.n);
        self.took.lock().unwrap().push(takeover);
        Ok(json!({}))
    }

    fn fence(&self, reason: Option<String>) {
        self.fences.lock().unwrap().push(reason);
    }
}

fn in_cluster(dir: &Path, main: BTreeMap<String, MainSession>) {
    let fp = fingerprint(b"m5");
    let mut home = member(&NodeId::from_fingerprint(&fp).unwrap(), "m5");
    home.fingerprint = fp;
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
    std::fs::create_dir_all(dir).unwrap();
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

pub(crate) fn trusting(folder: &Path) -> String {
    json!({"projects": {folder.display().to_string(): {"hasTrustDialogAccepted": true}}})
        .to_string()
}

pub(crate) struct Side {
    pub(crate) mover: Arc<Mover>,
    pub(crate) link: Arc<FakeLink>,
    pub(crate) acts: Arc<FakeActs>,
    pub(crate) rows: Arc<Rows>,
    pub(crate) snaps: Arc<StoreSnaps>,
    pub(crate) restore: Arc<WorldRestorer>,
    pub(crate) inbox: Arc<InboxOps>,
    pub(crate) cluster: PathBuf,
}

fn side(w: &Arc<World>, who: &'static str, link: Arc<FakeLink>, log: &Log, cluster: &Path) -> Side {
    let acts = Arc::new(FakeActs {
        who,
        log: log.clone(),
        on_stop: Mutex::new(None),
        on_start: Mutex::new(None),
        swaps: AtomicUsize::new(0),
        held: OnceLock::new(),
    });
    let rows = Arc::new(Rows::default());
    let snaps = Arc::new(StoreSnaps {
        store: StandbyStore::new(&w.cluster, &node()),
        holds: Mutex::new(Vec::new()),
    });
    let restore = Arc::new(WorldRestorer {
        w: w.clone(),
        restores: AtomicUsize::new(0),
        fail: Mutex::new(None),
    });
    let inbox = InboxOps::new(w.at(&format!("{who}-data")), Arc::new(FakeRoom::default()));
    let mover = Mover::new(MoveParts {
        link: link.clone(),
        acts: acts.clone(),
        rows: rows.clone(),
        snaps: snaps.clone(),
        inbox: inbox.clone(),
        gate: Arc::new(MoveGate::default()),
        cluster: cluster.to_path_buf(),
        clock: Arc::new(crate::snap_files_tests::now_ms),
        ready: Arc::new(|_, _| Ok(())),
        restore: restore.clone(),
    });
    Side {
        mover,
        link,
        acts,
        rows,
        snaps,
        restore,
        inbox,
        cluster: cluster.to_path_buf(),
    }
}

pub(crate) struct Pair {
    pub(crate) w: Arc<World>,
    pub(crate) from: Side,
    pub(crate) to: Side,
    pub(crate) log: Log,
    pub(crate) manifest: Manifest,
    pub(crate) a: NodeId,
    pub(crate) b: NodeId,
}

impl Drop for Pair {
    fn drop(&mut self) {
        self.from.inbox.close();
        self.to.inbox.close();
    }
}

pub(crate) struct Spec {
    pub(crate) id: &'static str,
    pub(crate) flags: &'static [&'static str],
    pub(crate) movable: bool,
}

pub(crate) fn pair(specs: &[Spec], before_stage: &dyn Fn(&World)) -> Pair {
    let w = Arc::new(World::new());
    let a = node();
    let b = id_of(2);
    let mains: Vec<MainSession> = specs.iter().map(|s| session(&w, s.id, s.flags)).collect();
    let main: BTreeMap<String, MainSession> =
        mains.iter().map(|m| (m.id.clone(), m.clone())).collect();
    in_cluster(&w.cluster, main.clone());
    let from_cluster = w.at("from-cluster");
    in_cluster(&from_cluster, main);
    crate::snap_files_tests::put(&w.at(".claude.json"), trusting(&w.repo).as_bytes());
    before_stage(&w);
    let mut entries = Vec::new();
    let mut transcripts = Vec::new();
    for (spec, main) in specs.iter().zip(&mains) {
        let path = PathBuf::from(main.transcript.clone().unwrap());
        crate::snap_files_tests::put(&path, b"one\ntwo\n");
        let mut entry = w.entry(spec.id, &w.repo, &[&path], &[]);
        if !spec.movable {
            entry.movable = Movable::No("It has commits that aren't pushed".to_string());
        }
        entries.push(entry);
        transcripts.push(path);
    }
    let db = w.at("data/cluster/cluster.db");
    let manifest = w.stage(entries, &[], &[&db]);
    for path in &transcripts {
        std::fs::remove_file(path).unwrap();
    }
    let log: Log = Arc::default();
    let from_link = FakeLink::new(&a, &a, &b, &from_cluster);
    let to_link = FakeLink::new(&b, &a, &a, &w.cluster);
    let from = side(&w, "from", from_link, &log, &from_cluster);
    let to = side(&w, "to", to_link, &log, &w.cluster.clone());
    let _ = from.link.peer.set(to.mover.clone());
    let _ = to.link.peer.set(from.mover.clone());
    Pair {
        w,
        from,
        to,
        log,
        manifest,
        a,
        b,
    }
}

pub(crate) fn runner(p: &Pair) -> Arc<MoveRunner> {
    let dir = p.w.at("journal");
    std::fs::create_dir_all(&dir).unwrap();
    runner_in(p, dir)
}

pub(crate) fn runner_in(p: &Pair, dir: PathBuf) -> Arc<MoveRunner> {
    MoveRunner::with(
        p.from.mover.clone(),
        dir,
        (true, Duration::from_millis(5), Duration::from_millis(5)),
    )
}

pub(crate) fn run(p: &Pair) -> Arc<MoveRunner> {
    let runner = runner(p);
    runner.start(&p.b, false).expect("the move starts");
    assert!(runner.wait_done(WAIT));
    runner
}

pub(crate) fn one() -> Vec<Spec> {
    vec![Spec {
        id: S1,
        flags: &["--name", "tester", "--model", "x"],
        movable: true,
    }]
}

pub(crate) fn logged(p: &Pair) -> Vec<String> {
    p.log.lock().unwrap().clone()
}

#[test]
fn move_stops_then_resumes_with_same_flags() {
    let p = pair(&one(), &|_| {});
    let runner = run(&p);

    assert_eq!(
        logged(&p),
        vec![
            format!("from stop {S1}"),
            format!("to start {S1} --name tester --model x"),
        ]
    );
    assert_eq!(runner.status()["step"], "done");
    let took = p.to.link.took.lock().unwrap();
    assert_eq!(took.len(), 1);
    assert_eq!(took[0].term.n, 2);
    assert_eq!(took[0].term.node, p.b);
    assert_eq!(took[0].set.as_deref(), Some(p.manifest.set.as_str()));
    assert!(took[0].staged);
    let restored = took[0].restored.as_ref().expect("the restored state");
    assert_eq!(restored.main[S1].respawn.len(), 4);
    assert_eq!(
        std::fs::read(p.w.at(&format!(".claude/projects/p/{S1}.jsonl"))).unwrap(),
        b"one\ntwo\n"
    );
}

#[test]
fn unpushed_session_stays_as_worker_hosted() {
    let p = pair(
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
    let log = logged(&p).join("\n");
    assert!(log.contains(&format!("from stop {S1}")));
    assert!(log.contains(&format!("to start {S1}")));
    assert!(!log.contains(S2), "{log}");
}

#[test]
fn inbox_survives_move() {
    let p = pair(&one(), &|w| {
        let theirs = InboxOps::new(w.at("c-theirs"), Arc::new(FakeRoom::default()))
            .with_cluster_dir(w.cluster.clone());
        theirs.load();
        theirs.raise(item("from-item"), None);
        theirs.flush();
        theirs.close();
    });
    let cluster = p.w.cluster.clone();
    let own = p.to.inbox.clone().with_cluster_dir(cluster.clone());
    own.load();
    own.raise(item("to-own"), None);
    own.flush();

    let ask = takeover(&p.a, &p.b, 2, &[S1]);
    p.to.mover
        .switch_here(&serde_json::to_value(&ask).unwrap(), &origin(&p.a))
        .expect("the switch");

    let ids: Vec<String> = own.items().into_iter().map(|i| i.id).collect();
    assert!(ids.contains(&"to-own".to_string()), "{ids:?}");
    assert!(ids.contains(&"from-item".to_string()), "{ids:?}");
    assert!(p.to.link.took.lock().unwrap()[0].restored.is_some());
}
