use serde::Deserialize;
use serde_json::{json, Value};

use super::{Effect, Joining, LocalCmd, NodeCore, Target, Then};
use crate::cluster::id::NodeId;
use crate::cluster::members::{Desired, Member, MemberState, Term};
use crate::cluster::state::Command;
use crate::cluster::versions::Versions;
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
    headless: Option<bool>,
    #[serde(default)]
    desired: Option<Desired>,
}

#[derive(Deserialize)]
struct RemoveArgs {
    node: NodeId,
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
                headless,
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
                            headless,
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
                        "headless": headless,
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
                        since: self.boot_ms,
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

    fn set_member(&mut self, a: SetArgs, out: &mut Vec<Effect>) -> Result<Value, WireError> {
        let command = Command::SetMember {
            node: a.node,
            can_be_master: a.can_be_master,
            client: a.client,
            reference: a.reference,
            accepts_delegation: a.accepts_delegation,
            headless: a.headless,
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

    pub(super) fn shared_members(&self) -> Vec<Member> {
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
}
