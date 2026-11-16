use crate::cluster_redact::TailMasks;
use crate::grant_broker::{Clock, View};
use crate::grant_roots::{GrantMarks, JobRoots, Local, Marked};
use crate::grant_use::keys_here;
use notchling_engine::cluster::grant_files::GrantStore;
use notchling_engine::cluster::grants::GRANT_TTL_MS;
use notchling_engine::cluster::{ClusterState, JobState, NodeId};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub const SWEEP_EVERY: Duration = Duration::from_secs(30);
const POLL_EVERY: Duration = Duration::from_secs(1);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct GrantJanitor {
    store: Arc<GrantStore>,
    parts: (Arc<TailMasks>, Arc<GrantMarks>, Arc<JobRoots>),
    view: Arc<dyn View>,
    clock: Clock,
    last: Mutex<Option<(Mine, Local)>>,
    nudged: Mutex<bool>,
    wake: Condvar,
    stop: AtomicBool,
}

type Mine = BTreeMap<String, (JobState, Option<String>)>;

#[derive(Default)]
struct Seen {
    state: Option<ClusterState>,
    mine: Mine,
    local: Local,
}

fn mine(state: &ClusterState, me: &NodeId) -> Mine {
    state
        .jobs
        .values()
        .filter(|j| j.spec.node == *me)
        .map(|j| (j.id.clone(), (j.state, j.session.clone())))
        .collect()
}

fn live_state(state: JobState) -> bool {
    matches!(state, JobState::Running | JobState::Dispatching)
}

fn live_of(jobs: &Mine) -> BTreeSet<String> {
    jobs.iter()
        .filter(|(_, (s, _))| live_state(*s))
        .map(|(id, _)| id.clone())
        .collect()
}

impl GrantJanitor {
    pub fn new(
        store: Arc<GrantStore>,
        parts: (Arc<TailMasks>, Arc<GrantMarks>, Arc<JobRoots>),
        view: Arc<dyn View>,
        clock: Clock,
    ) -> Arc<GrantJanitor> {
        Arc::new(GrantJanitor {
            store,
            parts,
            view,
            clock,
            last: Mutex::new(None),
            nudged: Mutex::new(false),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
        })
    }

    fn seen(&self, now: i64) -> Seen {
        match (self.view.state(), self.view.me()) {
            (Some(state), Some(me)) => {
                let mut local = self.parts.2.local(now);
                local.live.retain(|job, _| !state.jobs.contains_key(job));
                local.pending.retain(|job| !state.jobs.contains_key(job));
                Seen {
                    mine: mine(&state, &me),
                    local,
                    state: Some(state),
                }
            }
            (state, None) => Seen {
                state,
                ..Seen::default()
            },
            (None, Some(_)) => Seen::default(),
        }
    }

    pub fn tick(&self) -> Result<usize, String> {
        let _gate = self.parts.2.gate();
        let now = (self.clock)();
        let seen = self.seen(now);
        let live = live_of(&seen.mine);
        let held: BTreeSet<String> = live
            .iter()
            .chain(seen.local.live.keys())
            .chain(&seen.local.pending)
            .cloned()
            .collect();
        let swept = self.store.sweep(now, &held);
        for job in self.parts.2.rooted() {
            if seen.mine.get(&job).is_some_and(|(s, _)| !live_state(*s)) {
                self.parts.2.end(&job);
            }
        }
        if let Some(state) = &seen.state {
            for marked in self.parts.1.marked() {
                self.tend(state, (&live, &seen.local), &marked, now);
            }
        }
        *lock(&self.last) = Some((seen.mine, seen.local));
        swept
    }

    fn tend(
        &self,
        state: &ClusterState,
        (live, local): (&BTreeSet<String>, &Local),
        marked: &Marked,
        now: i64,
    ) {
        let (masks, marks, roots) = &self.parts;
        let job = &marked.job;
        if live.contains(job) || local.live.contains_key(job) {
            let keys = keys_here(Some(state), roots, job);
            if keys.iter().any(|k| !marked.keys.contains(k)) && marks.mark_with(job, &keys).is_ok()
            {
                masks.mark_granted_as(job, &keys);
            }
            return;
        }
        if local.pending.contains(job) {
            return;
        }
        masks.mark_granted_as(job, &marked.keys);
        masks.end_job(job);
        match marked.ended_ms {
            None => {
                let _ = marks.stamp_end(job, now);
            }
            Some(at) if now.saturating_sub(at) >= GRANT_TTL_MS && masks.lost(job) => {
                masks.release_blanket(job);
            }
            Some(_) => {}
        }
    }

    pub(crate) fn changed(&self) -> bool {
        let seen = self.seen((self.clock)());
        lock(&self.last).as_ref() != Some(&(seen.mine, seen.local))
    }

    pub fn nudge(&self) {
        *lock(&self.nudged) = true;
        self.wake.notify_all();
    }

    fn wait(&self) {
        let mut nudged = lock(&self.nudged);
        if !*nudged {
            nudged = self
                .wake
                .wait_timeout(nudged, POLL_EVERY)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        *nudged = false;
    }

    pub fn spawn(self: &Arc<GrantJanitor>) {
        let janitor = self.clone();
        let spawned = std::thread::Builder::new()
            .name("grant-janitor".into())
            .spawn(move || {
                let mut swept = Instant::now();
                let _ = janitor.tick();
                while !janitor.stop.load(Ordering::SeqCst) {
                    janitor.wait();
                    if janitor.stop.load(Ordering::SeqCst) {
                        break;
                    }
                    if janitor.changed() || swept.elapsed() >= SWEEP_EVERY {
                        let _ = janitor.tick();
                        swept = Instant::now();
                    }
                }
            });
        if spawned.is_err() {
            self.stop.store(true, Ordering::SeqCst);
        }
    }

    pub fn close(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.nudge();
    }
}

#[cfg(test)]
#[path = "grant_janitor_tests.rs"]
mod tests;
