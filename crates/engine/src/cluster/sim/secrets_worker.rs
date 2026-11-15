use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::secrets::{Path, StaleAccept, Stored, GRANT_LIFE_MS, JANITOR_MS, KNOWN_WAIT_MS};
use super::world::World;
use crate::cluster::id::NodeId;
use crate::cluster::members::Term;
use crate::cluster::wire::{Origin, WireError};

fn refusal(why: impl Into<String>) -> WireError {
    WireError::new("forbidden", why)
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

    /// What this machine's own daemon knows about a job: a start ran here,
    /// or its own process ended. A Worker's engine state holds no jobs, and
    /// nothing the home knows reaches the Worker except by a message.
    pub(super) fn knows_job(&self, node: &NodeId, job: &str) -> Knows {
        let key = (node.clone(), job.to_string());
        if self.secrets.local_ended.contains_key(&key) {
            Knows::Ended
        } else if self.secrets.started_on.contains(&key) {
            Knows::Live
        } else {
            Knows::Unknown
        }
    }

    pub(super) fn live_here(&self, node: &NodeId) -> BTreeSet<String> {
        self.secrets
            .started_on
            .iter()
            .filter(|(n, job)| {
                n == node
                    && !self
                        .secrets
                        .local_ended
                        .contains_key(&(n.clone(), job.clone()))
            })
            .map(|(_, job)| job.clone())
            .collect()
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
        let key = (job.to_string(), name.clone());
        self.secrets
            .stores
            .entry(node.clone())
            .or_default()
            .insert(key, stored);
        self.secrets.record(node, Path::Store, value);
        self.secrets.stats.stored += 1;
        self.finish_local(node, call, Ok(json!({"stored": name})));
        false
    }

    /// A revoke removes what is stored and bars the delivers that arrived
    /// before it. It does not end the job: a later fresh deliver is fine.
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
        self.finish_local(node, call, Ok(json!({})));
    }

    /// The Worker's janitor: a sweep at start, every 30 s, and when the set
    /// of jobs running here changes. It sees only this machine's jobs.
    pub(super) fn secrets_tick(&mut self, n: &NodeId) {
        let now = self.now;
        self.secrets_resync(n);
        let live = self.live_here(n);
        let changed = self.secrets.live_seen.get(n) != Some(&live);
        let due = self.secrets.due.get(n).copied().unwrap_or(0);
        if !changed && now < due {
            return;
        }
        self.secrets.live_seen.insert(n.clone(), live.clone());
        self.secrets.due.insert(n.clone(), now + JANITOR_MS);
        if self.secrets.plant.keep_grants {
            return;
        }
        let s = &mut self.secrets;
        let Some(store) = s.stores.get_mut(n) else {
            return;
        };
        let before = store.len();
        store.retain(|(job, _), g| g.expires_at > now && live.contains(job));
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

    /// A child takes a grant at use, as `GrantStore::get` lets it: only a
    /// grant still stored, not expired, for a job still running here.
    pub(super) fn consume_grants(&mut self, n: &NodeId, job: &str, names: &[String]) {
        if self.knows_job(n, job) != Knows::Live {
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
