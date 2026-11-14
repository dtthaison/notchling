use serde_json::{json, Value};

use super::{Effect, NodeCore, OutCall, Seen, Target, Then, CALL_MS, DEDUPE_MS};
use crate::cluster::id::{qualify, NodeId};
use crate::cluster::jobs::JobState;
use crate::cluster::members::Term;
use crate::cluster::policy::{check, SenderFacts};
use crate::cluster::state::{role_of, Command};
use crate::cluster::wire::{Msg, Origin, OriginTier, WireError};

impl NodeCore {
    pub(super) fn on_msg(&mut self, from: NodeId, msg: Msg, out: &mut Vec<Effect>) {
        let boot = self.boot_ms;
        if let Some(p) = self.peers.get_mut(&from) {
            p.last_heard = boot;
        }
        match msg {
            Msg::Hello {
                node,
                cluster,
                versions,
                term,
                ..
            } => self.on_hello(from, node, cluster, versions, term, out),
            Msg::HelloOk { term, .. } => self.on_hello_ok(from, term, out),
            Msg::Refused { reason, term, .. } => self.on_refused(from, reason, term, out),
            Msg::Join {
                secret,
                name,
                fingerprint,
                addrs,
                port,
                versions,
            } => self.on_join(from, secret, name, fingerprint, addrs, port, versions, out),
            Msg::JoinPending { item } => self.on_join_pending(from, item, out),
            Msg::JoinOk {
                cluster,
                name,
                term,
                home,
                vault,
                members,
            } => self.on_join_ok(from, cluster, name, term, home, vault, members, out),
            other => {
                if !self.peers.get(&from).is_some_and(|p| p.greeted) {
                    return;
                }
                match other {
                    Msg::Members {
                        term,
                        home,
                        vault,
                        members,
                    } => self.on_members(from, term, home, vault, members, out),
                    Msg::Call {
                        id,
                        op,
                        args,
                        origin,
                        term,
                    } => self.on_call(from, id, op, args, origin, term, out),
                    Msg::Reply { id, ok, err } => self.on_reply(from, id, ok, err, out),
                    Msg::Event { topic, name, data } => {
                        out.push(Effect::Publish { topic, name, data })
                    }
                    Msg::Ping { boot_ms } => self.on_ping(&from, boot_ms, out),
                    Msg::Pong { .. } => {}
                    Msg::Unknown => self.refuse(&from, "unknown", false, out),
                    _ => {}
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn on_call(
        &mut self,
        from: NodeId,
        id: u64,
        op: String,
        args: Value,
        origin: Origin,
        term: Term,
        out: &mut Vec<Effect>,
    ) {
        let key = (from.clone(), id);
        if let Some(seen) = self.seen.get(&key) {
            if let Some(result) = seen.result.clone() {
                self.reply(&from, id, result, out);
            }
            return;
        }
        let Some(state) = &self.state else {
            self.reply(
                &from,
                id,
                Err(WireError::new("forbidden", "not in a cluster")),
                out,
            );
            return;
        };
        let my_term = state.term.clone();
        let facts = state.members.get(&from).map(|m| SenderFacts {
            id: m.id.clone(),
            role: role_of(state, &m.id),
            client: m.client,
            state: m.state,
            is_home: state.home == m.id,
        });
        #[cfg(test)]
        let skip = self.skip_term_check
            && facts
                .as_ref()
                .is_some_and(|_| role_of(state, &self.me) == crate::cluster::id::Role::Worker);
        #[cfg(not(test))]
        let skip = false;
        if !skip && term < my_term {
            let err = self.stale_error();
            out.push(self.publish(
                "cluster.refused",
                json!({"from": from, "op": op, "reason": "stale_term"}),
            ));
            self.reply(&from, id, Err(err), out);
            return;
        }
        self.adopt(&term, out);
        let Some(facts) = facts else {
            self.refused_call(&from, id, &op, "not a member", out);
            return;
        };
        let rule = match check(&op, &facts, &origin) {
            Ok(rule) => rule,
            Err(denied) => {
                self.refused_call(&from, id, &op, &format!("{denied:?}"), out);
                return;
            }
        };
        if rule.home_only && !self.is_home() {
            self.refused_call(&from, id, &op, "not the primary", out);
            return;
        }
        self.seen.insert(
            key,
            Seen {
                at: self.boot_ms,
                result: None,
            },
        );
        let target = Target::Peer(from, id);
        match op.as_str() {
            "cluster.dispatch" => self.native_dispatch(target, &args, &origin, out),
            "cluster.member.set" => self.native_member_set(target, &args, out),
            "cluster.remove" => self.native_remove(target, &args, out),
            "node.versions" => self.native_versions(target, &args, out),
            _ => {
                let call = self.next_local;
                self.next_local += 1;
                self.local_runs.insert(call, Then::Reply(target));
                out.push(Effect::RunLocal {
                    call,
                    op,
                    args,
                    origin,
                    term,
                });
            }
        }
    }

    fn refused_call(&mut self, from: &NodeId, id: u64, op: &str, why: &str, out: &mut Vec<Effect>) {
        out.push(self.publish(
            "cluster.refused",
            json!({"from": from, "op": op, "reason": why}),
        ));
        self.reply(from, id, Err(WireError::new("forbidden", why)), out);
    }

    fn reply(&self, to: &NodeId, id: u64, result: Result<Value, WireError>, out: &mut Vec<Effect>) {
        let (ok, err) = match result {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(e)),
        };
        self.send(to, Msg::Reply { id, ok, err }, out);
    }

    fn on_reply(
        &mut self,
        from: NodeId,
        id: u64,
        ok: Option<Value>,
        err: Option<WireError>,
        out: &mut Vec<Effect>,
    ) {
        let matches = self.out_calls.get(&id).is_some_and(|c| c.to == from);
        if !matches {
            return;
        }
        let Some(call) = self.out_calls.remove(&id) else {
            return;
        };
        let result = match err {
            Some(e) => {
                if e.code == "stale_term" {
                    if let Ok(v) = serde_json::from_str::<Value>(&e.message) {
                        if let Ok(t) = serde_json::from_value::<Term>(v["term"].clone()) {
                            self.adopt(&t, out);
                        }
                    }
                }
                Err(e)
            }
            None => Ok(ok.unwrap_or(Value::Null)),
        };
        self.complete(call.then, result, out);
    }

    pub(super) fn forward(
        &mut self,
        local: u64,
        node: NodeId,
        op: String,
        args: Value,
        origin: Origin,
        out: &mut Vec<Effect>,
    ) {
        let target = Target::Local(local);
        if node == self.me {
            let call = self.next_local;
            self.next_local += 1;
            self.local_runs.insert(call, Then::Reply(target));
            let term = self.current_term();
            out.push(Effect::RunLocal {
                call,
                op,
                args,
                origin,
                term,
            });
            return;
        }
        if self.state.is_none() {
            self.finish(
                target,
                Err(WireError::new("no_cluster", "this node is in no cluster")),
                out,
            );
            return;
        }
        if !self.is_online(&node) {
            let name = self.member_name(&node);
            self.finish(
                target,
                Err(WireError::new("offline", format!("{name} is offline"))),
                out,
            );
            return;
        }
        self.call_out(&node, op, args, origin, Then::Reply(target), out);
    }

    pub(super) fn current_term(&self) -> Term {
        match &self.state {
            Some(s) => s.term.clone(),
            None => Term {
                n: 0,
                node: self.me.clone(),
            },
        }
    }

    pub(super) fn call_out(
        &mut self,
        to: &NodeId,
        op: String,
        args: Value,
        origin: Origin,
        then: Then,
        out: &mut Vec<Effect>,
    ) {
        let id = self.next_wire;
        self.next_wire += 1;
        let term = self.current_term();
        self.out_calls.insert(
            id,
            OutCall {
                to: to.clone(),
                deadline: self.boot_ms + CALL_MS,
                then,
            },
        );
        self.send(
            to,
            Msg::Call {
                id,
                op,
                args,
                origin,
                term,
            },
            out,
        );
    }

    pub(super) fn node_origin(&self) -> Origin {
        Origin {
            node: self.me.clone(),
            tier: OriginTier::Node,
        }
    }

    pub(super) fn fail_calls_to(&mut self, peer: &NodeId, code: &str, out: &mut Vec<Effect>) {
        let ids: Vec<u64> = self
            .out_calls
            .iter()
            .filter(|(_, c)| c.to == *peer)
            .map(|(id, _)| *id)
            .collect();
        let name = self.member_name(peer);
        for id in ids {
            if let Some(call) = self.out_calls.remove(&id) {
                let err = WireError::new(code, format!("{name} is offline"));
                self.complete(call.then, Err(err), out);
            }
        }
    }

    pub(super) fn expire_calls(&mut self, out: &mut Vec<Effect>) {
        let boot = self.boot_ms;
        let ids: Vec<u64> = self
            .out_calls
            .iter()
            .filter(|(_, c)| boot >= c.deadline)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Some(call) = self.out_calls.remove(&id) {
                let name = self.member_name(&call.to);
                let err = WireError::new("timeout", format!("{name} didn't answer"));
                self.complete(call.then, Err(err), out);
            }
        }
    }

    pub(super) fn prune_seen(&mut self) {
        let boot = self.boot_ms;
        self.seen
            .retain(|_, s| s.result.is_none() || s.at + DEDUPE_MS > boot);
    }

    pub(super) fn local_done(
        &mut self,
        call: u64,
        result: Result<Value, WireError>,
        out: &mut Vec<Effect>,
    ) {
        if let Some(then) = self.local_runs.remove(&call) {
            self.complete(then, result, out);
        }
    }

    pub(super) fn complete(
        &mut self,
        then: Then,
        result: Result<Value, WireError>,
        out: &mut Vec<Effect>,
    ) {
        match then {
            Then::Reply(target) => self.finish(target, result, out),
            Then::Dispatch {
                job,
                warning,
                target,
            } => {
                let node = self
                    .state
                    .as_ref()
                    .and_then(|s| s.jobs.get(&job))
                    .map(|j| j.spec.node.clone());
                match result {
                    Ok(v) => {
                        let session = v
                            .get("session")
                            .or_else(|| v.get("id"))
                            .and_then(|s| s.as_str())
                            .map(str::to_string);
                        let _ = self.apply(
                            Command::JobState {
                                id: job.clone(),
                                state: JobState::Running,
                                session: session.clone(),
                                detail: warning.clone(),
                            },
                            out,
                        );
                        let qualified = match (&node, &session) {
                            (Some(n), Some(s)) => Some(qualify(n, s)),
                            _ => None,
                        };
                        self.finish(
                            target,
                            Ok(json!({
                                "job": job,
                                "state": "running",
                                "session": session,
                                "qualified": qualified,
                                "warning": warning,
                            })),
                            out,
                        );
                    }
                    Err(e) => {
                        let _ = self.apply(
                            Command::JobState {
                                id: job,
                                state: JobState::Failed,
                                session: None,
                                detail: Some(e.message.clone()),
                            },
                            out,
                        );
                        self.finish(target, Err(e), out);
                    }
                }
            }
        }
    }

    pub(super) fn finish(
        &mut self,
        target: Target,
        result: Result<Value, WireError>,
        out: &mut Vec<Effect>,
    ) {
        match target {
            Target::Local(local) => out.push(Effect::Answer { local, result }),
            Target::Peer(from, id) => {
                let boot = self.boot_ms;
                self.seen.insert(
                    (from.clone(), id),
                    Seen {
                        at: boot,
                        result: Some(result.clone()),
                    },
                );
                if self.is_online(&from) {
                    self.reply(&from, id, result, out);
                }
            }
        }
    }
}
