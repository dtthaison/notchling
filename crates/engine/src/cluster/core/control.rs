use serde::Deserialize;
use serde_json::{json, Value};

use super::{Effect, Joining, LocalCmd, NodeCore, Target, Then};
use crate::cluster::id::NodeId;
use crate::cluster::jobs::{job_id, Job, JobSpec};
use crate::cluster::members::{can_be_master_default, Desired, Member, MemberState, Term};
use crate::cluster::state::Command;
use crate::cluster::versions::{compat, Compat, Purpose, Versions};
use crate::cluster::wire::{Msg, Origin, OriginTier, WireError};

#[derive(Deserialize)]
struct SetArgs {
    node: NodeId,
    #[serde(default)]
    can_be_master: Option<bool>,
    #[serde(default)]
    client: Option<bool>,
    #[serde(default)]
    reference: Option<bool>,
    #[serde(default)]
    accepts_delegation: Option<bool>,
    #[serde(default)]
    desired: Option<Desired>,
}

#[derive(Deserialize)]
struct RemoveArgs {
    node: NodeId,
}

#[derive(Deserialize)]
struct DispatchArgs {
    key: String,
    spec: JobSpec,
}

fn refusal(code: &str, message: impl Into<String>) -> WireError {
    WireError::new(code, message)
}

impl NodeCore {
    pub(super) fn on_local(&mut self, id: u64, cmd: LocalCmd, out: &mut Vec<Effect>) {
        let target = Target::Local(id);
        match cmd {
            LocalCmd::Create {
                id: cluster,
                name,
                mut me,
            } => {
                me.client = self.client;
                let result = if self.state.is_some() || self.joining.is_some() {
                    Err(refusal("exists", "this node is already in a cluster"))
                } else {
                    match self.apply(
                        Command::Create {
                            id: cluster.clone(),
                            name,
                            home: me,
                        },
                        out,
                    ) {
                        Ok(_) => Ok(json!({"cluster": cluster})),
                        Err(e) => Err(refusal("refused", e.to_string())),
                    }
                };
                self.finish(target, result, out);
            }
            LocalCmd::IssueToken { secret } => {
                let result = if self.is_home() {
                    self.tokens.issue(secret, self.wall_ms);
                    Ok(json!({}))
                } else {
                    Err(refusal("forbidden", "only the primary makes join tokens"))
                };
                self.finish(target, result, out);
            }
            LocalCmd::Approve {
                node,
                client,
                can_be_master,
            } => self.approve(target, node, client, can_be_master, out),
            LocalCmd::Deny { node } => self.deny(target, node, out),
            LocalCmd::Remove { node } => {
                if self.is_home() {
                    let result = self.remove(&node, out);
                    self.finish(target, result, out);
                } else {
                    self.relay_to_home(id, "cluster.remove", json!({"node": node}), out);
                }
            }
            LocalCmd::SetMember {
                node,
                can_be_master,
                client,
                reference,
                accepts_delegation,
                desired,
            } => {
                if self.is_home() {
                    let result = self.set_member(
                        SetArgs {
                            node,
                            can_be_master,
                            client,
                            reference,
                            accepts_delegation,
                            desired,
                        },
                        out,
                    );
                    self.finish(target, result, out);
                } else {
                    let args = json!({
                        "node": node,
                        "can_be_master": can_be_master,
                        "client": client,
                        "reference": reference,
                        "accepts_delegation": accepts_delegation,
                        "desired": desired,
                    });
                    self.relay_to_home(id, "cluster.member.set", args, out);
                }
            }
            LocalCmd::SetVersions { versions } => self.set_versions(id, versions, out),
            LocalCmd::Dispatch { key, spec, tier } => {
                if self.is_home() {
                    self.dispatch_here(target, key, spec, out);
                } else {
                    let origin = Origin {
                        node: self.me.clone(),
                        tier,
                    };
                    let args = json!({"key": key, "spec": spec});
                    self.relay_to_home_as(id, "cluster.dispatch", args, origin, out);
                }
            }
            LocalCmd::Forward {
                node,
                op,
                args,
                origin,
            } => self.forward(id, node, op, args, origin, out),
            LocalCmd::JoinWith { token, me } => {
                let result = if self.state.is_some() || self.joining.is_some() {
                    Err(refusal("exists", "this node is already in a cluster"))
                } else {
                    self.joining = Some(Joining {
                        token,
                        me,
                        local: id,
                        answered: false,
                    });
                    self.dial(out);
                    return;
                };
                self.finish(target, result, out);
            }
        }
    }

