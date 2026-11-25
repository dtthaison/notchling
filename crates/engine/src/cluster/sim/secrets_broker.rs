use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::secrets::SecretPlant;
use super::secrets::{Path, Release, SimSecrets, GRANT_LIFE_MS};
use super::world::{Event, World};
use crate::cluster::core::LocalCmd;
use crate::cluster::grants::{decide, encode_env, GrantAsk, GrantDecision};
use crate::cluster::id::NodeId;
use crate::cluster::jobs::{Job, JobState};
use crate::cluster::members::{Member, MemberState};
use crate::cluster::wire::{Origin, OriginTier, WireError};

pub(super) const BROKER_OPS: [&str; 7] = [
    "grant.attach",
    "grant.request",
    "grant.revoke_job",
    "grant.deliver",
    "grant.revoke",
    "node.inbox",
    "inbox.answer",
];

#[derive(Clone, Debug)]
pub(super) enum BrokerEv {
    Run {
        node: NodeId,
        call: u64,
        op: String,
        args: Value,
        origin: Origin,
        arrived: (u64, u64),
    },
    Answer {
        node: NodeId,
        local: u64,
        result: Result<Value, WireError>,
    },
}

#[derive(Clone, Debug)]
pub(super) struct Pending {
    pub(super) node: NodeId,
    pub(super) call: u64,
    pub(super) job: Job,
    pub(super) names: Vec<String>,
    pub(super) values: Vec<(String, String)>,
    pub(super) delivered: Vec<String>,
    pub(super) approval: Option<String>,
    pub(super) target: Option<NodeId>,
    pub(super) expires: i64,
}

fn refusal(why: impl Into<String>) -> WireError {
    WireError::new("forbidden", why)
}

fn canonical(names: &[String]) -> Option<Vec<String>> {
    let mut seen: Vec<String> = Vec::new();
    for name in names {
        let lower = name.to_lowercase();
        if !seen.contains(&lower) {
            seen.push(lower);
        }
    }
    (!seen.is_empty() && seen.len() <= 16).then_some(seen)
}

fn var_of(name: &str) -> String {
    name.to_uppercase().replace(['-', '.'], "_")
}

impl World {
    pub(super) fn note_stats(&mut self) {
        let line = format!("{:>8} ---- secrets {:?}", self.now, self.secrets.stats());
        self.trace.push(line);
    }

    pub fn secrets(&self) -> &SimSecrets {
        &self.secrets
    }

    pub fn plant_secrets(&mut self, plant: SecretPlant) {
        self.secrets.plant = plant;
    }

    pub fn seed_vault(&mut self, node: &NodeId, entries: &[(&str, bool)]) {
        self.secrets.vault_node = Some(node.clone());
        for (i, (name, asks)) in entries.iter().enumerate() {
            let marker = format!("{}{}", super::secrets::MARKER, 100 + i);
            self.secrets.vault.insert(name.to_string(), (marker, *asks));
        }
        let text: String = self
            .secrets
            .vault
            .values()
            .map(|(m, _)| m.clone())
            .collect();
        self.secrets.record(node, Path::Vault, &text);
    }

    pub(super) fn route_run(
        &mut self,
        n: &NodeId,
        call: u64,
        (op, args): (&str, &Value),
        origin: &Origin,
    ) -> bool {
        if !BROKER_OPS.contains(&op) {
            return false;
        }
        let at = self.now + 1;
        let ev = BrokerEv::Run {
            node: n.clone(),
            call,
            op: op.to_string(),
            args: args.clone(),
            origin: origin.clone(),
            arrived: (self.now, self.secrets.revoke_seq),
        };
        self.schedule(at, Event::Broker(ev));
        true
    }

    pub(super) fn broker_event(&mut self, ev: BrokerEv) {
        let node = match &ev {
            BrokerEv::Run { node, .. } | BrokerEv::Answer { node, .. } => node.clone(),
        };
        if !self.is_alive(&node) {
            return;
        }
        if self.is_suspended(&node) {
            let at = self.now + 1_000;
            self.schedule(at, Event::Broker(ev));
            return;
        }
        match ev {
            BrokerEv::Run {
                node,
                call,
                op,
                args,
                origin,
                arrived,
            } => match op.as_str() {
                "grant.attach" => self.b_attach(&node, call, &args),
                "grant.request" => self.b_request(&node, call, &origin, &args),
                "grant.revoke_job" => self.b_revoke_job(&node, call, &args),
                "grant.deliver" => self.w_deliver(&node, call, (&origin, &args), arrived),
                "grant.revoke" => self.w_revoke(&node, call, &origin, &args),
                "node.inbox" => self.mirror_in(&node, call, &origin, &args),
                _ => self.owner_answer(&node, call, &origin, &args),
            },
            BrokerEv::Answer {
                node,
                local,
                result,
            } => self.on_answer(&node, local, result),
        }
    }

