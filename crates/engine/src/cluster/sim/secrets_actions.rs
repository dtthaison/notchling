use std::collections::BTreeMap;

use serde_json::json;

use super::secrets_broker::BrokerEv;
use super::world::{Event, World};
use crate::cluster::core::LocalCmd;
use crate::cluster::id::{qualify, NodeId, Qualified, Tool};
use crate::cluster::jobs::{Job, JobSpec, JobState};
use crate::cluster::wire::{Msg, Origin, OriginTier};

impl World {
    pub fn dispatch_with_secrets(
        &mut self,
        from: &NodeId,
        key: &str,
        node: &NodeId,
        names: &[&str],
    ) {
        let tier = if self.is_client(from) {
            OriginTier::App
        } else {
            OriginTier::Agent
        };
        let spec = JobSpec {
            node: node.clone(),
            tool: Tool::Shell,
            cwd: "/srv/repo".to_string(),
            args: json!({"command": "run"}),
            secrets: names.iter().map(|n| n.to_string()).collect(),
        };
        let cmd = LocalCmd::Dispatch {
            key: key.to_string(),
            spec,
            tier,
        };
        self.local(from, cmd);
    }

    pub(super) fn job_candidates(&self) -> Vec<(NodeId, Job)> {
        let mut seen: BTreeMap<String, (NodeId, Job)> = BTreeMap::new();
        for (id, state) in self.states() {
            for job in state.jobs.values() {
                let live = matches!(job.state, JobState::Running | JobState::Dispatching);
                if live && self.is_up(&job.spec.node) {
                    let mine = state.home == id;
                    if mine || !seen.contains_key(&job.id) {
                        seen.insert(job.id.clone(), (id.clone(), job.clone()));
                    }
                }
            }
        }
        seen.into_values().collect()
    }

    pub fn end_job(&mut self, job: &str) {
        let found = self.job_candidates().into_iter().find(|(_, j)| j.id == job);
        let Some((home_view, found)) = found else {
            return;
        };
        let node = found.spec.node.clone();
        let now = self.now;
        self.secrets
            .local_ended
            .entry((node.clone(), found.id.clone()))
            .or_insert(now);
        let data = json!({"job": found.id, "state": "done"});
        let home = self
            .state_of(&home_view)
            .map(|s| s.home)
            .unwrap_or(home_view);
        if home == node {
            let cmd = LocalCmd::JobEvent {
                name: "job.state".to_string(),
                data,
            };
            self.local(&node, cmd);
        } else {
            let msg = Msg::Event {
                topic: "jobs".to_string(),
                name: "job.state".to_string(),
                data,
            };
            self.send(&node, &home, msg);
        }
    }

    pub fn request_secret(&mut self, job: &str, name: &str) {
        let found = self.job_candidates().into_iter().find(|(_, j)| j.id == job);
        let (Some((_, found)), Some(vault)) = (found, self.secrets.vault_node.clone()) else {
            return;
        };
        let node = found.spec.node.clone();
        let args =
            json!({"job": found.id, "names": [name], "command": "cmd", "folder": "/srv/repo"});
        let cmd = LocalCmd::Forward {
            node: vault,
            op: "grant.request".to_string(),
            args: args.into(),
            origin: Origin {
                node: node.clone(),
                tier: OriginTier::Agent,
            },
        };
        self.local(&node, cmd);
    }

