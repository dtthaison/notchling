use crate::acting::Acting;
use crate::cluster_fence_keeper_kit::{node, world, Seed, World, S1};
use crate::cluster_move::{Link, MoveGate, MoveParts, Mover, Restorer, Snaps};
use crate::cluster_move_tests::{Log, Rows};
use crate::cluster_returns::{Repo, Returns, ReturnsParts};
use crate::cluster_returns_waiting::Waiting;
use notchling_engine::cluster::core::Takeover;
use notchling_engine::cluster::main_sessions::MainSession;
use notchling_engine::cluster::persist;
use notchling_engine::cluster::snapshot::repo_state::RepoFacts;
use notchling_engine::cluster::snapshot::Manifest;
use notchling_engine::cluster::state::{apply, Command};
use notchling_engine::cluster::wire::{OriginTier, WireError};
use notchling_engine::cluster::NodeId;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(crate) struct RecLink {
    pub cluster: PathBuf,
    pub me: NodeId,
    pub other: NodeId,
    pub records: Mutex<Vec<(Command, OriginTier)>>,
    pub app_calls: Mutex<Vec<(NodeId, String, Value)>>,
    pub app_answer: Mutex<Result<Value, WireError>>,
    pub fail: Mutex<Option<String>>,
    pub rival: Mutex<Option<Command>>,
    pub lag: Mutex<Option<Duration>>,
    pub log: Log,
}

impl Link for RecLink {
    fn status(&self) -> Value {
        let versions = serde_json::to_value(crate::cluster_versions::own()).unwrap();
        let state = persist::load(&self.cluster).unwrap().unwrap();
        json!({
            "me": self.me,
            "cluster": {"id": "c1", "name": "home", "home": state.home, "term": state.term, "vault": state.home},
            "members": [
                {"id": self.me, "name": "m5", "canBeMaster": true, "online": true, "state": "active", "versions": versions},
                {"id": self.other, "name": "m1", "canBeMaster": true, "online": true, "state": "active", "versions": versions},
            ],
        })
    }

    fn switch(&self, _: &NodeId, _: &[String], _: Option<&str>) -> Result<Value, WireError> {
        Err(WireError::new("not_sent", "no switch here"))
    }

    fn call(&self, _: &NodeId, _: &str, _: Value, _: Duration) -> Result<Value, WireError> {
        Err(WireError::new("not_sent", "no call here"))
    }

    fn take_primary(&self, _: Takeover) -> Result<Value, WireError> {
        Err(WireError::new("not_sent", "no takeover here"))
    }

    fn fence(&self, _: Option<String>) {}

    fn record(
        &self,
        command: Command,
        tier: OriginTier,
        _within: Duration,
    ) -> Result<(), WireError> {
        if let Some(why) = self.fail.lock().unwrap().clone() {
            return Err(WireError::new("refused", why));
        }
        let note = match &command {
            Command::Claim { claim } => format!("record claim {} {}", claim.session, claim.n),
            Command::Touched { session } => format!("record touched {session}"),
            Command::SideCopy { copy } => format!("record side-copy {}", copy.session),
            Command::HideSideCopy { session, .. } => format!("record hide {session}"),
            _ => "record other".to_string(),
        };
        self.log.lock().unwrap().push(note);
        self.records.lock().unwrap().push((command.clone(), tier));
        let mut state = persist::load(&self.cluster).unwrap().unwrap();
        if let Some(rival) = self.rival.lock().unwrap().take() {
            let applied = apply(&mut state, rival, 1_000).unwrap();
            if let Some(entry) = applied.entry {
                persist::save(&self.cluster, &entry, &applied.changes).unwrap();
            }
        }
        let session = match &command {
            Command::Claim { claim } => Some(claim.session.clone()),
            _ => None,
        };
        let applied = apply(&mut state, command, 1_000)
            .map_err(|e| WireError::new("refused", e.to_string()))?;
        let Some(entry) = applied.entry else {
            let held = session.and_then(|s| state.claims.get(&s).map(|c| c.node.clone()));
            return match held {
                Some(node) if node != self.me => Err(WireError::new(
                    "refused",
                    "another machine holds a newer claim on that session",
                )),
                _ => Ok(()),
            };
        };
        let lag = *self.lag.lock().unwrap();
        let Some(lag) = lag else {
            persist::save(&self.cluster, &entry, &applied.changes).unwrap();
            return Ok(());
        };
        let cluster = self.cluster.clone();
        std::thread::spawn(move || {
            std::thread::sleep(lag);
            persist::save(&cluster, &entry, &applied.changes).unwrap();
        });
        Ok(())
    }

