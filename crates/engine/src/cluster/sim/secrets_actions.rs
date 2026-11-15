use std::collections::BTreeMap;

use serde_json::json;

use super::world::World;
use crate::cluster::core::LocalCmd;
use crate::cluster::id::{NodeId, Tool};
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

    pub fn answer_item(&mut self, from: &NodeId, item: &str, allow: bool) {
        let owner = self
            .secrets
            .mirrors
            .get(from)
            .and_then(|m| m.get(item))
            .map(|(o, _)| o.clone())
            .or_else(|| self.secrets.vault_node.clone());
        let Some(owner) = owner else {
            return;
        };
        let action = if allow { "allow_once" } else { "deny" };
        let cmd = LocalCmd::Forward {
            node: owner,
            op: "inbox.answer".to_string(),
            args: json!({"id": item, "action": action}).into(),
            origin: Origin {
                node: from.clone(),
                tier: OriginTier::App,
            },
        };
        self.local(from, cmd);
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
                    if let Some(list) = self.secrets.mirrors.get(mac) {
                        for (id, (_, is_open)) in list {
                            if *is_open && id.starts_with("secret:") {
                                open.push((mac.clone(), id.clone()));
                            }
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
                    for other in macs.iter().filter(|m| **m != mac) {
                        self.answer_item(other, &item, allow);
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
}