    pub(super) fn secrets_crashed(&mut self, n: &NodeId) {
        let s = &mut self.secrets;
        s.pending.retain(|_, p| p.node != *n);
        s.waiting.retain(|(node, _), _| node != n);
        s.consuming.retain(|(node, _), _| node != n);
        s.started_on.retain(|(node, _)| node != n);
        s.local_ended.retain(|(node, _), _| node != n);
        s.holds.retain(|(node, _), _| node != n);
        s.hold_log.retain(|(node, _), _| node != n);
        s.live_seen.remove(n);
        s.due.insert(n.clone(), 0);
        self.mirrors_lost(n);
    }

    pub(super) fn finish_local(
        &mut self,
        node: &NodeId,
        call: u64,
        result: Result<Value, WireError>,
    ) {
        let at = self.now + 1 + self.rng.below(5);
        self.schedule(
            at,
            Event::LocalDone {
                node: node.clone(),
                call,
                result,
            },
        );
    }

    pub fn state_of_pub(&self, n: &NodeId) -> Option<crate::cluster::ClusterState> {
        self.state_of(n)
    }

    pub(super) fn state_of(&self, n: &NodeId) -> Option<crate::cluster::ClusterState> {
        self.core(n).and_then(|c| c.state()).cloned()
    }

    pub(super) fn b_attach(&mut self, node: &NodeId, call: u64, args: &Value) {
        let job = args
            .get("job")
            .and_then(|j| serde_json::from_value::<Job>(j.clone()).ok());
        let member = args
            .get("member")
            .and_then(|m| serde_json::from_value::<Member>(m.clone()).ok());
        let primary = args.get("vaultPrimary").and_then(Value::as_bool);
        let (Some(job), Some(member), Some(primary)) = (job, member, primary) else {
            let err = WireError::new("bad_request", "not an attach request");
            self.finish_local(node, call, Err(err));
            return;
        };
        let names = job.spec.secrets.clone();
        self.grant_start((node, call), (job, member), names, primary);
    }

    pub(super) fn b_request(&mut self, node: &NodeId, call: u64, origin: &Origin, args: &Value) {
        let state = self.state_of(node);
        let fail = |w: &mut World, err: WireError| w.finish_local(node, call, Err(err));
        let Some(state) = state else {
            return fail(self, WireError::new("failed", "this node is in no cluster"));
        };
        if state.vault != *node {
            return fail(self, refusal("this node doesn't hold the vault"));
        }
        let job_id = args.get("job").and_then(Value::as_str).unwrap_or("");
        let names: Option<Vec<String>> =
            args.get("names").and_then(Value::as_array).and_then(|l| {
                l.iter()
                    .map(|n| n.as_str().map(str::to_string))
                    .collect::<Option<_>>()
            });
        let (Some(job), Some(names)) = (state.jobs.get(job_id), names) else {
            return fail(self, WireError::new("failed", "no such job"));
        };
        if job.spec.node != origin.node {
            return fail(self, refusal("that job isn't on your machine"));
        }
        let Some(member) = state.members.get(&origin.node) else {
            return fail(self, refusal("not a member"));
        };
        let primary = state.home == state.vault;
        let who = (job.clone(), member.clone());
        self.grant_start((node, call), who, names, primary);
    }

