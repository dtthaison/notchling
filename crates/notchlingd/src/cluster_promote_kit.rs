use crate::acting::Acting;
use crate::cluster_fence_keeper::Boot;
use crate::cluster_move::{Link, MoveGate, MoveParts, Mover, Restorer};
use crate::cluster_move_tests::{member, one, pair, Log, Pair, Spec, WorldRestorer};
use crate::cluster_promote::{Cluster, PromoteParts, Promoter, Replier};
use notchling_engine::cluster::failover::Failover;
use notchling_engine::cluster::members::Term;
use notchling_engine::cluster::persist;
use notchling_engine::cluster::state::{ClusterState, Command};
use notchling_engine::cluster::wire::WireError;
use notchling_engine::cluster::NodeId;
use notchling_engine::inbox::{Item, Kind, State};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(crate) const CUT_PUSH: &str =
    include_str!("../../engine/tests/fixtures/cluster/cut-push.jsonl");
pub(crate) const BOOT_MS: u64 = 5_000_000;

pub(crate) type Answers = BTreeMap<(NodeId, String), Result<Value, WireError>>;

pub(crate) struct FakeCluster {
    pub(crate) state: Mutex<ClusterState>,
    pub(crate) link: Arc<crate::cluster_move_tests::FakeLink>,
    pub(crate) answers: Mutex<Answers>,
    pub(crate) calls: Mutex<Vec<(NodeId, String, Value)>>,
    pub(crate) records: Mutex<Vec<Command>>,
    pub(crate) record_fail: Mutex<Option<String>>,
    pub(crate) log: Log,
}

impl Cluster for FakeCluster {
    fn state(&self) -> Option<ClusterState> {
        Some(self.state.lock().unwrap().clone())
    }

    fn status(&self) -> Value {
        Link::status(&*self.link)
    }

    fn call(
        &self,
        to: &NodeId,
        op: &str,
        args: Value,
        _within: Duration,
    ) -> Result<Value, WireError> {
        self.calls
            .lock()
            .unwrap()
            .push((to.clone(), op.to_string(), args));
        self.answers
            .lock()
            .unwrap()
            .get(&(to.clone(), op.to_string()))
            .cloned()
            .unwrap_or_else(|| Err(WireError::new("not_sent", "offline")))
    }

