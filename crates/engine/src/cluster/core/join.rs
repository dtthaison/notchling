use serde_json::json;

use super::{Effect, NodeCore, Target};
use crate::cluster::id::NodeId;
use crate::cluster::members::{can_be_master_default, Desired, Member, MemberState, Term};
use crate::cluster::state::Command;
use crate::cluster::versions::{compat, Compat, Purpose, Versions};
use crate::cluster::wire::{Msg, WireError};

fn refusal(code: &str, message: impl Into<String>) -> WireError {
    WireError::new(code, message)
}

impl NodeCore {
    pub(super) fn approve(
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

    pub(super) fn deny(&mut self, target: Target, node: NodeId, out: &mut Vec<Effect>) {
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
}