    pub(super) fn grant_start(
        &mut self,
        (node, call): (&NodeId, u64),
        (job, member): (Job, Member),
        names: Vec<String>,
        vault_primary: bool,
    ) {
        let Some(names) = canonical(&names) else {
            let err = WireError::new("bad_request", "ask for 1 to 16 secrets");
            return self.finish_local(node, call, Err(err));
        };
        let off = self.secrets.plant.release_off_primary;
        let ask = |name: &str, looked: Option<(bool, bool)>| {
            decide(&GrantAsk {
                node: &member,
                job: &job,
                name,
                exists: looked.is_none_or(|l| l.0),
                asks: looked.is_some_and(|l| l.1),
                vault_is_primary: vault_primary || off,
                vault_name: "vault",
            })
        };
        let mut wait: Option<String> = None;
        for name in &names {
            match ask(name, None) {
                GrantDecision::Refuse(why) => {
                    return self.finish_local(node, call, Err(refusal(why)));
                }
                GrantDecision::Wait(why) => wait = wait.or(Some(why)),
                GrantDecision::Release | GrantDecision::Ask => {}
            }
        }
        if let Some(why) = wait {
            return self.finish_local(node, call, Ok(json!({"waiting": why})));
        }
        let mut asking = false;
        for name in &names {
            let looked = self
                .secrets
                .vault
                .get(name)
                .map(|(_, asks)| (true, *asks))
                .unwrap_or((false, false));
            match ask(name, Some(looked)) {
                GrantDecision::Refuse(why) => {
                    return self.finish_local(node, call, Err(refusal(why)));
                }
                GrantDecision::Wait(why) => {
                    return self.finish_local(node, call, Ok(json!({"waiting": why})));
                }
                GrantDecision::Ask => asking = true,
                GrantDecision::Release => {}
            }
        }
        let pid = self.secrets.next_id();
        let pending = Pending {
            node: node.clone(),
            call,
            job: job.clone(),
            names: names.clone(),
            values: Vec::new(),
            delivered: Vec::new(),
            approval: None,
            target: None,
            expires: 0,
        };
        self.secrets.pending.insert(pid, pending);
        if asking && !self.secrets.plant.skip_approval {
            let item = self.approval_item(node, &job, &names);
            let id = item.id.clone();
            self.secrets.approvals.insert(id.clone(), pid);
            self.secrets.last_approval = Some(id.clone());
            if let Some(p) = self.secrets.pending.get_mut(&pid) {
                p.approval = Some(id);
            }
            self.raise_item(node, item);
        } else {
            if asking {
                let stale = self.secrets.last_approval.clone();
                if let Some(p) = self.secrets.pending.get_mut(&pid) {
                    p.approval = stale;
                }
            }
            self.release(pid);
        }
    }

    pub(super) fn still(&self, p: &Pending) -> Result<(), String> {
        let state = self.state_of(&p.node).ok_or("no cluster")?;
        if state.vault != p.node || state.home != p.node {
            return Err("this machine isn't the primary vault node now".to_string());
        }
        let found = state.jobs.get(&p.job.id).ok_or("the job is gone")?;
        if !matches!(found.state, JobState::Running | JobState::Dispatching) {
            return Err("the job isn't running".to_string());
        }
        let member = state
            .members
            .get(&found.spec.node)
            .filter(|m| m.state == MemberState::Active)
            .ok_or("not an active member")?;
        match p.names.iter().find(|n| !member.secrets.allows(n)) {
            Some(_) => Err("a secret isn't allowed now".to_string()),
            None => Ok(()),
        }
    }

    pub(super) fn release(&mut self, pid: u64) {
        let Some(p) = self.secrets.pending.get(&pid).cloned() else {
            return;
        };
        let plant = self.secrets.plant;
        if !plant.release_off_primary {
            if let Err(why) = self.still(&p) {
                self.secrets.pending.remove(&pid);
                return self.finish_local(&p.node, p.call, Err(refusal(why)));
            }
        }
        let Some(state) = self.state_of(&p.node) else {
            return;
        };
        let target = state
            .jobs
            .get(&p.job.id)
            .map_or_else(|| p.job.spec.node.clone(), |j| j.spec.node.clone());
        let approval = p.approval.clone();
        let asked: Vec<(String, Option<String>)> = p
            .names
            .iter()
            .filter(|n| self.secrets.vault.get(*n).is_some_and(|(_, asks)| *asks))
            .map(|n| (n.clone(), approval.clone()))
            .collect();
        let newest = self.secrets_current_term(&state.term);
        if state.term < newest {
            self.secrets.stats.stale_releases += 1;
        }
        let rid = self.secrets.releases.len();
        self.secrets.releases.push(Release {
            node: p.node.clone(),
            term: state.term.clone(),
            newest,
            home: state.home.clone(),
            vault: state.vault.clone(),
            job: p.job.id.clone(),
            target: target.clone(),
            asked,
            at_ms: self.now,
        });
        let mut values = Vec::new();
        for name in &p.names {
            let token = self.secrets.new_mark(&target, Some(rid));
            let env = BTreeMap::from([(var_of(name), token.clone())]);
            if let Ok(value) = encode_env(&env) {
                values.push((name.clone(), value.expose().to_string()));
            }
            self.secrets.stats.released += 1;
            if let (Some(at), false) = (plant.leak, self.secrets.leaked_once) {
                self.secrets.leaked_once = true;
                self.plant_leak(&p.node, at, &token);
            }
        }
        self.secrets.record(
            &p.node,
            Path::Audit,
            &format!("released {}", p.names.join(",")),
        );
        let expires = (self.now + GRANT_LIFE_MS) as i64;
        if let Some(slot) = self.secrets.pending.get_mut(&pid) {
            slot.values = values;
            slot.target = Some(target);
            slot.expires = expires;
        }
        self.deliver_next(pid);
    }

