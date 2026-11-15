use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{json, Value};

use super::{
    shown_members, Effect, Joining, LocalCmd, NodeCore, Target, Then, CALL_MS, DELEGATE_CALL_MS,
};
use crate::cluster::failover::{armed_at_current, Failover};
use crate::cluster::id::NodeId;
use crate::cluster::main_sessions::{aliases, reconcile};
use crate::cluster::members::{Desired, Member, MemberState, Term};
use crate::cluster::state::{Command, CommandError};
use crate::cluster::versions::Versions;
use crate::cluster::wire::{Msg, Origin, OriginTier, WireError};
use crate::sessions::Agent;

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

#[derive(Deserialize)]
struct MainArgs {
    id: String,
    main: bool,
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
            LocalCmd::IssueToken { secret, minter } => {
                let result = if !self.is_home() {
                    Err(refusal("forbidden", "only the primary makes join tokens"))
                } else if !self.may_mint(&minter) {
                    Err(refusal("forbidden", "only a member makes join tokens"))
                } else {
                    self.tokens.issue(secret, minter, &self.me, self.wall_ms);
                    Ok(json!({}))
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
            LocalCmd::SetMain { session, main } => {
                if self.is_home() {
                    let result = self.set_main(session, main, out);
                    self.finish(target, result, out);
                } else {
                    let args = json!({"id": session, "main": main});
                    self.relay_to_home(id, "cluster.main.set", args, out);
                }
            }
            LocalCmd::MainSeen {
                agents,
                codex_exec,
                respawn,
            } => {
                self.main_seen(&agents, &codex_exec, &respawn, out);
                self.finish(target, Ok(json!({})), out);
            }
            LocalCmd::Snapshot { trigger } => self.snap_now(target, trigger, out),
            LocalCmd::TakePrimary { takeover } => self.take_primary(target, *takeover, out),
            LocalCmd::MoveSwitch { to, sessions, set } => {
                self.move_switch(id, (to, sessions, set), out)
            }
            LocalCmd::Fence { reason } => {
                self.set_fence(reason, out);
                self.finish(target, Ok(json!({})), out);
            }
            LocalCmd::SetVersions { versions } => self.set_versions(id, versions, out),
            LocalCmd::SetFailover { failover } => self.local_failover(id, failover, out),
            LocalCmd::Dispatch { key, spec, tier } => {
                if self.is_home() {
                    self.dispatch_here(target, key, spec, tier, out);
                } else {
                    let origin = Origin {
                        node: self.me.clone(),
                        tier,
                    };
                    let wait = if spec.is_delegate() {
                        DELEGATE_CALL_MS + CALL_MS
                    } else {
                        CALL_MS
                    };
                    let args = json!({"key": key, "spec": spec});
                    self.relay_to_home_within(id, "cluster.dispatch", args, origin, wait, out);
                }
            }
            LocalCmd::Forward {
                node,
                op,
                args,
                origin,
            } => self.forward(id, node, op, args, origin, out),
            LocalCmd::JobEvent { name, data } => {
                let me = self.me.clone();
                self.on_job_event(&me, &name, &data, out);
                self.finish(target, Ok(json!({})), out);
            }
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
                        since_ms: self.wall_ms,
                    });
                    self.changed = true;
                    self.dial(out);
                    return;
                };
                self.finish(target, result, out);
            }
        }
    }

    pub(super) fn relay_to_home(
        &mut self,
        local: u64,
        op: &str,
        args: Value,
        out: &mut Vec<Effect>,
    ) {
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
        self.relay_to_home_within(local, op, args, origin, CALL_MS, out);
    }

    fn relay_to_home_within(
        &mut self,
        local: u64,
        op: &str,
        args: Value,
        origin: Origin,
        wait: u64,
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
        self.call_out_within(
            &home,
            op.to_string(),
            args,
            origin,
            (Then::Reply(target), wait),
            out,
        );
    }

    fn set_versions(&mut self, local: u64, versions: Versions, out: &mut Vec<Effect>) {
        self.versions = versions.clone();
        let target = Target::Local(local);
        if self.is_home() {
            let me = self.me.clone();
            if self
                .apply(Command::Versions { node: me, versions }, out)
                .is_ok_and(|changes| !changes.is_empty())
            {
                self.push_members(None, out);
            }
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
                match self.record_versions(&from, versions, out) {
                    Ok(changed) => {
                        if changed {
                            self.push_members(None, out);
                        }
                        Ok(json!({}))
                    }
                    Err(e) => Err(refusal("refused", e.to_string())),
                }
            }
            Err(_) => Err(refusal("bad_args", "not a version report")),
        };
        self.finish(target, result, out);
    }

    pub(super) fn record_versions(
        &mut self,
        node: &NodeId,
        versions: Versions,
        out: &mut Vec<Effect>,
    ) -> Result<bool, CommandError> {
        let known = self.state.as_ref().and_then(|s| s.members.get(node));
        let Some(member) = known.filter(|m| m.state != MemberState::Removed) else {
            return self
                .apply(
                    Command::Versions {
                        node: node.clone(),
                        versions,
                    },
                    out,
                )
                .map(|changes| !changes.is_empty());
        };
        if member.versions.as_ref() == Some(&versions) {
            if let Some(gate) = self.versions_gate.get_mut(node) {
                gate.held = None;
            }
            return Ok(false);
        }
        let boot = self.boot_ms;
        let gap = self.versions_gap_ms;
        let gate = self.versions_gate.entry(node.clone()).or_default();
        if gate.last.is_some_and(|at| boot.saturating_sub(at) < gap) {
            gate.held = Some(versions);
            return Ok(false);
        }
        gate.held = None;
        let changes = self.apply(
            Command::Versions {
                node: node.clone(),
                versions,
            },
            out,
        )?;
        if changes.is_empty() {
            return Ok(false);
        }
        if let Some(gate) = self.versions_gate.get_mut(node) {
            gate.last = Some(boot);
        }
        Ok(true)
    }

    pub(super) fn release_versions(&mut self, out: &mut Vec<Effect>) {
        if !self.is_home() {
            self.versions_gate.clear();
            return;
        }
        let boot = self.boot_ms;
        let gap = self.versions_gap_ms;
        if let Some(state) = &self.state {
            self.versions_gate.retain(|id, _| {
                state
                    .members
                    .get(id)
                    .is_some_and(|m| m.state == MemberState::Active)
            });
        }
        let due: Vec<NodeId> = self
            .versions_gate
            .iter()
            .filter(|(_, g)| {
                g.held.is_some() && g.last.is_none_or(|at| boot.saturating_sub(at) >= gap)
            })
            .map(|(id, _)| id.clone())
            .collect();
        let mut changed = false;
        for node in due {
            let Some(versions) = self
                .versions_gate
                .get_mut(&node)
                .and_then(|g| g.held.take())
            else {
                continue;
            };
            changed |= self.record_versions(&node, versions, out).unwrap_or(false);
        }
        if changed {
            self.push_members(None, out);
        }
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

    pub(super) fn native_main_set(&mut self, target: Target, args: &Value, out: &mut Vec<Effect>) {
        let result = match serde_json::from_value::<MainArgs>(args.clone()) {
            Ok(a) => self.set_main(a.id, a.main, out),
            Err(_) => Err(refusal("bad_args", "not a main-session change")),
        };
        self.finish(target, result, out);
    }

    fn set_main(
        &mut self,
        session: String,
        main: bool,
        out: &mut Vec<Effect>,
    ) -> Result<Value, WireError> {
        self.fenced()?;
        let canonical = self.aliases.get(&session).unwrap_or(&session).clone();
        let mut ids = vec![canonical.clone()];
        if main {
            ids.push(session);
            ids.extend(
                self.aliases
                    .iter()
                    .filter(|(_, v)| **v == canonical)
                    .map(|(k, _)| k.clone()),
            );
        }
        ids.dedup();
        for id in ids {
            self.apply(Command::SetMain { session: id, main }, out)
                .map_err(|e| refusal("refused", e.to_string()))?;
        }
        Ok(json!({}))
    }

    fn main_seen(
        &mut self,
        agents: &[Agent],
        codex_exec: &BTreeSet<String>,
        respawn: &BTreeMap<String, Vec<String>>,
        out: &mut Vec<Effect>,
    ) {
        if !self.is_home() || self.fence.is_some() {
            return;
        }
        self.aliases = aliases(agents);
        let Some(state) = &self.state else {
            return;
        };
        let (mut main, ended, kept) = reconcile(
            &state.main,
            &state.ended,
            agents,
            (&state.kept, codex_exec),
            self.wall_ms,
        );
        for (id, session) in &mut main {
            if let Some(flags) = respawn.get(id) {
                session.respawn = flags.clone();
            }
        }
        let _ = self.apply(Command::Main { main, ended, kept }, out);
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

    fn may_mint(&self, minter: &NodeId) -> bool {
        *minter == self.me
            || self
                .state
                .as_ref()
                .and_then(|s| s.members.get(minter))
                .is_some_and(|m| m.state == MemberState::Active)
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
            failover: Some(state.failover),
            armed: armed_at_current(state),
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

    #[allow(clippy::too_many_arguments)]
    pub(super) fn on_members(
        &mut self,
        from: NodeId,
        term: Term,
        home: NodeId,
        vault: Option<NodeId>,
        members: Vec<Member>,
        failover: Option<Failover>,
        armed: bool,
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
        if home != term.node || !self.adopt(&term, &from, out) {
            self.refuse(&from, "bad_term", false, out);
            return;
        }
        if self.state.as_ref().is_none_or(|s| s.home != from) {
            self.refuse(&from, "bad_term", false, out);
            return;
        }
        let removed = members
            .iter()
            .any(|m| m.id == self.me && m.state == MemberState::Removed);
        if removed {
            self.wipe(out);
            return;
        }
        let _ = self.apply(
            Command::Sync {
                id,
                name,
                term,
                home,
                vault: vault.unwrap_or(old_vault),
                members: shown_members(members),
            },
            out,
        );
        self.follow_failover(failover, armed, out);
    }
}
