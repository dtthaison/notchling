use serde_json::json;

use super::{Dial, Effect, NodeCore, DIAL_BACKOFF_MS, JOIN_ANSWER_MS, OFFLINE_MS, PING_MS};
use crate::cluster::id::NodeId;
use crate::cluster::members::{Member, MemberState, Term};
use crate::cluster::state::Command;
use crate::cluster::versions::{compat, Compat, Purpose, Versions};
use crate::cluster::wire::{Msg, WireError};

impl NodeCore {
    pub(super) fn on_linked(&mut self, peer: NodeId, out: &mut Vec<Effect>) {
        let boot = self.boot_ms;
        let p = self.peers.entry(peer.clone()).or_default();
        p.linked = true;
        p.greeted = false;
        p.online = false;
        p.last_heard = boot;
        p.last_hello = boot;
        p.last_ping = boot;
        self.dials.remove(&peer);
        self.hello(&peer, out);
    }

    pub(super) fn hello(&mut self, peer: &NodeId, out: &mut Vec<Effect>) {
        if let Some(j) = &self.joining {
            if *peer == j.token.home {
                let msg = Msg::Join {
                    secret: j.token.secret.clone(),
                    name: j.me.name.clone(),
                    fingerprint: j.me.fingerprint.clone(),
                    addrs: j.me.addrs.clone(),
                    port: j.me.port,
                    versions: self.versions.clone(),
                    headless: j.me.headless,
                };
                self.send(peer, msg, out);
                return;
            }
        }
        let active = self
            .state
            .as_ref()
            .and_then(|s| s.members.get(peer))
            .is_some_and(|m| m.state == MemberState::Active);
        if !active {
            return;
        }
        let msg = Msg::Hello {
            node: self.me.clone(),
            cluster: self.state.as_ref().map(|s| s.id.clone()),
            versions: self.versions.clone(),
            boot_id: self.boot_id.clone(),
            term: self.state.as_ref().map(|s| s.term.clone()),
        };
        self.send(peer, msg, out);
    }

    pub(super) fn on_unlinked(&mut self, peer: NodeId, out: &mut Vec<Effect>) {
        if let Some(p) = self.peers.get_mut(&peer) {
            let was_online = p.online;
            p.linked = false;
            p.greeted = false;
            p.online = false;
            if was_online {
                self.changed = true;
            }
        }
        self.fail_calls_to(&peer, "offline", out);
    }

    pub(super) fn drop_link(&mut self, peer: &NodeId, out: &mut Vec<Effect>) {
        let mut was_online = false;
        if let Some(p) = self.peers.get_mut(peer) {
            was_online = p.online;
            p.linked = false;
            p.greeted = false;
            p.online = false;
        }
        if was_online {
            self.changed = true;
        }
        out.push(Effect::Close { peer: peer.clone() });
        self.fail_calls_to(peer, "offline", out);
    }

    pub(super) fn greet(&mut self, peer: &NodeId) {
        let boot = self.boot_ms;
        let p = self.peers.entry(peer.clone()).or_default();
        let was = p.online && p.greeted;
        p.linked = true;
        p.greeted = true;
        p.online = true;
        p.last_heard = boot;
        if !was {
            self.changed = true;
        }
    }

    pub(super) fn refuse(&mut self, to: &NodeId, reason: &str, close: bool, out: &mut Vec<Effect>) {
        let (term, home) = match &self.state {
            Some(s) if reason == "stale_term" => (Some(s.term.clone()), Some(s.home.clone())),
            _ => (None, None),
        };
        self.send(
            to,
            Msg::Refused {
                reason: reason.to_string(),
                term,
                home,
            },
            out,
        );
        if close {
            self.drop_link(to, out);
        }
    }

