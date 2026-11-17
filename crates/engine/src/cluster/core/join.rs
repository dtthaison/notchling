use serde_json::json;

use super::{shown_members, Effect, NodeCore, Target};
use crate::cluster::failover::Failover;
use crate::cluster::grants::SecretAllow;
use crate::cluster::id::NodeId;
use crate::cluster::invite::Invite;
use crate::cluster::members::{can_be_master_default, Desired, Member, MemberState, Term};
use crate::cluster::state::Command;
use crate::cluster::text::{plain, NAME_MAX};
use crate::cluster::versions::{compat, Compat, Purpose, Versions};
use crate::cluster::wire::{Msg, WireError};

pub(super) struct Joiner {
    pub name: String,
    pub fingerprint: String,
    pub addrs: Vec<String>,
    pub port: u16,
    pub versions: Versions,
    pub headless: bool,
}

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

    fn may_hold_primary_role(&self, id: &NodeId) -> bool {
        self.state.as_ref().is_some_and(|s| {
            s.members.get(id).is_some_and(|m| {
                m.state == MemberState::Active && (m.can_be_master || s.home == m.id)
            })
        })
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
            failover: Some(state.failover),
        };
        self.send(to, msg, out);
    }

    pub(super) fn on_join(
        &mut self,
        from: NodeId,
        secret: String,
        joiner: Joiner,
        out: &mut Vec<Effect>,
    ) {
        let Joiner {
            name,
            fingerprint,
            addrs,
            port,
            versions,
            headless,
        } = joiner;
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
        let Some((minter, invite)) = self.tokens.take_open(&secret, self.wall_ms) else {
            self.refuse(&from, "bad token", true, out);
            return;
        };
        if invite.as_ref().is_some_and(|i| i.node != from) {
            self.refuse(&from, "this token was made for another machine", true, out);
            return;
        }
        let name = match &invite {
            Some(invite) => invite.name.clone(),
            None => super::shown_name(&name, &from),
        };
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
            headless,
            state: MemberState::Pending,
            versions: Some(versions.clone()),
            desired: Desired::default(),
            joined_ms: self.wall_ms,
            secrets: SecretAllow::default(),
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
        if let Some(invite) = invite {
            self.admit_invited(&from, invite, minter, &versions, out);
            return;
        }
        let can_be_master =
            can_be_master_default(&versions.os) && self.may_hold_primary_role(&minter);
        out.push(Effect::JoinRequest {
            item: item.clone(),
            node: from.clone(),
            name,
            os: versions.os.clone(),
            arch: versions.arch.clone(),
            addr: addrs.first().cloned(),
            can_be_master,
            headless,
            minter_name: self.member_name(&minter),
            minter,
        });
        self.send(&from, Msg::JoinPending { item }, out);
    }

    fn admit_invited(
        &mut self,
        from: &NodeId,
        invite: Invite,
        minter: NodeId,
        versions: &Versions,
        out: &mut Vec<Effect>,
    ) {
        let approved = self.apply(
            Command::Approve {
                node: from.clone(),
                client: false,
                can_be_master: invite.can_be_master,
            },
            out,
        );
        if approved.is_err() {
            let _ = self.apply(Command::Deny { node: from.clone() }, out);
            self.refuse(from, "refused", true, out);
            return;
        }
        let _ = self.apply(
            Command::SetMember {
                node: from.clone(),
                can_be_master: None,
                client: None,
                reference: None,
                accepts_delegation: None,
                headless: None,
                desired: Some(Desired {
                    acting: invite.acting,
                    bridge: false,
                    install: false,
                }),
            },
            out,
        );
        self.send_join_ok(from, out);
        self.push_members(None, out);
        out.push(Effect::InviteJoined {
            node: from.clone(),
            name: invite.name,
            os: versions.os.clone(),
            arch: versions.arch.clone(),
            minter_name: self.member_name(&minter),
            minter,
        });
    }

    pub(super) fn on_join_pending(&mut self, from: NodeId, item: String, out: &mut Vec<Effect>) {
        let Some(j) = &mut self.joining else {
            return;
        };
        if from != j.token.home || j.answered {
            return;
        }
        j.answered = true;
        self.changed = true;
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
        failover: Option<Failover>,
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
                name: plain(&name, NAME_MAX),
                term,
                home,
                vault,
                members: shown_members(members),
            },
            out,
        );
        self.follow_failover(failover, false, out);
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
