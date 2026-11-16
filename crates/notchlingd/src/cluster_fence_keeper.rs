use crate::cluster_fence::{fence_keys, fence_path, sync_gates, Wanted};
use crate::cluster_move::{raise_notice, Acts};
use crate::hook_fence::{look_all, real_now};
use crate::inbox_ops::InboxOps;
use notchling_engine::cluster::failover::PrimaryLease;
use notchling_engine::cluster::fence::{Fence, Gate};
use notchling_engine::cluster::fence_gates::{
    gates, lease_target, may_move, valid_until, LocalSession, SetView,
};
use notchling_engine::cluster::main_sessions::tool_of_row;
use notchling_engine::cluster::persist;
use notchling_engine::cluster::state::ClusterState;
use notchling_engine::cluster::text::{plain, NAME_MAX};
use notchling_engine::cluster::{NodeId, Tool};
use notchling_engine::inbox::Kind;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

pub const EVERY: Duration = Duration::from_secs(2);
pub const STOP_TRIES: u32 = 3;
const HISTORY: usize = 64;
const WRITE_NOTICE: &str = "cluster-fence-write";

pub trait Feed: Send + Sync {
    fn me(&self) -> Option<NodeId>;
    fn ready(&self) -> BTreeSet<NodeId>;
    fn latest_set(&self) -> Option<SetView>;
    fn acked(&self, node: &NodeId) -> Option<String>;
    fn waiting(&self) -> BTreeMap<String, String>;
    fn live(&self, id: &str) -> bool;
}

pub trait CodexTurns: Send + Sync {
    fn running(&self) -> Vec<String>;
    fn stop(&self, session: &str);
}

#[derive(Clone)]
pub struct Boot {
    pub id: String,
    pub now: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl Boot {
    pub fn real() -> Boot {
        Boot {
            id: notchling_platform::clock::boot_id().unwrap_or_default(),
            now: Arc::new(real_now),
        }
    }
}

pub struct KeeperParts {
    pub feed: Arc<dyn Feed>,
    pub acts: Arc<dyn Acts>,
    pub inbox: Arc<InboxOps>,
    pub boot: Boot,
}

#[derive(Default)]
struct Memory {
    history: Vec<SetView>,
    stops: BTreeMap<(String, String, u64), u32>,
    error: Option<String>,
}

pub struct Keeper {
    cluster: PathBuf,
    p: KeeperParts,
    lease: Mutex<PrimaryLease>,
    memory: Mutex<Memory>,
    codex: OnceLock<Arc<dyn CodexTurns>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn name_in(state: &ClusterState, node: &NodeId) -> String {
    let name = state
        .members
        .get(node)
        .map(|m| plain(&m.name, NAME_MAX))
        .filter(|name| !name.is_empty());
    name.unwrap_or_else(|| "that machine".to_string())
}

fn compute(
    state: &ClusterState,
    me: &NodeId,
    sessions: &[LocalSession],
    waiting: &BTreeMap<String, String>,
) -> BTreeMap<String, Gate> {
    gates(me, state, sessions, waiting, &|node| name_in(state, node))
}

fn sessions_of(state: &ClusterState, lease: &dyn Fn(&str) -> bool) -> Vec<LocalSession> {
    state
        .main
        .keys()
        .map(|id| LocalSession {
            id: id.clone(),
            keys: fence_keys(&state.main, std::slice::from_ref(id)),
            lease: lease(id),
        })
        .collect()
}

pub fn write_at_start(cluster: &Path, me: &NodeId) -> Result<(), String> {
    let Some(state) = persist::load(cluster).ok().flatten() else {
        return Ok(());
    };
    let may = lease_target(&state, &|_| true).is_some();
    let sessions = sessions_of(&state, &|_| may);
    let gates = compute(&state, me, &sessions, &BTreeMap::new());
    let want = Wanted {
        me,
        term: state.term.clone(),
        cluster: &state.id,
        valid_until: valid_until(&state, &PrimaryLease::default()),
        gates: &gates,
    };
    sync_gates(cluster, &want).map(|_| ())
}

impl Keeper {
    pub fn new(cluster: PathBuf, parts: KeeperParts) -> Arc<Keeper> {
        Arc::new(Keeper {
            cluster,
            p: parts,
            lease: Mutex::new(PrimaryLease::default()),
            memory: Mutex::new(Memory::default()),
            codex: OnceLock::new(),
        })
    }

    pub fn lease(&self) -> MutexGuard<'_, PrimaryLease> {
        lock(&self.lease)
    }

    pub fn set_codex(&self, codex: Arc<dyn CodexTurns>) {
        let _ = self.codex.set(codex);
    }