    fn call_app(
        &self,
        to: &NodeId,
        op: &str,
        args: Value,
        _within: Duration,
    ) -> Result<Value, WireError> {
        self.app_calls
            .lock()
            .unwrap()
            .push((to.clone(), op.to_string(), args));
        self.log.lock().unwrap().push(format!("call {op}"));
        self.app_answer.lock().unwrap().clone()
    }
}

pub(crate) struct KActs {
    pub log: Log,
}

impl crate::cluster_move::Acts for KActs {
    fn stop_claude(&self, id: &str) -> Result<(), String> {
        self.log.lock().unwrap().push(format!("stop {id}"));
        Ok(())
    }

    fn stop_codex(&self, _id: &str) {}

    fn start_claude(&self, session: &MainSession) -> Result<(), String> {
        self.log
            .lock()
            .unwrap()
            .push(format!("start {}", session.id));
        Ok(())
    }
}

pub(crate) struct KSnaps(pub Mutex<Option<Manifest>>);

impl Snaps for KSnaps {
    fn latest(&self) -> Option<Manifest> {
        self.0.lock().unwrap().clone()
    }

    fn catch_up(&self, _: &NodeId, _: &str) -> Result<(), String> {
        Ok(())
    }

    fn final_set(&self, _: &NodeId, _: &str) -> Result<Manifest, String> {
        Err("no set".to_string())
    }

    fn hold(&self, _: &NodeId, _: bool) {}
}

struct KRestore;

impl Restorer for KRestore {
    fn check(
        &self,
        _: &NodeId,
        _: &str,
        _: Option<&str>,
        _: &BTreeSet<String>,
    ) -> Result<(), String> {
        Ok(())
    }

    fn restore(
        &self,
        _: &NodeId,
        _: &str,
        _: &BTreeSet<String>,
        _: Option<&str>,
        _: &Path,
    ) -> Result<(), String> {
        Ok(())
    }
}

pub(crate) struct KRepo(pub Mutex<bool>);

impl Repo for KRepo {
    fn changed(&self, _cwd: &str, _was: Option<&RepoFacts>) -> bool {
        *self.0.lock().unwrap()
    }
}

pub(crate) struct Kit {
    pub w: World,
    pub link: Arc<RecLink>,
    pub rows: Arc<Rows>,
    pub repo: Arc<KRepo>,
    pub acting: Arc<Acting>,
    pub mover: Arc<Mover>,
    pub waiting: Arc<Waiting>,
    pub returns: Arc<Returns>,
    pub log: Log,
    pub dir: PathBuf,
}