    fn record(&self, command: Command) -> Result<(), WireError> {
        if let Some(why) = self.record_fail.lock().unwrap().clone() {
            return Err(WireError::new("refused", why));
        }
        if let Command::Claim { claim } = &command {
            self.log
                .lock()
                .unwrap()
                .push(format!("claim {} {}", claim.session, claim.n));
        }
        self.records.lock().unwrap().push(command);
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct FakeReplier {
    pub(crate) sent: Mutex<Vec<(String, String)>>,
    pub(crate) fail: Mutex<Option<String>>,
}

impl Replier for FakeReplier {
    fn reply(&self, session: &str, text: &str) -> Result<(), String> {
        if let Some(why) = self.fail.lock().unwrap().clone() {
            return Err(why);
        }
        self.sent
            .lock()
            .unwrap()
            .push((session.to_string(), text.to_string()));
        Ok(())
    }
}

type After = Box<dyn Fn() + Send + Sync>;

pub(crate) struct Editing {
    pub(crate) inner: Arc<WorldRestorer>,
    pub(crate) after: Mutex<Option<After>>,
}

impl Restorer for Editing {
    fn check(
        &self,
        primary: &NodeId,
        node: &str,
        set: Option<&str>,
        sessions: &BTreeSet<String>,
    ) -> Result<(), String> {
        self.inner.check(primary, node, set, sessions)
    }

    fn restore(
        &self,
        primary: &NodeId,
        node: &str,
        sessions: &BTreeSet<String>,
        set: Option<&str>,
        stage: &Path,
    ) -> Result<(), String> {
        self.inner.restore(primary, node, sessions, set, stage)?;
        if let Some(after) = self.after.lock().unwrap().as_ref() {
            after();
        }
        Ok(())
    }
}

pub(crate) struct Rig {
    pub(crate) p: Pair,
    pub(crate) cluster: Arc<FakeCluster>,
    pub(crate) promoter: Arc<Promoter>,
    pub(crate) replier: Arc<FakeReplier>,
    pub(crate) acting: Arc<Acting>,
    pub(crate) restore: Arc<Editing>,
    pub(crate) clock: Arc<AtomicU64>,
}

impl Rig {
    pub(crate) fn a(&self) -> NodeId {
        self.p.a.clone()
    }

    pub(crate) fn b(&self) -> NodeId {
        self.p.b.clone()
    }

    pub(crate) fn answer(&self, node: &NodeId, op: &str, result: Result<Value, WireError>) {
        self.cluster
            .answers
            .lock()
            .unwrap()
            .insert((node.clone(), op.to_string()), result);
    }

    pub(crate) fn add_peer(&self, peer: &NodeId, name: &str) {
        let mut state = self.cluster.state.lock().unwrap();
        state.members.insert(peer.clone(), member(peer, name));
        drop(state);
        let versions = serde_json::to_value(crate::cluster_versions::own()).unwrap();
        let entry = json!({
            "id": peer, "name": name, "canBeMaster": true, "online": true,
            "state": "active", "versions": versions,
        });
        self.p
            .to
            .link
            .edit(|v| v["members"].as_array_mut().unwrap().push(entry));
    }

    pub(crate) fn item(&self, id: &str) -> Option<Item> {
        self.p.to.inbox.items().into_iter().find(|i| i.id == id)
    }

    pub(crate) fn open_ids(&self) -> Vec<String> {
        self.p
            .to
            .inbox
            .items()
            .into_iter()
            .filter(|i| i.state == State::Open)
            .map(|i| i.id)
            .collect()
    }

    pub(crate) fn kinds(&self, kind: Kind) -> Vec<Item> {
        self.p
            .to
            .inbox
            .items()
            .into_iter()
            .filter(|i| i.kind == kind)
            .collect()
    }

    pub(crate) fn finish(&self) {
        assert!(self.promoter.wait_done(crate::cluster_move_tests::WAIT));
    }
}

pub(crate) fn state_of(p: &Pair) -> ClusterState {
    let mut state = persist::load(&p.w.cluster).unwrap().unwrap();
    state.id = "c1".to_string();
    state.name = "home".to_string();
    state.term = Term {
        n: 1,
        node: p.a.clone(),
    };
    state.home = p.a.clone();
    state.vault = p.a.clone();
    state.members.clear();
    state.members.insert(p.a.clone(), member(&p.a, "m5"));
    state.members.insert(p.b.clone(), member(&p.b, "m1"));
    state.failover = Failover::Off;
    state
}

pub(crate) fn rig_with(specs: &[Spec], proof: bool) -> Rig {
    let p = pair(specs, &|_| {});
    let clock = Arc::new(AtomicU64::new(BOOT_MS));
    let restore = Arc::new(Editing {
        inner: p.to.restore.clone(),
        after: Mutex::new(None),
    });
    let mover = Mover::new(MoveParts {
        link: p.to.link.clone(),
        acts: p.to.acts.clone(),
        rows: p.to.rows.clone(),
        snaps: p.to.snaps.clone(),
        inbox: p.to.inbox.clone(),
        gate: Arc::new(MoveGate::default()),
        cluster: p.to.cluster.clone(),
        clock: Arc::new(crate::snap_files_tests::now_ms),
        ready: Arc::new(|_, _| Ok(())),
        restore: restore.clone(),
    });
    let cluster = Arc::new(FakeCluster {
        state: Mutex::new(state_of(&p)),
        link: p.to.link.clone(),
        answers: Mutex::default(),
        calls: Mutex::default(),
        records: Mutex::default(),
        record_fail: Mutex::default(),
        log: p.log.clone(),
    });
    let replier = Arc::new(FakeReplier::default());
    let acting = Arc::new(Acting::in_memory(true));
    let now = clock.clone();
    let promoter = Promoter::with(
        PromoteParts {
            mover: mover.clone(),
            cluster: cluster.clone(),
            replier: replier.clone(),
            acting: acting.clone(),
            boot: Boot {
                id: "boot".to_string(),
                now: Arc::new(move || now.load(Ordering::SeqCst)),
            },
        },
        proof,
    );
    Rig {
        p,
        cluster,
        promoter,
        replier,
        acting,
        restore,
        clock,
    }
}

pub(crate) fn rig() -> Rig {
    rig_with(&one(), true)
}