    fn relay_to_home(&mut self, local: u64, op: &str, args: Value, out: &mut Vec<Effect>) {
        let origin = Origin {
            node: self.me.clone(),
            tier: OriginTier::App,
        };
        self.relay_to_home_as(local, op, args, origin, out);
    }

    fn relay_to_home_as(
        &mut self,
        local: u64,
        op: &str,
        args: Value,
        origin: Origin,
        out: &mut Vec<Effect>,
    ) {
        let target = Target::Local(local);
        let Some(home) = self.state.as_ref().map(|s| s.home.clone()) else {
            self.finish(
                target,
                Err(refusal("no_cluster", "this node is in no cluster")),
                out,
            );
            return;
        };
        if !self.is_online(&home) {
            let name = self.member_name(&home);
            self.finish(
                target,
                Err(refusal("offline", format!("{name} is offline"))),
                out,
            );
            return;
        }
        self.call_out(
            &home,
            op.to_string(),
            args,
            origin,
            Then::Reply(target),
            out,
        );
    }

    fn set_versions(&mut self, local: u64, versions: Versions, out: &mut Vec<Effect>) {
        self.versions = versions.clone();
        let target = Target::Local(local);
        if self.is_home() {
            let me = self.me.clone();
            let _ = self.apply(Command::Versions { node: me, versions }, out);
            self.push_members(None, out);
            self.finish(target, Ok(json!({})), out);
            return;
        }
        let Some(home) = self.state.as_ref().map(|s| s.home.clone()) else {
            self.finish(target, Ok(json!({})), out);
            return;
        };
        if !self.is_online(&home) {
            self.finish(target, Ok(json!({})), out);
            return;
        }
        let origin = self.node_origin();
        let args = serde_json::to_value(&versions).unwrap_or(Value::Null);
        self.call_out(
            &home,
            "node.versions".to_string(),
            args,
            origin,
            Then::Reply(target),
            out,
        );
    }

    pub(super) fn native_versions(&mut self, target: Target, args: &Value, out: &mut Vec<Effect>) {
        let Target::Peer(from, _) = &target else {
            return;
        };
        let result = match serde_json::from_value::<Versions>(args.clone()) {
            Ok(versions) => {
                let from = from.clone();
                match self.apply(
                    Command::Versions {
                        node: from,
                        versions,
                    },
                    out,
                ) {
                    Ok(_) => {
                        self.push_members(None, out);
                        Ok(json!({}))
                    }
                    Err(e) => Err(refusal("refused", e.to_string())),
                }
            }
            Err(_) => Err(refusal("bad_args", "not a version report")),
        };
        self.finish(target, result, out);
    }

    pub(super) fn native_member_set(
        &mut self,
        target: Target,
        args: &Value,
        out: &mut Vec<Effect>,
    ) {
        let result = match serde_json::from_value::<SetArgs>(args.clone()) {
            Ok(a) => self.set_member(a, out),
            Err(_) => Err(refusal("bad_args", "not a member change")),
        };
        self.finish(target, result, out);
    }

    pub(super) fn native_remove(&mut self, target: Target, args: &Value, out: &mut Vec<Effect>) {
        let result = match serde_json::from_value::<RemoveArgs>(args.clone()) {
            Ok(a) => self.remove(&a.node, out),
            Err(_) => Err(refusal("bad_args", "not a removal")),
        };
        self.finish(target, result, out);
    }

    pub(super) fn native_dispatch(
        &mut self,
        target: Target,
        args: &Value,
        _origin: &Origin,
        out: &mut Vec<Effect>,
    ) {
        match serde_json::from_value::<DispatchArgs>(args.clone()) {
            Ok(a) => self.dispatch_here(target, a.key, a.spec, out),
            Err(_) => self.finish(target, Err(refusal("bad_args", "not a dispatch")), out),
        }
    }

