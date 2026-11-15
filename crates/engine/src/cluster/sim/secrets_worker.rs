use serde_json::{json, Value};

use super::secrets::{Path, Stored, GRANT_LIFE_MS, JANITOR_MS};
use super::world::World;
use crate::cluster::id::NodeId;
use crate::cluster::jobs::JobState;
use crate::cluster::wire::{Origin, WireError};

fn refusal(why: impl Into<String>) -> WireError {
    WireError::new("forbidden", why)
}

impl World {
    pub(super) fn w_deliver(&mut self, node: &NodeId, call: u64, origin: &Origin, args: &Value) {
        let state = self.state_of(node);
        let sent_by_vault_home = state
            .as_ref()
            .is_some_and(|s| origin.node == s.vault && s.home == s.vault);
        let job = args.get("job").and_then(Value::as_str);
        let name = args.get("name").and_then(Value::as_str);
        let value = args.get("value").and_then(Value::as_str);
        let err = |code: &str, why: &str| Err(WireError::new(code, why));
        let result = match (sent_by_vault_home, job, name, value) {
            (false, ..) => err(
                "forbidden",
                "only the vault node, while it is the primary, delivers a secret",
            ),
            (true, Some(job), Some(name), Some(value)) => {
                let known = self.secrets.job_nodes.get(job) == Some(node);
                let ended = self
                    .secrets
                    .local_ended
                    .contains_key(&(node.clone(), job.to_string()));
                if (!known && !self.secrets.plant.misdeliver) || ended {
                    err("failed", "the job isn't running on this machine")
                } else {
                    let stored = Stored {
                        expires_at: self.now + GRANT_LIFE_MS,
                    };
                    let key = (job.to_string(), name.to_lowercase());
                    self.secrets
                        .stores
                        .entry(node.clone())
                        .or_default()
                        .insert(key, stored);
                    self.secrets.record(node, Path::Store { job }, value);
                    self.secrets.record(node, Path::ChildEnv { job }, value);
                    self.secrets.child_env += 1;
                    self.secrets.stats.stored += 1;
                    Ok(json!({"stored": name}))
                }
            }
            _ => err(
                "bad_request",
                "grant.deliver takes {job, name, value, expiresMs}",
            ),
        };
        self.finish_local(node, call, result);
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
        let gone: Vec<String> = self
            .core(n)
            .and_then(|c| c.state())
            .map(|s| {
                s.jobs
                    .values()
                    .filter(|j| !matches!(j.state, JobState::Running | JobState::Dispatching))
                    .map(|j| j.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let now = self.now;
        for job in gone {
            self.secrets.ended.entry(job).or_insert(now);
        }
    }

    pub(super) fn reachable(&self, n: &NodeId) -> bool {
        match self.core(n).and_then(|c| c.state()) {
            Some(s) => s.home == *n || self.net.linked(n, &s.home),
            None => false,
        }
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
        let s = &mut self.secrets;
        let Some(store) = s.stores.get_mut(n) else {
            return;
        };
        let before = store.len();
        store.retain(|(job, _), g| {
            let expired = g.expires_at <= now;
            let mine = s.local_ended.contains_key(&(n.clone(), job.clone()));
            let told = reachable && s.ended.contains_key(job);
            !(expired || mine || told)
        });
        s.stats.revoked += (before - store.len()) as u64;
    }

    pub(super) fn secrets_started(&mut self, n: &NodeId, args: &Value) {
        self.secrets.record(n, Path::Argv, &args.to_string());
        let Some(job) = args.get("job").and_then(Value::as_str) else {
            return;
        };
        let held = self
            .secrets
            .stores
            .get(n)
            .map(|s| s.keys().filter(|(j, _)| j == job).count())
            .unwrap_or(0);
        self.secrets.child_env += held as u64;
    }
}