    fn note_set(&self) -> Vec<SetView> {
        let latest = self.p.feed.latest_set();
        let mut memory = lock(&self.memory);
        if let Some(view) = latest {
            if memory.history.last().map(|v| &v.set) != Some(&view.set) {
                memory.history.push(view);
                let extra = memory.history.len().saturating_sub(HISTORY);
                memory.history.drain(..extra);
            }
        }
        memory.history.clone()
    }

    fn leases(&self, state: &ClusterState) -> impl Fn(&str) -> bool {
        let ready = self.p.feed.ready();
        let target = lease_target(state, &|node| ready.contains(node));
        let acked = target.as_ref().and_then(|node| self.p.feed.acked(node));
        let history = self.note_set();
        move |id: &str| target.is_some() && may_move(&history, acked.as_deref(), id)
    }

    pub fn tick(&self) {
        let Some(state) = persist::load(&self.cluster).ok().flatten() else {
            return;
        };
        let Some(me) = self.p.feed.me() else {
            return;
        };
        let sessions = sessions_of(&state, &self.leases(&state));
        let gates = compute(&state, &me, &sessions, &self.p.feed.waiting());
        let until = valid_until(&state, &lock(&self.lease));
        let want = Wanted {
            me: &me,
            term: state.term.clone(),
            cluster: &state.id,
            valid_until: until,
            gates: &gates,
        };
        match sync_gates(&self.cluster, &want) {
            Ok(_) => lock(&self.memory).error = None,
            Err(why) => self.report(&why),
        }
        self.step_down(&state, &me);
        self.interrupt_codex();
    }

    fn report(&self, why: &str) {
        {
            let mut memory = lock(&self.memory);
            if memory.error.as_deref() == Some(why) {
                return;
            }
            memory.error = Some(why.to_string());
        }
        let detail = format!("{why}. Sessions that moved may still run here.");
        self.notice(
            WRITE_NOTICE,
            "Couldn't update this machine's session fence",
            &detail,
        );
    }

    fn notice(&self, id: &str, title: &str, detail: &str) {
        let now = crate::acting_ops::now_ms();
        raise_notice(&self.p.inbox, now, id, (Kind::ActionFailed, title, detail));
    }

    fn is_live(&self, session: &str) -> bool {
        match tool_of_row(session) {
            Tool::Codex => self.codex.get().is_some_and(|codex| {
                codex
                    .running()
                    .iter()
                    .any(|thread| session == format!("codex:{thread}"))
            }),
            _ => self.p.feed.live(session),
        }
    }

    fn step_down(&self, state: &ClusterState, me: &NodeId) {
        lock(&self.memory).stops.retain(|(session, owner, n), _| {
            state
                .claims
                .get(session)
                .is_some_and(|c| c.n == *n && c.node.as_str() == owner)
        });
        for claim in state.claims.values().filter(|c| c.node != *me) {
            let key = (claim.session.clone(), claim.node.to_string(), claim.n);
            if lock(&self.memory).stops.get(&key).copied().unwrap_or(0) >= STOP_TRIES {
                continue;
            }
            if !self.is_live(&claim.session) {
                continue;
            }
            *lock(&self.memory).stops.entry(key.clone()).or_default() += 1;
            let stopped = match tool_of_row(&claim.session) {
                Tool::Codex => {
                    self.p.acts.stop_codex(&claim.session);
                    Ok(())
                }
                _ => self.p.acts.stop_claude(&claim.session),
            };
            match stopped {
                Ok(()) => {
                    lock(&self.memory).stops.insert(key, STOP_TRIES);
                }
                Err(why) => self.notice(
                    &format!("cluster-fence-stop-{}", claim.session),
                    &format!("Couldn't stop {} on this machine", claim.session),
                    &format!(
                        "It moved to {}, but it may still run here: {why}",
                        name_in(state, &claim.node)
                    ),
                ),
            }
        }
    }

    fn interrupt_codex(&self) {
        let Some(codex) = self.codex.get() else {
            return;
        };
        let running = codex.running();
        if running.is_empty() {
            return;
        }
        let path = fence_path(&self.cluster);
        let now = (self.p.boot.now)();
        for thread in running {
            let row = format!("codex:{thread}");
            if look_all(&path, &[&thread, &row], &self.p.boot.id, now) != Fence::Pass {
                codex.stop(&thread);
            }
        }
    }
}

pub struct KeeperThread {
    stop: Mutex<Option<Sender<()>>>,
}

impl KeeperThread {
    pub fn spawn(keeper: Arc<Keeper>, every: Duration) -> KeeperThread {
        let (stop, stopped) = channel::<()>();
        let spawned = std::thread::Builder::new()
            .name("cluster-fence".into())
            .spawn(move || {
                while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(every) {
                    keeper.tick();
                }
            });
        KeeperThread {
            stop: Mutex::new(spawned.ok().map(|_| stop)),
        }
    }

    pub fn close(&self) {
        lock(&self.stop).take();
    }
}