pub(crate) fn kit(tag: &str, mut seed: Seed) -> Kit {
    let w = world(tag, Seed::default());
    let dir = w._dir.0.clone();
    let proj = dir.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    for (id, _) in &seed.main {
        let path = proj.join(format!("{id}.jsonl"));
        seed.transcripts
            .entry(id.clone())
            .or_insert_with(|| path.display().to_string());
    }
    w.reseed(&seed);
    let log: Log = Arc::default();
    let link = Arc::new(RecLink {
        cluster: w.cluster.clone(),
        me: node("m5"),
        other: node("m1"),
        records: Mutex::default(),
        app_calls: Mutex::default(),
        app_answer: Mutex::new(Ok(json!({"started": true}))),
        fail: Mutex::default(),
        rival: Mutex::default(),
        lag: Mutex::default(),
        log: log.clone(),
    });
    let rows = Arc::new(Rows::default());
    let snaps = Arc::new(KSnaps(Mutex::new(None)));
    let mover = Mover::new(MoveParts {
        link: link.clone(),
        acts: Arc::new(KActs { log: log.clone() }),
        rows: rows.clone(),
        snaps,
        inbox: w.inbox.clone(),
        gate: Arc::new(MoveGate::default()),
        cluster: w.cluster.clone(),
        clock: Arc::new(|| 5_000),
        ready: Arc::new(|_, _| Ok(())),
        restore: Arc::new(KRestore),
    });
    let waiting = Waiting::new(&w.cluster);
    let repo = Arc::new(KRepo(Mutex::new(false)));
    let acting = Arc::new(Acting::in_memory(true));
    let returns = Returns::new(ReturnsParts {
        mover: mover.clone(),
        repo: repo.clone(),
        acting: acting.clone(),
        data: dir.clone(),
        waiting: waiting.clone(),
    });
    Kit {
        w,
        link,
        rows,
        repo,
        acting,
        mover,
        waiting,
        returns,
        log,
        dir,
    }
}

impl Kit {
    pub(crate) fn transcript(&self, session: &str) -> PathBuf {
        self.dir.join("proj").join(format!("{session}.jsonl"))
    }

    pub(crate) fn write_transcript(&self, session: &str, bytes: usize) -> PathBuf {
        let path = self.transcript(session);
        std::fs::write(&path, vec![b'x'; bytes]).unwrap();
        path
    }

    pub(crate) fn state(&self) -> notchling_engine::cluster::state::ClusterState {
        persist::load(&self.w.cluster).unwrap().unwrap()
    }

    pub(crate) fn logged(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    pub(crate) fn recorded(&self) -> Vec<Command> {
        self.link
            .records
            .lock()
            .unwrap()
            .iter()
            .map(|(c, _)| c.clone())
            .collect()
    }
}

pub(crate) fn seed_after_promote(claims: &[(&str, u64)]) -> Seed {
    Seed {
        home: "m1",
        main: vec![(S1.to_string(), notchling_engine::cluster::Tool::Claude)],
        claims: claims
            .iter()
            .map(|(s, n)| crate::cluster_fence_keeper_kit::claim(s, "m1", *n))
            .collect(),
        ..Seed::default()
    }
}

pub(crate) fn agent_row(
    id: &str,
    name: &str,
    group: notchling_engine::sessions::AgentGroup,
) -> Value {
    use notchling_engine::sessions::agent::{Agent, AgentTarget};
    serde_json::to_value(Agent {
        id: id.to_string(),
        name: name.to_string(),
        project: "repo".to_string(),
        cwd: "/work".to_string(),
        group,
        label: "x".to_string(),
        detail: None,
        since_ms: 1,
        target: AgentTarget::Job("job-1".to_string()),
        suggested_reply: None,
        transcript_path: None,
        host: "Background".to_string(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
        worktree_path: None,
    })
    .unwrap()
}

impl Kit {
    pub(crate) fn was_primary(&self, term: u64) {
        let bytes = serde_json::to_vec(&json!({"term": term})).unwrap();
        std::fs::write(self.w.cluster.join("was-primary.json"), bytes).unwrap();
    }

    pub(crate) fn item(&self, id: &str) -> Option<notchling_engine::inbox::Item> {
        self.w.inbox.items().into_iter().find(|i| i.id == id)
    }

    pub(crate) fn side_copy(&self, session: &str, path: &Path) {
        use notchling_engine::cluster::claims::SideCopy;
        let copy = SideCopy {
            session: session.to_string(),
            node: node("m5"),
            base_set: "set".to_string(),
            extra_bytes: 5,
            repo_changed: false,
            path: Some(path.display().to_string()),
            at_ms: 7,
            hidden: false,
        };
        self.link
            .record(Command::SideCopy { copy }, OriginTier::Node, Duration::ZERO)
            .unwrap();
        self.link.log.lock().unwrap().clear();
        self.link.records.lock().unwrap().clear();
    }
}
