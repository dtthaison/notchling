use crate::cluster_move::lock;
use crate::cluster_returns::Returns;
use crate::cluster_returns_claim::{transcript_len, CLAIM_WAIT};
use notchling_engine::cluster::claims::Claim;
use notchling_engine::cluster::returns::{on_step_down, LocalCopy};
use notchling_engine::cluster::state::{ClusterState, Command};
use notchling_engine::cluster::wire::OriginTier;
use notchling_platform::files::{ensure_private_dir, write_private_no_follow};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const MARKER_FILE: &str = "was-primary.json";
pub const EVERY: Duration = Duration::from_secs(5);

fn read_marker(cluster: &Path) -> Option<u64> {
    let bytes = std::fs::read(cluster.join(MARKER_FILE)).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    value["term"].as_u64()
}

impl Returns {
    pub fn tick(self: &Arc<Self>) {
        self.step_down();
        self.sync_waiting();
        self.touch_working();
    }

    fn note_home(&self, term: u64) {
        if self.remember(|m| m.marker_term) == Some(term) {
            return;
        }
        let bytes = serde_json::to_vec(&json!({"term": term})).unwrap_or_default();
        let cluster = self.cluster();
        let written = ensure_private_dir(cluster)
            .and_then(|()| write_private_no_follow(&cluster.join(MARKER_FILE), &bytes));
        if written.is_ok() {
            self.remember(|m| m.marker_term = Some(term));
        }
    }

    fn local_copy(&self, state: &ClusterState, session: &str) -> LocalCopy {
        let main = state.main.get(session);
        let path = main
            .and_then(|m| m.transcript.clone())
            .filter(|p| Path::new(p).is_file());
        let len = path.as_deref().map_or(0, transcript_len);
        LocalCopy {
            len,
            folder_changed: self.folder_changed(state, session),
            path,
            at_ms: 0,
        }
    }

    fn folder_changed(&self, state: &ClusterState, session: &str) -> bool {
        let (Some(main), Some(claim)) = (state.main.get(session), state.claims.get(session)) else {
            return true;
        };
        let was = self
            .p
            .mover
            .p
            .snaps
            .latest()
            .filter(|manifest| manifest.set == claim.base_set)
            .and_then(|manifest| manifest.sessions.into_iter().find(|e| e.id == session))
            .and_then(|entry| entry.repo);
        self.p.repo.changed(&main.cwd, was.as_ref())
    }

    fn step_down(self: &Arc<Self>) {
        let (Some(state), Some(me)) = (self.load(), self.p.mover.me()) else {
            return;
        };
        if state.home == me {
            self.note_home(state.term.n);
            self.withdraw_back_except(None);
            self.remember(|m| m.copied.clear());
            return;
        }
        let was = read_marker(self.cluster());
        if was.is_none_or(|term| state.term.n <= term) {
            return;
        }
        let Some(newer) = state.members.get(&state.home) else {
            return;
        };
        self.withdraw_back_except(Some(state.term.n));
        self.primary_back(state.term.n, newer);
        let fresh: Vec<String> = state
            .main
            .keys()
            .filter(|id| {
                state.claims.get(*id).is_some_and(|c| {
                    c.node != me && !self.remember(|m| m.copied.contains(&(c.session.clone(), c.n)))
                })
            })
            .cloned()
            .collect();
        if fresh.is_empty() {
            return;
        }
        let claims: Vec<Claim> = state.claims.values().cloned().collect();
        let local = |session: &str| self.local_copy(&state, session);
        let out = on_step_down(&me, newer, &fresh, &claims, &local);
        let mut unrecorded = Vec::new();
        for copy in out.side_copies {
            let session = copy.session.clone();
            let command = Command::SideCopy { copy };
            if let Err(why) = self
                .p
                .mover
                .p
                .link
                .record(command, OriginTier::Node, CLAIM_WAIT)
            {
                let title = format!("Couldn't keep this machine's copy of {session}");
                self.failed(&format!("side-copy-failed:{session}"), &title, &why.message);
                unrecorded.push(session);
            }
        }
        let settled = fresh
            .iter()
            .filter(|s| !unrecorded.contains(s) && !self.p.mover.live(s));
        for session in settled {
            if let Some(claim) = state.claims.get(session) {
                self.remember(|m| m.copied.insert((session.clone(), claim.n)));
            }
        }
    }
}

pub struct ReturnsThread {
    stop: Mutex<Option<Sender<()>>>,
}

impl ReturnsThread {
    pub fn spawn(returns: Arc<Returns>, every: Duration) -> ReturnsThread {
        let (stop, stopped) = channel::<()>();
        let spawned = std::thread::Builder::new()
            .name("cluster-returns".into())
            .spawn(move || {
                while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(every) {
                    returns.tick();
                }
            });
        ReturnsThread {
            stop: Mutex::new(spawned.ok().map(|_| stop)),
        }
    }

    pub fn close(&self) {
        lock(&self.stop).take();
    }
}