    pub(super) fn on_hello(
        &mut self,
        from: NodeId,
        node: NodeId,
        cluster: Option<String>,
        versions: Versions,
        term: Option<Term>,
        out: &mut Vec<Effect>,
    ) {
        if node != from {
            self.drop_link(&from, out);
            return;
        }
        if self.state.is_none() && self.joining.is_some() {
            return;
        }
        let Some(state) = &self.state else {
            self.refuse(&from, "not a member", true, out);
            return;
        };
        if cluster.as_deref().is_some_and(|c| c != state.id) {
            self.refuse(&from, "other cluster", true, out);
            return;
        }
        match state.members.get(&from).map(|m| m.state) {
            Some(MemberState::Active) => {}
            Some(MemberState::Removed) => {
                self.refuse(&from, "removed", true, out);
                return;
            }
            _ => {
                self.refuse(&from, "not a member", true, out);
                return;
            }
        }
        if let Some(t) = &term {
            if *t < state.term {
                self.refuse(&from, "stale_term", true, out);
                return;
            }
        }
        let name = self.member_name(&from);
        if let Compat::Refuse(text) = compat(&self.versions, &versions, &name, Purpose::Link) {
            self.refuse(&from, &text, true, out);
            return;
        }
        if let Some(t) = &term {
            self.adopt(t, out);
        }
        self.greet(&from);
        let (home, term, cluster_id) = match &self.state {
            Some(s) => (s.home.clone(), s.term.clone(), s.id.clone()),
            None => return,
        };
        self.send(
            &from,
            Msg::HelloOk {
                node: self.me.clone(),
                cluster: cluster_id,
                term,
                home,
            },
            out,
        );
        if self.is_home() {
            let differs = self
                .state
                .as_ref()
                .and_then(|s| s.members.get(&from))
                .is_some_and(|m| m.versions.as_ref() != Some(&versions));
            if differs {
                let _ = self.apply(
                    Command::Versions {
                        node: from.clone(),
                        versions,
                    },
                    out,
                );
                self.push_members(None, out);
            } else {
                self.push_members(Some(&from), out);
            }
        }
    }

    pub(super) fn on_hello_ok(&mut self, from: NodeId, term: Term, out: &mut Vec<Effect>) {
        let known = self
            .state
            .as_ref()
            .and_then(|s| s.members.get(&from))
            .is_some_and(|m| m.state == MemberState::Active);
        if !known {
            return;
        }
        self.adopt(&term, out);
        self.greet(&from);
    }

    pub(super) fn on_refused(
        &mut self,
        from: NodeId,
        reason: String,
        term: Option<Term>,
        out: &mut Vec<Effect>,
    ) {
        if let Some(j) = &self.joining {
            if from == j.token.home {
                if !j.answered {
                    out.push(Effect::Answer {
                        local: j.local,
                        result: Err(WireError::new("refused", reason.clone())),
                    });
                }
                self.joining = None;
                self.changed = true;
                out.push(self.publish("cluster.refused", json!({"from": from, "reason": reason})));
                self.drop_link(&from, out);
                return;
            }
        }
        let from_home = self.state.as_ref().is_some_and(|s| s.home == from);
        if reason == "removed" && from_home {
            self.wipe(out);
            return;
        }
        if reason == "stale_term" {
            if let Some(t) = term {
                self.adopt(&t, out);
            }
        }
        out.push(self.publish("cluster.refused", json!({"from": from, "reason": reason})));
    }

    pub(super) fn wipe(&mut self, out: &mut Vec<Effect>) {
        let peers: Vec<NodeId> = self.peers.keys().cloned().collect();
        for peer in peers {
            out.push(Effect::Close { peer });
        }
        self.state = None;
        self.peers.clear();
        self.dials.clear();
        self.out_calls.clear();
        self.local_runs.clear();
        self.seen.clear();
        self.joining = None;
        self.changed = true;
        out.push(Effect::Wipe);
    }

    pub(super) fn adopt(&mut self, term: &Term, out: &mut Vec<Effect>) {
        let Some(state) = &self.state else {
            return;
        };
        if *term <= state.term {
            return;
        }
        let was_home = state.home == self.me;
        if self
            .apply(Command::Term { term: term.clone() }, out)
            .is_err()
        {
            return;
        }
        if was_home && term.node != self.me {
            out.push(Effect::SteppedDown {
                term: term.clone(),
                home: term.node.clone(),
            });
        }
    }

