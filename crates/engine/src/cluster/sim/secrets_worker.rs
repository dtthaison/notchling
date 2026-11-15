use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::secrets::{Path, Seen, StaleAccept, Stored, GRANT_LIFE_MS, JANITOR_MS, PENDING_MS};
use super::world::World;
use crate::cluster::id::NodeId;
use crate::cluster::jobs::JobState;
use crate::cluster::members::Term;
use crate::cluster::wire::{Origin, OriginTier, WireError};

fn refusal(why: impl Into<String>) -> WireError {
    WireError::new("forbidden", why)
}

fn live(state: JobState) -> bool {
    matches!(state, JobState::Running | JobState::Dispatching)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Standing {
    Live,
    Pending,
}

impl World {
    pub(super) fn secrets_current_term(&self, own: &Term) -> Term {
        let top = self.seen.values().max().cloned();
        match top {
            Some(t) if t > *own => t,
            _ => own.clone(),
        }
    }

    pub(super) fn held_term(&self) -> Option<Term> {
        self.nodes
            .values()
            .filter_map(|n| n.core.as_ref().and_then(|c| c.highest_term().cloned()))
            .max()
    }

    pub(super) fn live_here(&self, node: &NodeId, job: &str) -> bool {
        let key = (node.clone(), job.to_string());
        self.secrets.started_on.contains(&key) && !self.secrets.local_ended.contains_key(&key)
    }

    pub(super) fn standing(&self, node: &NodeId, job: &str) -> Result<Standing, &'static str> {
        let Some(state) = self.state_of(node) else {
            return Err("the job isn't running");
        };
        match state.jobs.get(job) {
            Some(found) if found.spec.node == *node && live(found.state) => Ok(Standing::Live),
            Some(found) if found.spec.node == *node => Err("the job isn't running"),
            Some(_) => Err("the job isn't running on this machine"),
            None if self.live_here(node, job) => Ok(Standing::Live),
            None => Ok(Standing::Pending),
        }
    }

    fn end_here(&mut self, node: &NodeId, job: &str) {
        let key = (node.clone(), job.to_string());
        self.secrets
            .local_ended
            .entry(key.clone())
            .or_insert(self.now);
        self.secrets.holds.remove(&key);
        self.secrets.hold_log.remove(&key);
    }

    pub(super) fn b_revoke_job(&mut self, node: &NodeId, call: u64, args: &Value) {
        let job = args.get("job").and_then(Value::as_str);
        let target = args
            .get("node")
            .and_then(|n| serde_json::from_value::<NodeId>(n.clone()).ok());
        let kept = self.secrets.plant.keep_grants || self.secrets.drop_revokes;
        if let (Some(job), Some(target), false) = (job, target, kept) {
            self.forward_node_op(node, &target, "grant.revoke", json!({"job": job}));
        }
        self.finish_local(node, call, Ok(json!({})));
    }

    pub(super) fn w_deliver(
        &mut self,
        node: &NodeId,
        call: u64,
        (origin, args): (&Origin, &Value),
        arrived: (u64, u64),
    ) {
        let state = self.state_of(node);
        let from_vault_home = state
            .as_ref()
            .is_some_and(|s| origin.node == s.vault && s.home == s.vault);
        let err = |code: &str, why: &str| Err(WireError::new(code, why));
        let job = args.get("job").and_then(Value::as_str);
        let name = args
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_lowercase);
        let value = args.get("value").and_then(Value::as_str);
        let asked = args.get("expiresMs").and_then(Value::as_i64);
        let (Some(job), Some(name), Some(value), Some(asked)) = (job, name, value, asked) else {
            let bad = err(
                "bad_request",
                "grant.deliver takes {job, name, value, expiresMs}",
            );
            self.finish_local(node, call, bad);
            return;
        };
        if !from_vault_home {
            let why = "only the vault node, while it is the primary, delivers a secret";
            self.finish_local(node, call, err("forbidden", why));
            return;
        }
        let anything = self.secrets.plant.worker_accepts_anything;
        let key = (node.clone(), job.to_string());
        if self.secrets.local_ended.contains_key(&key) && !anything {
            self.finish_local(node, call, err("failed", "the job isn't running"));
            return;
        }
        let now = self.now as i64;
        let expires = asked.min(now.saturating_add(GRANT_LIFE_MS as i64));
        if expires <= now {
            self.finish_local(node, call, err("failed", "the grant has already expired"));
            return;
        }
        let revoked = self.secrets.revokes.iter().any(|(at, n, j, names)| {
            *at > arrived.1
                && n == node
                && j == job
                && names.as_ref().is_none_or(|l| l.contains(&name))
        });
        if revoked {
            self.finish_local(node, call, err("failed", "the job isn't running"));
            return;
        }
        let standing = match self.standing(node, job) {
            Ok(standing) => standing,
            Err(_) if anything => Standing::Live,
            Err(why) => {
                self.finish_local(node, call, err("failed", why));
                return;
            }
        };
        let (intended, release) = match self.secrets.mark_of(value) {
            Some(m) => (Some(m.target.clone()), m.release),
            None => (None, None),
        };
        if let Some(rid) = release {
            let released = self.secrets.releases[rid].term.clone();
            let own = self.core(node).and_then(|c| c.highest_term().cloned());
            let elsewhere = self.held_term();
            if own.is_some_and(|held| released < held) {
                self.secrets.stale_accepts.push(StaleAccept {
                    node: node.clone(),
                    release: released,
                });
            } else if elsewhere.is_some_and(|held| released < held) {
                self.secrets.stats.split_accepts += 1;
            }
        }
        let stored = Stored {
            expires_at: expires as u64,
            stored_at: self.now,
            value: value.to_string(),
            intended,
            release,
        };
        let grant = (job.to_string(), name.clone());
        self.secrets
            .stores
            .entry(node.clone())
            .or_default()
            .insert(grant, stored);
        self.secrets.record(node, Path::Store, value);
        self.secrets.stats.stored += 1;
        if standing == Standing::Pending {
            let now = self.now;
            for book in [&mut self.secrets.holds, &mut self.secrets.hold_log] {
                book.entry(key.clone())
                    .and_modify(|since| {
                        if now.saturating_sub(*since) >= PENDING_MS {
                            *since = now;
                        }
                    })
                    .or_insert(now);
            }
        }
        self.finish_local(node, call, Ok(json!({"stored": name})));
    }

    pub(super) fn w_revoke(&mut self, node: &NodeId, call: u64, origin: &Origin, args: &Value) {
        let allowed = self
            .state_of(node)
            .is_some_and(|s| origin.node == s.home || origin.node == s.vault);
        let Some(job) = args.get("job").and_then(Value::as_str).filter(|_| allowed) else {
            let err = refusal("only the home or the vault node revokes a secret");
            return self.finish_local(node, call, Err(err));
        };
        let names: Option<Vec<String>> = args.get("names").and_then(Value::as_array).map(|l| {
            l.iter()
                .filter_map(|n| n.as_str().map(str::to_lowercase))
                .collect()
        });
        self.secrets.revoke_seq += 1;
        let seq = self.secrets.revoke_seq;
        self.secrets
            .revokes
            .push((seq, node.clone(), job.to_string(), names.clone()));
        let store = self.secrets.stores.entry(node.clone()).or_default();
        let before = store.len();
        match &names {
            None => store.retain(|(j, _), _| j != job),
            Some(list) => store.retain(|(j, n), _| !(j == job && list.contains(n))),
        }
        let removed = (before - store.len()) as u64;
        self.secrets.stats.revoked += removed;
        if names.is_none() {
            let key = (node.clone(), job.to_string());
            self.secrets.holds.remove(&key);
            self.secrets.hold_log.remove(&key);
        }
        self.finish_local(node, call, Ok(json!({})));
    }

    fn seen_here(&mut self, n: &NodeId) -> (Seen, BTreeSet<String>) {
        let now = self.now;
        self.secrets
            .holds
            .retain(|(node, _), since| node != n || now.saturating_sub(*since) < PENDING_MS);
        let Some(state) = self.state_of(n) else {
            return (Seen::default(), BTreeSet::new());
        };
        let mine: std::collections::BTreeMap<String, bool> = state
            .jobs
            .values()
            .filter(|j| j.spec.node == *n)
            .map(|j| (j.id.clone(), live(j.state)))
            .collect();
        let alive = |job: &String| !state.jobs.contains_key(job) && self.live_here(n, job);
        let local_live: BTreeSet<String> = self
            .secrets
            .started_on
            .iter()
            .filter(|(node, _)| node == n)
            .map(|(_, job)| job.clone())
            .filter(alive)
            .collect();
        let pending: BTreeSet<String> = self
            .secrets
            .holds
            .keys()
            .filter(|(node, job)| node == n && !state.jobs.contains_key(job))
            .map(|(_, job)| job.clone())
            .collect();
        let held: BTreeSet<String> = mine
            .iter()
            .filter(|(_, up)| **up)
            .map(|(job, _)| job.clone())
            .chain(local_live.iter().cloned())
            .chain(pending.iter().cloned())
            .collect();
        let seen = Seen {
            mine,
            live: local_live,
            pending,
        };
        (seen, held)
    }

    pub(super) fn secrets_tick(&mut self, n: &NodeId) {
        let now = self.now;
        self.secrets_resync(n);
        let (seen, held) = self.seen_here(n);
        let changed = self.secrets.live_seen.get(n) != Some(&seen);
        let due = self.secrets.due.get(n).copied().unwrap_or(0);
        if !changed && now < due {
            return;
        }
        self.secrets.live_seen.insert(n.clone(), seen);
        self.secrets.due.insert(n.clone(), now + JANITOR_MS);
        if self.secrets.plant.keep_grants {
            return;
        }
        let s = &mut self.secrets;
        let Some(store) = s.stores.get_mut(n) else {
            return;
        };
        let before = store.len();
        store.retain(|(job, _), g| g.expires_at > now && held.contains(job));
        s.stats.revoked += (before - store.len()) as u64;
    }

    pub(super) fn observe_jobs(&mut self, n: &NodeId) {
        let Some(state) = self.state_of(n) else {
            return;
        };
        let now = self.now;
        for job in state.jobs.values() {
            if job.spec.node != *n || !live(job.state) {
                self.secrets
                    .done_seen
                    .entry((n.clone(), job.id.clone()))
                    .or_insert(now);
            }
        }
    }

    pub(super) fn secrets_started(
        &mut self,
        n: &NodeId,
        (op, args, origin): (&str, &Value, &Origin),
        ok: bool,
    ) {
        self.secrets.record(n, Path::Argv, &args.to_string());
        let Some(job) = args.get("job").and_then(Value::as_str) else {
            return;
        };
        let key = (n.clone(), job.to_string());
        let from_home = origin.tier == OriginTier::App
            && self
                .state_of(n)
                .is_some_and(|state| origin.node == state.home);
        let ended = self.secrets.local_ended.contains_key(&key);
        let noted = match op {
            "shell.run" => ok && !ended,
            "session.new" | "agy.send" => from_home && !ended,
            _ => false,
        };
        if !noted {
            return;
        }
        if !ok {
            self.end_here(n, job);
            self.secrets.started_on.remove(&key);
            if let Some(store) = self.secrets.stores.get_mut(n) {
                store.retain(|(j, _), _| j != job);
            }
            return;
        }
        self.secrets.started_on.insert(key.clone());
        self.secrets.holds.remove(&key);
        self.secrets.hold_log.remove(&key);
        let names: Vec<String> = self
            .secrets
            .stores
            .get(n)
            .map(|s| {
                s.keys()
                    .filter(|(j, _)| j == job)
                    .map(|(_, name)| name.clone())
                    .collect()
            })
            .unwrap_or_default();
        self.consume_grants(n, job, &names);
    }

    pub(super) fn reap_here(&mut self, n: &NodeId, job: &str) {
        self.end_here(n, job);
    }

    pub(super) fn consume_grants(&mut self, n: &NodeId, job: &str, names: &[String]) {
        if self.standing(n, job) != Ok(Standing::Live) {
            return;
        }
        let now = self.now;
        for name in names {
            let held = self
                .secrets
                .stores
                .get(n)
                .and_then(|s| s.get(&(job.to_string(), name.clone())))
                .filter(|g| g.expires_at > now)
                .map(|g| g.value.clone());
            let Some(value) = held else {
                continue;
            };
            self.secrets.record(n, Path::ChildEnv, &value);
            self.secrets.child_env += 1;
            self.secrets.stats.consumed += 1;
            if self.secrets.plant.child_echo {
                self.push_trace(n, &format!("child printed {value}"));
            }
        }
    }
}
