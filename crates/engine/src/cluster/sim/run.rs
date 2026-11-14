use serde_json::Value;

use super::host::START_OPS;
use super::invariants::Violation;
use super::world::{Event, World};
use crate::cluster::core::{Effect, Input};
use crate::cluster::id::NodeId;
use crate::cluster::members::{MemberState, Term};
use crate::cluster::testkit::fp;
use crate::cluster::wire::Msg;

const TICK_MS: u64 = 1_000;

impl World {
    pub(super) fn handle(&mut self, event: Event) {
        match event {
            Event::Tick { node, gen } => {
                let live = self
                    .nodes
                    .get(&node)
                    .is_some_and(|n| n.alive && !n.suspended && n.tick_gen == gen);
                if !live {
                    return;
                }
                let (boot, wall) = match self.nodes.get(&node) {
                    Some(n) => (self.boot_of(n), self.wall_of(n)),
                    None => return,
                };
                self.step(
                    &node,
                    Input::Tick {
                        boot_ms: boot,
                        wall_ms: wall,
                    },
                );
                let at = self.now + TICK_MS;
                self.schedule(at, Event::Tick { node, gen });
            }
            Event::Deliver {
                to,
                from,
                msg,
                survive,
            } => {
                if survive || self.net.linked(&from, &to) {
                    let event = Event::Deliver {
                        to: to.clone(),
                        from,
                        msg,
                        survive,
                    };
                    self.hand_to(&to, event);
                }
            }
            Event::LinkUp { a, b } => {
                let ok = self.is_up(&a)
                    && self.is_up(&b)
                    && !self.net.blocked(&a, &b)
                    && !self.net.linked(&a, &b);
                if !ok {
                    return;
                }
                self.net.connect(&a, &b);
                self.step_linked(&a, &b);
                self.step_linked(&b, &a);
            }
            other => self.hand_event(other),
        }
    }

    fn hand_event(&mut self, event: Event) {
        let target = match &event {
            Event::LinkDown { to, .. } => to.clone(),
            Event::LocalDone { node, .. } => node.clone(),
            _ => return,
        };
        self.hand_to(&target, event);
    }

    fn step_linked(&mut self, n: &NodeId, peer: &NodeId) {
        if self.is_suspended(n) {
            return;
        }
        self.step(n, Input::Linked { peer: peer.clone() });
    }

    fn hand_to(&mut self, n: &NodeId, event: Event) {
        let Some(node) = self.nodes.get_mut(n) else {
            return;
        };
        if !node.alive {
            return;
        }
        if node.suspended {
            node.held.push(event);
            return;
        }
        match event {
            Event::Deliver { from, msg, .. } => self.step(n, Input::Msg { from, msg: *msg }),
            Event::LinkDown { peer, .. } => {
                if !self.net.linked(n, &peer) {
                    self.step(n, Input::Unlinked { peer });
                }
            }
            Event::LocalDone { call, result, .. } => {
                self.step(n, Input::LocalDone { call, result })
            }
            _ => {}
        }
    }

    pub(super) fn step(&mut self, n: &NodeId, input: Input) {
        let before = self.core(n).and_then(|c| c.highest_term().cloned());
        let effects = match self.nodes.get_mut(n).and_then(|x| x.core.as_mut()) {
            Some(core) => core.step(input),
            None => return,
        };
        self.after_step(n, before, effects);
    }

    pub(super) fn after_step(&mut self, n: &NodeId, before: Option<Term>, effects: Vec<Effect>) {
        let name = self.name_of(n);
        let mut persisted = false;
        let mut wiped = false;
        self.step_sends.clear();
        for effect in effects {
            self.trace
                .push(format!("{:>8} {:<4} {}", self.now, name, summary(&effect)));
            match effect {
                Effect::Send { to, msg } => self.send(n, &to, msg),
                Effect::Dial {
                    to, fingerprint, ..
                } => self.dial(n, &to, &fingerprint),
                Effect::Close { peer } => {
                    let flights: Vec<(u64, u64)> = self
                        .step_sends
                        .iter()
                        .filter(|(_, to)| *to == peer)
                        .map(|(key, _)| *key)
                        .collect();
                    for key in flights {
                        if let Some(Event::Deliver { survive, .. }) = self.queue.get_mut(&key) {
                            *survive = true;
                        }
                    }
                    if self.net.linked(n, &peer) {
                        self.net.disconnect(n, &peer);
                        let at = self.now + 1;
                        self.schedule(
                            at,
                            Event::LinkDown {
                                to: peer,
                                peer: n.clone(),
                            },
                        );
                    }
                }
                Effect::Pins {
                    fingerprints,
                    join_open,
                } => {
                    if let Some(node) = self.nodes.get_mut(n) {
                        node.pins = fingerprints.into_iter().collect();
                        node.join_open = join_open;
                    }
                }
                Effect::Persist { .. } => persisted = true,
                Effect::Wipe => wiped = true,
                Effect::Publish { .. } | Effect::SteppedDown { .. } => {}
                Effect::RunLocal {
                    call,
                    op,
                    args,
                    term,
                    ..
                } => self.run_local(n, call, &op, &args, &term, before.as_ref()),
                Effect::Answer { local, result } => self.answers.push((n.clone(), local, result)),
                Effect::JoinRequest { node, .. } => self.join_requests.push((n.clone(), node)),
            }
        }
        if let Some(node) = self.nodes.get_mut(n) {
            if wiped {
                node.journal = None;
            } else if persisted {
                node.journal = node.core.as_ref().and_then(|c| c.state().cloned());
            }
        }
        let after = self.core(n).and_then(|c| c.highest_term().cloned());
        if let (Some(b), Some(a)) = (&before, &after) {
            if a < b && self.violation.is_none() {
                self.violation = Some(Violation::TermWentDown { node: n.clone() });
            }
        }
        if let Some(a) = after {
            let slot = self.seen.entry(n.clone()).or_insert_with(|| a.clone());
            if a > *slot {
                *slot = a;
            }
        }
    }

