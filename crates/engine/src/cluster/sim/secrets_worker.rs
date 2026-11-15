use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::secrets::{Path, StaleAccept, Stored, GRANT_LIFE_MS, JANITOR_MS, KNOWN_WAIT_MS};
use super::world::World;
use crate::cluster::id::NodeId;
use crate::cluster::jobs::JobState;
use crate::cluster::members::Term;
use crate::cluster::wire::{Origin, WireError};

fn refusal(why: impl Into<String>) -> WireError {
    WireError::new("forbidden", why)
}

fn live(state: JobState) -> bool {
    matches!(state, JobState::Running | JobState::Dispatching)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Knows {
    Live,
    Ended,
    Unknown,
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

    pub(super) fn knows_job(&self, node: &NodeId, job: &str) -> Knows {
        if self
            .secrets
            .local_ended
            .contains_key(&(node.clone(), job.to_string()))
        {
            return Knows::Ended;
        }
        if self
            .secrets
            .started_on
            .contains(&(node.clone(), job.to_string()))
        {
            return Knows::Live;
        }
        let Some(own) = self.state_of(node) else {
            return Knows::Unknown;
        };
        if own.home != *node && !self.net.linked(node, &own.home) {
            return Knows::Unknown;
        }
        let home = if own.home == *node {
            Some(own)
        } else {
            self.state_of(&own.home)
        };
        match home.and_then(|h| h.jobs.get(job).cloned()) {
            Some(j) if j.spec.node == *node && live(j.state) => Knows::Live,
            Some(j) if j.spec.node == *node => Knows::Ended,
            _ => Knows::Unknown,
        }
    }

    pub(super) fn b_revoke_job(&mut self, node: &NodeId, call: u64, args: &Value) {
        let job = args.get("job").and_then(Value::as_str);
        let target = args
            .get("node")
            .and_then(|n| serde_json::from_value::<NodeId>(n.clone()).ok());
        if let (Some(job), Some(target), false) = (job, target, self.secrets.plant.keep_grants) {
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
    ) -> bool {
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
            return false;
        };
        if !from_vault_home {
            let why = "only the vault node, while it is the primary, delivers a secret";
            self.finish_local(node, call, err("forbidden", why));
            return false;
        }
        let anything = self.secrets.plant.worker_accepts_anything;
        match (self.knows_job(node, job), anything) {
            (Knows::Ended, false) => {
                let why = "the job isn't running";
                self.finish_local(node, call, err("failed", why));
                return false;
            }
            (Knows::Unknown, false) => {
                if self.now.saturating_sub(arrived.0) < KNOWN_WAIT_MS {
                    return true;
                }
                let why = "the job isn't running on this machine";
                self.finish_local(node, call, err("failed", why));
                return false;
            }
            _ => {}
        }
        let now = self.now as i64;
        let expires = asked.min(now.saturating_add(GRANT_LIFE_MS as i64));
        if expires <= now {
            self.finish_local(node, call, err("failed", "the grant has already expired"));
            return false;
        }
        let revoked = self.secrets.revokes.iter().any(|(at, n, j, names)| {
            *at > arrived.1
                && n == node
                && j == job
                && names.as_ref().is_none_or(|l| l.contains(&name))
        });
        if revoked {
            self.finish_local(node, call, err("failed", "the job isn't running"));
            return false;
        }
        let stored = Stored {
            expires_at: expires as u64,
            value: value.to_string(),
        };
        let key = (job.to_string(), name.clone());
        let release = self.secrets.released_terms.get(&key).cloned();
        if let Some(release) = release {
            let own = self.core(node).and_then(|c| c.highest_term().cloned());
            let elsewhere = self.held_term();
            if own.is_some_and(|held| release < held) {
                self.secrets.stale_accepts.push(StaleAccept {
                    node: node.clone(),
                    release,
                });
            } else if elsewhere.is_some_and(|held| release < held) {
                self.secrets.stats.split_accepts += 1;
            }
        }
        self.secrets
            .stores
            .entry(node.clone())
            .or_default()
            .insert(key, stored);
        self.secrets.record(node, Path::Store { job }, value);
        self.secrets.stats.stored += 1;
        self.finish_local(node, call, Ok(json!({"stored": name})));
        false
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
        let now = self.now;
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
            self.secrets
                .local_ended
                .entry((node.clone(), job.to_string()))
                .or_insert(now);
        }
        self.finish_local(node, call, Ok(json!({})));
    }

    pub(super) fn observe_jobs(&mut self, n: &NodeId) {
        let Some(state) = self.core(n).and_then(|c| c.state()) else {
            return;
        };
        let seen: Vec<(String, NodeId, bool)> = state
            .jobs
            .values()
            .map(|j| (j.id.clone(), j.spec.node.clone(), live(j.state)))
            .collect();
        let home = state.home.clone();
        let now = self.now;
        for (job, node, alive) in seen {
            self.secrets
                .truth
                .entry(job.clone())
                .or_default()
                .insert(node);
            if !alive && home == *n {
                self.secrets.ended.entry((home.clone(), job)).or_insert(now);
            }
        }
    }

    fn reachable(&self, n: &NodeId) -> bool {
        match self.core(n).and_then(|c| c.state()) {
            Some(s) => s.home == *n || self.net.linked(n, &s.home),
            None => false,
        }
    }

    fn told_ended(&self, n: &NodeId) -> BTreeSet<String> {
        let Some(own) = self.state_of(n) else {
            return BTreeSet::new();
        };
        if own.home != *n && !self.net.linked(n, &own.home) {
            return BTreeSet::new();
        }
        let home = if own.home == *n {
            Some(own)
        } else {
            self.state_of(&own.home)
        };
        home.map(|h| {
            h.jobs
                .values()
                .filter(|j| !live(j.state))
                .map(|j| j.id.clone())
                .collect()
        })
        .unwrap_or_default()
    }

    pub(super) fn secrets_tick(&mut self, n: &NodeId) {
        let now = self.now;
        let reachable = self.reachable(n);
        let last = self.secrets.last_tick.insert(n.clone(), now);
        let gap = last.map_or(0, |t| now.saturating_sub(t));
        if last.is_none() || gap > 2_500 {
            self.secrets.alive_since.insert(n.clone(), now);
        }
        let slot = self.secrets.reach.entry(n.clone()).or_insert(None);
        if gap > 2_500 || !reachable {
            *slot = None;
        }
        if reachable && slot.is_none() {
            *slot = Some(now);
        }
        self.secrets_resync(n);
        let due = *self
            .secrets
            .due
            .entry(n.clone())
            .or_insert(now + JANITOR_MS);
        if now < due {
            return;
        }
        self.secrets.due.insert(n.clone(), now + JANITOR_MS);
        if self.secrets.plant.keep_grants {
            return;
        }
        let told = self.told_ended(n);
        let s = &mut self.secrets;
        let Some(store) = s.stores.get_mut(n) else {
            return;
        };
        let before = store.len();
        store.retain(|(job, _), g| {
            let expired = g.expires_at <= now;
            let mine = s.local_ended.contains_key(&(n.clone(), job.clone()));
            !(expired || mine || told.contains(job))
        });
        s.stats.revoked += (before - store.len()) as u64;
    }

    pub(super) fn secrets_started(&mut self, n: &NodeId, args: &Value) {
        self.secrets.record(n, Path::Argv, &args.to_string());
        let Some(job) = args.get("job").and_then(Value::as_str) else {
            return;
        };
        self.secrets.started_on.insert((n.clone(), job.to_string()));
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

    pub(super) fn consume_grants(&mut self, n: &NodeId, job: &str, names: &[String]) {
        for name in names {
            let held = self
                .secrets
                .stores
                .get(n)
                .and_then(|s| s.get(&(job.to_string(), name.clone())))
                .map(|g| g.value.clone());
            let Some(value) = held else {
                continue;
            };
            self.secrets.record(n, Path::ChildEnv { job }, &value);
            self.secrets.child_env += 1;
            self.secrets.stats.consumed += 1;
            if self.secrets.plant.child_echo {
                self.push_trace(n, &format!("child printed {value}"));
            }
        }
    }
}
