use crate::snap_files::{stage_set, StageMeta};
use crate::snap_repo::read_repo;
use crate::snap_send::SnapSender;
use notchling_engine::cluster::core::SnapAsk;
use notchling_engine::cluster::persist::CLUSTER_DIR;
use notchling_engine::cluster::snapshot::{plan_set, smallest_acked, QUIET_MS};
use notchling_engine::control::Launcher;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

pub const OUTGOING: &str = "outgoing";
pub const RETRY: Duration = Duration::from_secs(2);
pub const TRIES: usize = 3;

pub struct Context {
    pub launcher: Arc<dyn Launcher>,
    pub home: PathBuf,
    pub data: PathBuf,
}

impl Context {
    pub fn outgoing(&self) -> PathBuf {
        self.data.join(CLUSTER_DIR).join(OUTGOING)
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn is_set_dir(name: &str) -> bool {
    name.strip_prefix("s-")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

pub fn prune(outgoing: &Path, pinned: &BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(outgoing) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let plain_dir = entry
            .file_type()
            .is_ok_and(|t| t.is_dir() && !t.is_symlink());
        if plain_dir && is_set_dir(&name) && !pinned.contains(&name) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

pub fn take(
    sender: &SnapSender,
    ctx: &Context,
    ask: &SnapAsk,
    now_ms: i64,
) -> Result<Option<String>, String> {
    let targets = sender.ready_targets();
    if targets.is_empty() {
        return Ok(None);
    }
    let mut repos = BTreeMap::new();
    for session in &ask.main {
        if let Ok(facts) = read_repo(Path::new(&session.cwd), ctx.launcher.as_ref()) {
            repos.insert(session.id.clone(), facts);
        }
    }
    let book = sender.book().all();
    let lengths: Vec<_> = targets
        .iter()
        .map(|t| book.get(t).map(|a| a.lengths.clone()).unwrap_or_default())
        .collect();
    let plan = plan_set(
        &ask.main,
        &repos,
        &ctx.home,
        &ctx.data,
        ask.seq,
        ask.trigger.clone(),
        &smallest_acked(&lengths),
    );
    let meta = StageMeta {
        seq: ask.seq,
        term: ask.term.clone(),
        home: ask.home.clone(),
        created_ms: now_ms,
        quiet_ms: QUIET_MS,
        now_ms,
    };
    let outgoing = ctx.outgoing();
    let manifest = stage_set(&plan, &outgoing, &meta)?;
    let set = manifest.set.clone();
    let dir = outgoing.join(&set);
    sender.offer(manifest, dir);
    prune(&outgoing, &sender.pinned());
    Ok(Some(set))
}

pub const KEEP_OUTCOMES: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Staged(String),
    NoReadyTarget,
    Superseded,
    Failed(String),
}

#[derive(Default)]
struct State {
    pending: Option<SnapAsk>,
    busy: bool,
    stop: bool,
    error: Option<String>,
    last: Option<String>,
    outcomes: BTreeMap<u64, Outcome>,
}

impl State {
    fn record(&mut self, seq: u64, outcome: Outcome) {
        self.outcomes.insert(seq, outcome);
        while self.outcomes.len() > KEEP_OUTCOMES {
            self.outcomes.pop_first();
        }
    }
}

struct Shared {
    sender: Arc<SnapSender>,
    ctx: Context,
    retry: Duration,
    state: Mutex<State>,
    wake: Condvar,
}

pub struct SnapRunner {
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl SnapRunner {
    pub fn spawn(sender: Arc<SnapSender>, ctx: Context, retry: Duration) -> SnapRunner {
        prune(&ctx.outgoing(), &BTreeSet::new());
        let shared = Arc::new(Shared {
            sender,
            ctx,
            retry,
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
        });
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
            .name("snap-stage".into())
            .spawn(move || worker.run())
            .ok();
        SnapRunner {
            shared,
            thread: Mutex::new(thread),
        }
    }

    pub fn submit(&self, ask: SnapAsk) {
        lock(&self.shared.state).pending = Some(ask);
        self.shared.wake.notify_all();
    }

    pub fn last_error(&self) -> Option<String> {
        lock(&self.shared.state).error.clone()
    }

    pub fn outcome(&self, seq: u64) -> Option<Outcome> {
        lock(&self.shared.state).outcomes.get(&seq).cloned()
    }

    pub fn last_set(&self) -> Option<String> {
        lock(&self.shared.state).last.clone()
    }

    pub fn wait_idle(&self, within: Duration) -> bool {
        let deadline = std::time::Instant::now() + within;
        let mut state = lock(&self.shared.state);
        while state.busy || state.pending.is_some() {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return false;
            }
            state = self
                .shared
                .wake
                .wait_timeout(state, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        true
    }

    pub fn close(&self) {
        lock(&self.shared.state).stop = true;
        self.shared.wake.notify_all();
        if let Some(thread) = lock(&self.thread).take() {
            let _ = thread.join();
        }
    }
}

impl Shared {
    fn run(&self) {
        loop {
            let ask = {
                let mut state = lock(&self.state);
                while state.pending.is_none() && !state.stop {
                    state = self.wake.wait(state).unwrap_or_else(|e| e.into_inner());
                }
                if state.stop {
                    return;
                }
                state.busy = true;
                state.pending.take()
            };
            if let Some(ask) = ask {
                self.stage(&ask);
            }
            lock(&self.state).busy = false;
            self.wake.notify_all();
        }
    }

    fn stage(&self, ask: &SnapAsk) {
        for attempt in 1..=TRIES {
            let now = crate::acting_ops::now_ms();
            match take(&self.sender, &self.ctx, ask, now) {
                Ok(set) => {
                    let mut state = lock(&self.state);
                    state.error = None;
                    let outcome = match set {
                        Some(set) => {
                            state.last = Some(set.clone());
                            Outcome::Staged(set)
                        }
                        None => Outcome::NoReadyTarget,
                    };
                    state.record(ask.seq, outcome);
                    return;
                }
                Err(why) => {
                    let mut state = lock(&self.state);
                    state.error = Some(why.clone());
                    if state.pending.is_some() || state.stop {
                        state.record(ask.seq, Outcome::Superseded);
                        return;
                    }
                    if attempt == TRIES {
                        state.record(ask.seq, Outcome::Failed(why));
                        return;
                    }
                    let (mut state, _) = self
                        .wake
                        .wait_timeout_while(state, self.retry, |s| s.pending.is_none() && !s.stop)
                        .unwrap_or_else(|e| e.into_inner());
                    if state.pending.is_some() || state.stop {
                        state.record(ask.seq, Outcome::Superseded);
                        return;
                    }
                }
            }
        }
    }
}

#[cfg(all(test, unix))]
#[path = "snap_run_tests.rs"]
mod tests;