    fn run_local(
        &mut self,
        n: &NodeId,
        call: u64,
        op: &str,
        args: &Value,
        term: &Term,
        before: Option<&Term>,
    ) {
        if let Some(b) = before {
            if term < b && self.violation.is_none() {
                self.violation = Some(Violation::StaleTermAccepted {
                    node: n.clone(),
                    call: op.to_string(),
                });
            }
        }
        let result = self.host.run(n, op, args, term, self.now);
        let at = self.now + 1 + self.rng.below(5);
        self.schedule(
            at,
            Event::LocalDone {
                node: n.clone(),
                call,
                result,
            },
        );
    }

    fn send(&mut self, from: &NodeId, to: &NodeId, msg: Msg) {
        if let Msg::Call { op, args, .. } = &msg {
            if START_OPS.contains(&op.as_str()) && self.violation.is_none() {
                let home_sees_active = self
                    .core(from)
                    .and_then(|c| c.state())
                    .and_then(|s| s.members.get(to))
                    .is_some_and(|m| m.state == MemberState::Active);
                if !home_sees_active {
                    let job = args
                        .get("job")
                        .and_then(|j| j.as_str())
                        .unwrap_or("?")
                        .to_string();
                    self.violation = Some(Violation::DispatchToRemoved {
                        job,
                        node: to.clone(),
                    });
                }
            }
        }
        if !self.net.linked(from, to) || self.net.blocked(from, to) {
            return;
        }
        if self.rng.chance(self.net.drop_pct) {
            return;
        }
        let at = self.now + self.net.delay(&mut self.rng);
        let copy = self.rng.chance(self.net.dup_pct);
        let event = Event::Deliver {
            to: to.clone(),
            from: from.clone(),
            msg: Box::new(msg),
            survive: false,
        };
        if copy {
            let again = self.now + self.net.delay(&mut self.rng);
            let key = self.schedule(again, event.clone());
            self.step_sends.push((key, to.clone()));
        }
        let key = self.schedule(at, event);
        self.step_sends.push((key, to.clone()));
    }

    fn dial(&mut self, from: &NodeId, to: &NodeId, fingerprint: &str) {
        let Some(target) = self.nodes.get(to) else {
            return;
        };
        let Some(me) = self.nodes.get(from) else {
            return;
        };
        let expected = fp(&target.name);
        if expected != fingerprint {
            return;
        }
        let reachable = target.alive
            && !target.suspended
            && !self.net.blocked(from, to)
            && !self.net.linked(from, to);
        let accepted = target.pins.contains(&fp(&me.name)) || target.join_open;
        if !reachable || !accepted {
            return;
        }
        let at = self.now + self.net.delay(&mut self.rng);
        self.schedule(
            at,
            Event::LinkUp {
                a: from.clone(),
                b: to.clone(),
            },
        );
    }
}

fn summary(effect: &Effect) -> String {
    match effect {
        Effect::Send { to, msg } => format!("send {} {}", &to.as_str()[..4], msg_name(msg)),
        Effect::Dial { to, .. } => format!("dial {}", &to.as_str()[..4]),
        Effect::Close { peer } => format!("close {}", &peer.as_str()[..4]),
        Effect::Pins {
            fingerprints,
            join_open,
        } => {
            format!("pins {} open={join_open}", fingerprints.len())
        }
        Effect::Persist { entry, .. } => format!("persist seq={}", entry.seq),
        Effect::Publish { name, .. } => format!("publish {name}"),
        Effect::RunLocal { op, .. } => format!("run {op}"),
        Effect::Answer { local, result } => format!("answer {local} ok={}", result.is_ok()),
        Effect::JoinRequest { node, .. } => format!("join_request {}", &node.as_str()[..4]),
        Effect::SteppedDown { term, .. } => format!("stepped_down {term}"),
        Effect::Wipe => "wipe".to_string(),
    }
}

fn msg_name(msg: &Msg) -> String {
    match msg {
        Msg::Call { op, term, .. } => format!("call {op} t{}", term.n),
        Msg::Reply { id, err, .. } => format!(
            "reply {id}{}",
            err.as_ref()
                .map(|e| format!(" err={}", e.code))
                .unwrap_or_default()
        ),
        Msg::Members { term, members, .. } => format!("members t{} n={}", term.n, members.len()),
        Msg::Hello { .. } => "hello".to_string(),
        Msg::HelloOk { .. } => "hello_ok".to_string(),
        Msg::Refused { reason, .. } => format!("refused {reason}"),
        Msg::Join { .. } => "join".to_string(),
        Msg::JoinPending { .. } => "join_pending".to_string(),
        Msg::JoinOk { .. } => "join_ok".to_string(),
        Msg::Event { name, .. } => format!("event {name}"),
        Msg::Ping { .. } => "ping".to_string(),
        Msg::Pong { .. } => "pong".to_string(),
        Msg::Unknown => "unknown".to_string(),
    }
}