    pub(super) fn tick(&mut self, out: &mut Vec<Effect>) {
        let boot = self.boot_ms;
        let peers: Vec<NodeId> = self.peers.keys().cloned().collect();
        for id in peers {
            let Some(p) = self.peers.get(&id).cloned() else {
                continue;
            };
            if !p.linked {
                continue;
            }
            if boot.saturating_sub(p.last_heard) >= OFFLINE_MS {
                self.drop_link(&id, out);
                continue;
            }
            if p.greeted {
                if boot.saturating_sub(p.last_ping) >= PING_MS {
                    self.send(&id, Msg::Ping { boot_ms: boot }, out);
                    if let Some(q) = self.peers.get_mut(&id) {
                        q.last_ping = boot;
                    }
                }
            } else if boot.saturating_sub(p.last_hello) >= PING_MS {
                self.hello(&id, out);
                if let Some(q) = self.peers.get_mut(&id) {
                    q.last_hello = boot;
                }
            }
        }
        self.expire_calls(out);
        self.expire_join(out);
        self.prune_seen();
        self.dial(out);
    }

    fn expire_join(&mut self, out: &mut Vec<Effect>) {
        let Some(j) = &self.joining else {
            return;
        };
        if j.answered || self.boot_ms.saturating_sub(j.since) < JOIN_ANSWER_MS {
            return;
        }
        let (local, home) = (j.local, j.token.home.clone());
        self.joining = None;
        self.changed = true;
        out.push(Effect::Answer {
            local,
            result: Err(WireError::new(
                "unreachable",
                "the cluster's primary didn't answer",
            )),
        });
        self.drop_link(&home, out);
    }

    pub(super) fn dial(&mut self, out: &mut Vec<Effect>) {
        let boot = self.boot_ms;
        let mut wanted: Vec<(NodeId, Vec<String>, u16, String)> = Vec::new();
        if let Some(j) = &self.joining {
            wanted.push((
                j.token.home.clone(),
                j.token.addrs.clone(),
                j.token.port,
                j.token.fingerprint.clone(),
            ));
        } else if let Some(state) = &self.state {
            let Some(mine) = state
                .members
                .get(&self.me)
                .filter(|m| m.state == MemberState::Active)
            else {
                return;
            };
            for m in state.active().filter(|m| m.id != self.me) {
                let i_would = would_dial(mine, &m.id, &state.home);
                let they_would = would_dial(m, &self.me, &state.home);
                if i_would && (!they_would || self.me < m.id) {
                    wanted.push((m.id.clone(), m.addrs.clone(), m.port, m.fingerprint.clone()));
                }
            }
        }
        for (id, addrs, port, fingerprint) in wanted {
            if self.peers.get(&id).is_some_and(|p| p.linked) {
                continue;
            }
            let entry = self.dials.entry(id.clone()).or_insert(Dial {
                next_at: 0,
                backoff: DIAL_BACKOFF_MS.0,
            });
            if boot < entry.next_at {
                continue;
            }
            entry.next_at = boot + entry.backoff;
            entry.backoff = (entry.backoff * 2).min(DIAL_BACKOFF_MS.1);
            out.push(Effect::Dial {
                to: id,
                addrs,
                port,
                fingerprint,
            });
        }
    }

    pub(super) fn on_ping(&mut self, from: &NodeId, boot_ms: u64, out: &mut Vec<Effect>) {
        self.send(from, Msg::Pong { echo: boot_ms }, out);
    }

    pub(super) fn stale_error(&self) -> WireError {
        let body = match &self.state {
            Some(s) => json!({"term": s.term, "home": s.home}),
            None => json!({}),
        };
        WireError::new("stale_term", body.to_string())
    }
}

fn would_dial(a: &Member, b: &NodeId, home: &NodeId) -> bool {
    a.id != *b && (b == home || a.client || a.can_be_master)
}