    fn set_member(&mut self, a: SetArgs, out: &mut Vec<Effect>) -> Result<Value, WireError> {
        let command = Command::SetMember {
            node: a.node,
            can_be_master: a.can_be_master,
            client: a.client,
            reference: a.reference,
            accepts_delegation: a.accepts_delegation,
            desired: a.desired,
        };
        self.apply(command, out)
            .map_err(|e| refusal("refused", e.to_string()))?;
        self.push_members(None, out);
        Ok(json!({}))
    }

    fn remove(&mut self, node: &NodeId, out: &mut Vec<Effect>) -> Result<Value, WireError> {
        self.apply(Command::Remove { node: node.clone() }, out)
            .map_err(|e| refusal("refused", e.to_string()))?;
        if self.peers.get(node).is_some_and(|p| p.linked) {
            self.refuse(node, "removed", true, out);
        }
        self.dials.remove(node);
        self.push_members(None, out);
        Ok(json!({}))
    }

    fn approve(
        &mut self,
        target: Target,
        node: NodeId,
        client: bool,
        can_be_master: bool,
        out: &mut Vec<Effect>,
    ) {
        if !self.is_home() {
            self.finish(
                target,
                Err(refusal("forbidden", "only the primary approves")),
                out,
            );
            return;
        }
        let result = self
            .apply(
                Command::Approve {
                    node: node.clone(),
                    client,
                    can_be_master,
                },
                out,
            )
            .map_err(|e| refusal("refused", e.to_string()))
            .map(|_| json!({}));
        if result.is_ok() {
            if self.peers.get(&node).is_some_and(|p| p.linked) {
                self.send_join_ok(&node, out);
            }
            self.push_members(None, out);
        }
        self.finish(target, result, out);
    }

    fn deny(&mut self, target: Target, node: NodeId, out: &mut Vec<Effect>) {
        if !self.is_home() {
            self.finish(
                target,
                Err(refusal("forbidden", "only the primary denies")),
                out,
            );
            return;
        }
        let result = self
            .apply(Command::Deny { node: node.clone() }, out)
            .map_err(|e| refusal("refused", e.to_string()))
            .map(|_| json!({}));
        if result.is_ok() && self.peers.get(&node).is_some_and(|p| p.linked) {
            self.refuse(&node, "denied", true, out);
        }
        self.finish(target, result, out);
    }

    fn send_join_ok(&mut self, to: &NodeId, out: &mut Vec<Effect>) {
        let Some(state) = &self.state else {
            return;
        };
        let msg = Msg::JoinOk {
            cluster: state.id.clone(),
            name: state.name.clone(),
            term: state.term.clone(),
            home: state.home.clone(),
            vault: Some(state.vault.clone()),
            members: self.shared_members(),
        };
        self.send(to, msg, out);
    }

