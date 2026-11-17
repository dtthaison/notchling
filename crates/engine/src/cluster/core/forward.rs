use serde_json::{json, Value};

use super::link::clean_reason;
use super::record::Pushed;
use super::{
    Effect, NodeCore, OutCall, Seen, Target, Then, ATTACH_CALL_MS, CALL_MS, DEDUPE_MS,
    MOVE_CALL_MS, PEER_STALE_TERM, WORKSPACE_CALL_MS,
};
use crate::cluster::id::NodeId;
use crate::cluster::members::{MemberState, Term};
use crate::cluster::move_home::{resume_call_ms, MOVE_RESUME};
use crate::cluster::policy::{check, SenderFacts};
use crate::cluster::standby_store::SNAP_ACK;
use crate::cluster::state::role_of;
use crate::cluster::wire::{Msg, Origin, OriginTier, WireError};

pub fn call_wait(op: &str, args: &Value) -> u64 {
    if op == MOVE_RESUME {
        let count = args
            .get("sessions")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        return resume_call_ms(count);
    }
    if op.starts_with("move.") {
        return MOVE_CALL_MS;
    }
    if op.starts_with("workspace.") || op.starts_with("snap.") {
        WORKSPACE_CALL_MS
    } else if op == "grant.request" {
        ATTACH_CALL_MS
    } else {
        CALL_MS
    }
}

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
                headless,
            } => {
                let joiner = super::join::Joiner {
                    name,
                    fingerprint,
                    addrs,
                    port,
                    versions,
                    headless,
                };
                self.on_join(from, secret, joiner, out)
            }
            Msg::JoinPending { item } => self.on_join_pending(from, item, out),
            Msg::JoinOk {
                cluster,
                name,
                term,
                home,
                vault,
                members,
                failover,
            } => self.on_join_ok(
                from, cluster, name, term, home, vault, members, failover, out,
            ),
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
                        failover,
                        armed,
                        claims,
                        side_copies,
                    } => {
                        let pushed = Pushed {
                            failover,
                            armed,
                            claims,
                            side_copies,
                        };
                        self.on_members(from, (term, home, vault), members, pushed, out)
                    }
                    Msg::Call {
                        id,
                        op,
                        args,
                        origin,
                        term,
                    } => self.on_call(from, id, op, args.into_inner(), origin, term, out),
                    Msg::Reply { id, ok, err } => self.on_reply(from, id, ok, err, out),
                    Msg::Event {
                        topic,
                        name,
                        mut data,
                    } => {
                        if topic == "jobs" {
                            self.on_job_event(&from, &name, &data, out);
                            return;
                        }
                        if topic == "prompts" {
                            self.on_prompt_event(&from, topic, name, data, out);
                            return;
                        }
                        if topic == "outbox" {
                            let from = json!(from);
                            let mut fields = serde_json::Map::new();
                            for key in ["id", "session", "why"] {
                                if let Some(text) = data.get(key).filter(|v| v.is_string()) {
                                    fields.insert(key.to_string(), text.clone());
                                }
                            }
                            fields.insert("from".to_string(), from);
                            out.push(Effect::Publish {
                                topic,
                                name,
                                data: serde_json::Value::Object(fields),
                            });
                            return;
                        }
                        if topic != "sessions" {
                            return;
                        }
                        if let Some(fields) = data.as_object_mut() {
                            fields.insert("from".to_string(), json!(from));
                        }
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

    fn on_prompt_event(
        &mut self,
        from: &NodeId,
        topic: String,
        name: String,
        mut data: Value,
        out: &mut Vec<Effect>,
    ) {
        if name != "prompt.pending" && name != "prompt.resolved" {
            return;
        }
        let Some(state) = &self.state else {
            return;
        };
        let sender_active = state
            .members
            .get(from)
            .is_some_and(|m| m.state == MemberState::Active);
        let me_client = state.members.get(&self.me).is_some_and(|m| m.client);
        if !sender_active || !me_client {
            return;
        }
        let Some(fields) = data.as_object_mut() else {
            return;
        };
        fields.insert("from".to_string(), json!(from));
        out.push(Effect::Publish { topic, name, data });
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
        #[cfg(test)]
        let skip = self.skip_term_check
            && state.members.contains_key(&from)
            && role_of(state, &self.me) == crate::cluster::id::Role::Worker;
        #[cfg(not(test))]
        let skip = false;
        if !skip && term < my_term {
            let err = self.stale_error();
            out.push(self.publish(
                "cluster.refused",
                json!({"from": from, "op": clean_reason(&op), "reason": "stale_term"}),
            ));
            self.reply(&from, id, Err(err), out);
            return;
        }
        if !self.adopt(&term, &from, out) {
            self.refused_call(&from, id, &op, "bad_term", out);
            return;
        }
        let facts = self.state.as_ref().and_then(|state| {
            state.members.get(&from).map(|m| SenderFacts {
                id: m.id.clone(),
                role: role_of(state, &m.id),
                client: m.client,
                state: m.state,
                is_home: state.home == m.id,
                is_vault: state.vault == m.id,
            })
        });
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
        if rule.vault_only && self.state.as_ref().is_some_and(|s| s.vault != self.me) {
            let why = match &self.state {
                Some(s) => format!("{} holds the vault", self.member_name(&s.vault)),
                None => "not in a cluster".to_string(),
            };
            self.refused_call(&from, id, &op, &why, out);
            return;
        }
        if rule.home_only && !self.is_home() {
            let name = self.state.as_ref().map(|s| self.member_name(&s.home));
            let why = format!("{} is primary now", name.unwrap_or_default());
            self.refused_call(&from, id, &op, &why, out);
            return;
        }
        self.seen.insert(
            key,
            Seen {
                at: self.boot_ms,
                result: None,
            },
        );
        if op == SNAP_ACK {
            self.record_ack(&from, &args);
        }
        let target = Target::Peer(from, id);
        match op.as_str() {
            "cluster.dispatch" => self.native_dispatch(target, &args, &origin, out),
            "cluster.member.set" => self.native_member_set(target, &args, out),
            "cluster.remove" => self.native_remove(target, &args, out),
            "cluster.main.set" => self.native_main_set(target, &args, out),
            "cluster.failover.set" => self.native_failover_set(target, &args, out),
            "cluster.allow.set" => self.native_allow_set(target, &args, out),
            "cluster.record" => self.native_record(target, &args, &origin, out),
            "node.versions" => self.native_versions(target, &args, out),
            _ => {
                let args = match &target {
                    Target::Peer(minter, _) if op == "cluster.join.token" => {
                        json!({"minter": minter})
                    }
                    Target::Peer(minter, _) if op == "cluster.invite" => with_minter(args, minter),
                    _ => args,
                };
                let call = self.next_local;
                self.next_local += 1;
                self.local_runs.insert(call, Then::Reply(target));
                out.push(Effect::RunLocal {
                    call,
                    op,
                    args: args.into(),
                    origin,
                    term,
                });
            }
        }
    }

    fn refused_call(&mut self, from: &NodeId, id: u64, op: &str, why: &str, out: &mut Vec<Effect>) {
        out.push(self.publish(
            "cluster.refused",
            json!({"from": from, "op": clean_reason(op), "reason": why}),
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
                            self.adopt(&t, &from, out);
                        }
                    }
                }
                Err(WireError::new(
                    &clean_reason(&e.code),
                    clean_reason(&e.message),
                ))
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
                args: args.into(),
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
        let wait = call_wait(&op, &args);
        self.call_out_within(&node, op, args, origin, (Then::Reply(target), wait), out);
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
        self.call_out_within(to, op, args, origin, (then, CALL_MS), out);
    }

    pub(super) fn call_out_within(
        &mut self,
        to: &NodeId,
        op: String,
        args: Value,
        origin: Origin,
        (then, wait): (Then, u64),
        out: &mut Vec<Effect>,
    ) {
        let id = self.next_wire;
        self.next_wire += 1;
        let term = self.current_term();
        self.out_calls.insert(
            id,
            OutCall {
                to: to.clone(),
                deadline: self.boot_ms + wait,
                then,
            },
        );
        self.send(
            to,
            Msg::Call {
                id,
                op,
                args: args.into(),
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
                let result = self.dispatched(job, warning, result, out);
                self.finish(target, result, out);
            }
            Then::Attach {
                job,
                warning,
                target,
            } => self.attached(job, warning, target, result, out),
        }
    }

    pub(super) fn finish(
        &mut self,
        target: Target,
        result: Result<Value, WireError>,
        out: &mut Vec<Effect>,
    ) {
        match target {
            Target::Nobody => {}
            Target::Local(local) => out.push(Effect::Answer { local, result }),
            Target::Peer(from, id) => {
                let result = result.map_err(relayed);
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

fn relayed(e: WireError) -> WireError {
    if e.code == "stale_term" {
        return WireError::new(PEER_STALE_TERM, e.message);
    }
    e
}

fn with_minter(mut args: Value, minter: &NodeId) -> Value {
    match args.as_object_mut() {
        Some(map) => {
            map.insert("minter".to_string(), json!(minter));
            args
        }
        None => json!({"minter": minter}),
    }
}