    pub(super) fn deliver_next(&mut self, pid: u64) {
        let Some(p) = self.secrets.pending.get(&pid).cloned() else {
            return;
        };
        let i = p.delivered.len();
        if i >= p.values.len() {
            self.secrets.pending.remove(&pid);
            let reply = Ok(json!({"delivered": p.delivered}));
            return self.finish_local(&p.node, p.call, reply);
        }
        if !self.secrets.plant.release_off_primary {
            if let Err(why) = self.still(&p) {
                self.secrets.pending.remove(&pid);
                self.revoke_names(&p.node, &p.job, &p.delivered);
                return self.finish_local(&p.node, p.call, Err(refusal(why)));
            }
        }
        let (name, value) = p.values[i].clone();
        let mut target = p.target.clone().unwrap_or_else(|| p.job.spec.node.clone());
        if self.secrets.plant.misroute {
            target = self.other_member(&p.node, &target).unwrap_or(target);
        }
        let args = json!({
            "job": p.job.id,
            "name": name,
            "value": value,
            "expiresMs": p.expires,
        });
        let local = self.next_local;
        self.secrets.waiting.insert((p.node.clone(), local), pid);
        let cmd = LocalCmd::Forward {
            node: target,
            op: "grant.deliver".to_string(),
            args: args.into(),
            origin: Origin {
                node: p.node.clone(),
                tier: OriginTier::Node,
            },
        };
        self.local(&p.node, cmd);
    }

    pub(super) fn other_member(&self, me: &NodeId, not: &NodeId) -> Option<NodeId> {
        let state = self.state_of(me)?;
        state
            .members
            .values()
            .find(|m| m.state == MemberState::Active && m.id != *me && m.id != *not && !m.client)
            .map(|m| m.id.clone())
    }

    pub(super) fn on_answer(
        &mut self,
        node: &NodeId,
        local: u64,
        result: Result<Value, WireError>,
    ) {
        let Some(pid) = self.secrets.waiting.remove(&(node.clone(), local)) else {
            return;
        };
        let Some(p) = self.secrets.pending.get_mut(&pid) else {
            return;
        };
        let i = p.delivered.len();
        let name = p.values.get(i).map(|(n, _)| n.clone()).unwrap_or_default();
        match result {
            Ok(_) => {
                p.delivered.push(name);
                self.deliver_next(pid);
            }
            Err(e) => {
                let mut sent = p.delivered.clone();
                sent.push(name);
                let (job, node, call) = (p.job.clone(), p.node.clone(), p.call);
                self.secrets.pending.remove(&pid);
                self.revoke_names(&node, &job, &sent);
                self.finish_local(&node, call, Err(e));
            }
        }
    }

    pub(super) fn revoke_names(&mut self, node: &NodeId, job: &Job, names: &[String]) {
        if names.is_empty() || self.secrets.plant.keep_grants {
            return;
        }
        let args = json!({"job": job.id, "names": names});
        self.forward_node_op(node, &job.spec.node, "grant.revoke", args);
    }

    pub(super) fn forward_node_op(&mut self, from: &NodeId, to: &NodeId, op: &str, args: Value) {
        let cmd = LocalCmd::Forward {
            node: to.clone(),
            op: op.to_string(),
            args: args.into(),
            origin: Origin {
                node: from.clone(),
                tier: OriginTier::Node,
            },
        };
        self.local(from, cmd);
    }
}