    fn shared_members(&self) -> Vec<Member> {
        self.state
            .as_ref()
            .map(|s| {
                s.members
                    .values()
                    .filter(|m| m.state != MemberState::Pending)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn push_members(&mut self, only: Option<&NodeId>, out: &mut Vec<Effect>) {
        if !self.is_home() {
            return;
        }
        let Some(state) = &self.state else {
            return;
        };
        let msg = Msg::Members {
            term: state.term.clone(),
            home: state.home.clone(),
            vault: Some(state.vault.clone()),
            members: self.shared_members(),
        };
        let targets: Vec<NodeId> = match only {
            Some(n) => vec![n.clone()],
            None => self
                .peers
                .iter()
                .filter(|(id, p)| {
                    p.linked
                        && p.greeted
                        && state
                            .members
                            .get(*id)
                            .is_some_and(|m| m.state == MemberState::Active)
                })
                .map(|(id, _)| id.clone())
                .collect(),
        };
        for to in targets {
            self.send(&to, msg.clone(), out);
        }
    }

    pub(super) fn on_members(
        &mut self,
        from: NodeId,
        term: Term,
        home: NodeId,
        vault: Option<NodeId>,
        members: Vec<Member>,
        out: &mut Vec<Effect>,
    ) {
        let Some(state) = &self.state else {
            return;
        };
        let known = state
            .members
            .get(&from)
            .is_some_and(|m| m.state == MemberState::Active);
        if !known || from != home {
            return;
        }
        if term < state.term {
            self.refuse(&from, "stale_term", false, out);
            return;
        }
        if term == state.term && state.home == self.me {
            return;
        }
        let (id, name, old_vault) = (state.id.clone(), state.name.clone(), state.vault.clone());
        let removed = members
            .iter()
            .any(|m| m.id == self.me && m.state == MemberState::Removed);
        if removed {
            self.wipe(out);
            return;
        }
        self.adopt(&term, out);
        let _ = self.apply(
            Command::Sync {
                id,
                name,
                term,
                home,
                vault: vault.unwrap_or(old_vault),
                members,
            },
            out,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn on_join(
        &mut self,
        from: NodeId,
        secret: String,
        name: String,
        fingerprint: String,
        addrs: Vec<String>,
        port: u16,
        versions: Versions,
        out: &mut Vec<Effect>,
    ) {
        if !self.is_home() {
            self.refuse(&from, "not the primary", true, out);
            return;
        }
        if NodeId::from_fingerprint(&fingerprint).as_ref() != Some(&from) {
            self.refuse(&from, "bad identity", true, out);
            return;
        }
        let item = format!("node-join:{from}");
        let existing = self
            .state
            .as_ref()
            .and_then(|s| s.members.get(&from))
            .map(|m| m.state);
        match existing {
            Some(MemberState::Pending) => {
                self.send(&from, Msg::JoinPending { item }, out);
                return;
            }
            Some(MemberState::Active) => {
                self.send_join_ok(&from, out);
                return;
            }
            _ => {}
        }
        if !self.tokens.take(&secret, self.wall_ms) {
            self.refuse(&from, "bad token", true, out);
            return;
        }
        if let Compat::Refuse(text) = compat(&self.versions, &versions, &name, Purpose::Link) {
            self.refuse(&from, &text, true, out);
            return;
        }
        let member = Member {
            id: from.clone(),
            name: name.clone(),
            fingerprint,
            addrs: addrs.clone(),
            port,
            client: false,
            can_be_master: false,
            reference: false,
            accepts_delegation: false,
            state: MemberState::Pending,
            versions: Some(versions.clone()),
            desired: Desired::default(),
            joined_ms: self.wall_ms,
        };
        if self
            .apply(
                Command::ProposeJoin {
                    member,
                    item: item.clone(),
                },
                out,
            )
            .is_err()
        {
            self.refuse(&from, "refused", true, out);
            return;
        }
        out.push(Effect::JoinRequest {
            item: item.clone(),
            node: from.clone(),
            name,
            os: versions.os.clone(),
            arch: versions.arch.clone(),
            addr: addrs.first().cloned(),
            can_be_master: can_be_master_default(&versions.os),
        });
        self.send(&from, Msg::JoinPending { item }, out);
    }

    pub(super) fn on_join_pending(&mut self, from: NodeId, item: String, out: &mut Vec<Effect>) {
        let Some(j) = &mut self.joining else {
            return;
        };
        if from != j.token.home || j.answered {
            return;
        }
        j.answered = true;
        let local = j.local;
        out.push(Effect::Answer {
            local,
            result: Ok(json!({"state": "pending", "item": item})),
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn on_join_ok(
        &mut self,
        from: NodeId,
        cluster: String,
        name: String,
        term: Term,
        home: NodeId,
        vault: Option<NodeId>,
        members: Vec<Member>,
        out: &mut Vec<Effect>,
    ) {
        let Some(j) = &self.joining else {
            return;
        };
        let mine_active = members
            .iter()
            .any(|m| m.id == self.me && m.state == MemberState::Active);
        if from != j.token.home || cluster != j.token.cluster || !mine_active {
            return;
        }
        let (local, answered) = (j.local, j.answered);
        self.joining = None;
        let vault = vault.unwrap_or_else(|| home.clone());
        let applied = self.apply(
            Command::Sync {
                id: cluster,
                name,
                term,
                home,
                vault,
                members,
            },
            out,
        );
        if !answered {
            let result = match applied {
                Ok(_) => Ok(json!({"state": "joined"})),
                Err(e) => Err(refusal("refused", e.to_string())),
            };
            out.push(Effect::Answer { local, result });
        }
        self.greet(&from);
        self.hello(&from, out);
    }

    fn reference_versions(&self) -> Versions {
        self.state
            .as_ref()
            .and_then(|s| {
                s.members
                    .values()
                    .find(|m| m.reference && m.state == MemberState::Active)
                    .and_then(|m| m.versions.clone())
            })
            .unwrap_or_else(|| self.versions.clone())
    }

    pub(super) fn dispatch_here(
        &mut self,
        target: Target,
        key: String,
        spec: JobSpec,
        out: &mut Vec<Effect>,
    ) {
        if !self.is_home() {
            self.finish(target, Err(refusal("forbidden", "not the primary")), out);
            return;
        }
        if let Some(job) = self.state.as_ref().and_then(|s| s.job_by_key(&key)) {
            let reply = job_reply(job, None);
            self.finish(target, Ok(reply), out);
            return;
        }
        let node = spec.node.clone();
        let name = self.member_name(&node);
        let member_state = self
            .state
            .as_ref()
            .and_then(|s| s.members.get(&node))
            .map(|m| (m.state, m.versions.clone()));
        let versions = match member_state {
            None => {
                self.finish(
                    target,
                    Err(refusal("unknown_node", format!("{name} is not a member"))),
                    out,
                );
                return;
            }
            Some((MemberState::Removed, _)) => {
                self.finish(
                    target,
                    Err(refusal("removed", format!("{name} was removed"))),
                    out,
                );
                return;
            }
            Some((MemberState::Pending, _)) => {
                self.finish(
                    target,
                    Err(refusal("pending", format!("{name} is not approved yet"))),
                    out,
                );
                return;
            }
            Some((MemberState::Active, versions)) => {
                if node == self.me {
                    Some(self.versions.clone())
                } else {
                    versions
                }
            }
        };
        if node != self.me && !self.is_online(&node) {
            self.finish(
                target,
                Err(refusal("offline", format!("{name} is offline"))),
                out,
            );
            return;
        }
        let Some(versions) = versions else {
            let text = format!("{name} hasn't reported its versions");
            self.finish(target, Err(refusal("refused", text)), out);
            return;
        };
        let warning = match compat(
            &self.reference_versions(),
            &versions,
            &name,
            Purpose::Dispatch(spec.tool),
        ) {
            Compat::Refuse(text) => {
                self.finish(target, Err(refusal("refused", text)), out);
                return;
            }
            Compat::Warn(text) => Some(text),
            Compat::Ok => None,
        };
        let job = job_id(&key);
        if let Err(e) = self.apply(
            Command::Dispatch {
                key,
                spec: spec.clone(),
            },
            out,
        ) {
            self.finish(target, Err(refusal("refused", e.to_string())), out);
            return;
        }
        let op = Self::tool_op(spec.tool);
        let args = start_args(&spec, &job);
        let then = Then::Dispatch {
            job,
            warning,
            target,
        };
        let origin = self.node_origin();
        if node == self.me {
            let call = self.next_local;
            self.next_local += 1;
            self.local_runs.insert(call, then);
            let term = self.current_term();
            out.push(Effect::RunLocal {
                call,
                op: op.to_string(),
                args,
                origin,
                term,
            });
        } else {
            self.call_out(&node, op.to_string(), args, origin, then, out);
        }
    }
}

fn job_reply(job: &Job, warning: Option<&str>) -> Value {
    json!({
        "job": job.id,
        "state": job.state,
        "session": job.session,
        "warning": warning.or(job.detail.as_deref()),
    })
}

fn start_args(spec: &JobSpec, job: &str) -> Value {
    let mut args = match &spec.args {
        Value::Object(map) => map.clone(),
        _ => serde_json::Map::new(),
    };
    let key = match spec.tool {
        crate::cluster::id::Tool::Codex => "draft_folder",
        _ => "cwd",
    };
    args.insert(key.to_string(), json!(spec.cwd));
    args.insert("job".to_string(), json!(job));
    Value::Object(args)
}