    pub(super) fn secrets_action(&mut self, kind: u8) {
        match kind {
            0 | 1 => {
                let jobs = self.job_candidates();
                if jobs.is_empty() {
                    return;
                }
                let i = self.rng.below(jobs.len() as u64) as usize;
                let id = jobs[i].1.id.clone();
                if kind == 0 {
                    self.end_job(&id);
                } else {
                    let names: Vec<String> = self.secrets.vault.keys().cloned().collect();
                    if names.is_empty() {
                        return;
                    }
                    let k = self.rng.below(names.len() as u64) as usize;
                    self.request_secret(&id, &names[k]);
                }
            }
            2 => {
                let macs: Vec<NodeId> = self
                    .node_ids()
                    .into_iter()
                    .filter(|n| self.is_client(n) && self.is_up(n))
                    .collect();
                let mut open: Vec<(NodeId, String)> = Vec::new();
                for mac in &macs {
                    for id in self.secrets.mirrored(mac) {
                        if id.contains("secret:") {
                            open.push((mac.clone(), id));
                        }
                    }
                }
                if open.is_empty() {
                    return;
                }
                let i = self.rng.below(open.len() as u64) as usize;
                let (mac, item) = open[i].clone();
                let allow = self.rng.chance(85);
                self.answer_item(&mac, &item, allow);
                if self.rng.chance(30) {
                    let owner = Qualified::parse(&item).map_or(mac.clone(), |q| q.node);
                    let plain = Qualified::parse(&item).map_or(item.clone(), |q| q.id);
                    let qualified = qualify(&owner, &plain);
                    for other in macs.iter().filter(|m| **m != mac) {
                        self.answer_item(other, &qualified, allow);
                    }
                }
            }
            _ => {
                let workers: Vec<NodeId> = self
                    .node_ids()
                    .into_iter()
                    .filter(|n| !self.is_client(n) && self.is_up(n))
                    .collect();
                if workers.is_empty() {
                    return;
                }
                let i = self.rng.below(workers.len() as u64) as usize;
                self.login_failure(&workers[i].clone());
            }
        }
    }

    pub fn inject_deliver(
        &mut self,
        node: &NodeId,
        job: &str,
        name: &str,
        (value, expires): (&str, i64),
        since: u64,
    ) {
        let Some(vault) = self.secrets.vault_node.clone() else {
            return;
        };
        self.inject_deliver_from(&vault, node, job, name, (value, expires), since);
    }

    pub fn inject_deliver_from(
        &mut self,
        from: &NodeId,
        node: &NodeId,
        job: &str,
        name: &str,
        (value, expires): (&str, i64),
        since: u64,
    ) {
        let args = json!({"job": job, "name": name, "value": value, "expiresMs": expires});
        let origin = Origin {
            node: from.clone(),
            tier: OriginTier::Node,
        };
        let ev = BrokerEv::Run {
            node: node.clone(),
            call: 0,
            op: "grant.deliver".to_string(),
            args,
            origin,
            arrived: (self.now, since),
        };
        let at = self.now + 1;
        self.schedule(at, Event::Broker(ev));
    }

    pub fn inject_revoke(&mut self, node: &NodeId, job: &str, names: Option<&[&str]>) {
        let Some(vault) = self.secrets.vault_node.clone() else {
            return;
        };
        let args = match names {
            Some(list) => json!({"job": job, "names": list}),
            None => json!({"job": job}),
        };
        let origin = Origin {
            node: vault,
            tier: OriginTier::Node,
        };
        let ev = BrokerEv::Run {
            node: node.clone(),
            call: 0,
            op: "grant.revoke".to_string(),
            args,
            origin,
            arrived: (self.now, self.secrets.revoke_seq),
        };
        let at = self.now + 1;
        self.schedule(at, Event::Broker(ev));
    }

    pub fn plant_grant(&mut self, node: &NodeId, job: &str, name: &str) {
        let stored = super::secrets::Stored {
            expires_at: self.now + 3_600_000,
            value: String::new(),
        };
        let key = (job.to_string(), name.to_string());
        self.secrets
            .stores
            .entry(node.clone())
            .or_default()
            .insert(key, stored);
    }

    pub fn set_release_term(&mut self, job: &str, name: &str, term: crate::cluster::members::Term) {
        let key = (job.to_string(), name.to_string());
        self.secrets.released_terms.insert(key, term);
    }

    pub fn worker_highest_term(&self, node: &NodeId) -> Option<crate::cluster::members::Term> {
        self.core(node).and_then(|c| c.highest_term().cloned())
    }
}
