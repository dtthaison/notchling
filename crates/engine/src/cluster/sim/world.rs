use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::host::SimHost;
use super::invariants::Violation;
use super::net::SimNet;
use super::Rng;
use crate::cluster::core::{Input, LocalCmd, NodeCore};
use crate::cluster::id::NodeId;
use crate::cluster::join::JoinToken;
use crate::cluster::members::Term;
use crate::cluster::state::ClusterState;
use crate::cluster::testkit::{fp, id, member, versions};
use crate::cluster::wire::{Msg, WireError};

const TICK_MS: u64 = 1_000;

#[derive(Clone, Debug)]
pub(super) enum Event {
    Tick {
        node: NodeId,
        gen: u64,
    },
    Deliver {
        to: NodeId,
        from: NodeId,
        msg: Box<Msg>,
        survive: bool,
    },
    LinkUp {
        a: NodeId,
        b: NodeId,
    },
    LinkDown {
        to: NodeId,
        peer: NodeId,
    },
    LocalDone {
        node: NodeId,
        call: u64,
        result: Result<Value, WireError>,
    },
}

pub(super) struct SimNode {
    pub(super) name: String,
    pub(super) id: NodeId,
    pub(super) client: bool,
    pub(super) os: String,
    pub(super) core: Option<NodeCore>,
    pub(super) journal: Option<ClusterState>,
    pub(super) alive: bool,
    pub(super) suspended: bool,
    pub(super) held: Vec<Event>,
    pub(super) boot_origin: u64,
    pub(super) boot_n: u32,
    pub(super) drift: i64,
    pub(super) skew: i64,
    pub(super) tick_gen: u64,
    pub(super) pins: BTreeSet<String>,
    pub(super) join_open: bool,
}

pub struct World {
    pub(super) now: u64,
    pub(super) rng: Rng,
    pub(super) net: SimNet,
    pub(super) nodes: BTreeMap<NodeId, SimNode>,
    pub(super) queue: BTreeMap<(u64, u64), Event>,
    pub(super) seq: u64,
    pub(super) host: SimHost,
    pub(super) trace: Vec<String>,
    pub(super) next_local: u64,
    pub(super) answers: Vec<(NodeId, u64, Result<Value, WireError>)>,
    pub(super) join_requests: Vec<(NodeId, NodeId)>,
    pub(super) seen: BTreeMap<NodeId, Term>,
    pub(super) violation: Option<Violation>,
    pub(super) first_home: Option<NodeId>,
    pub(super) skip_term_check: bool,
    pub(super) step_sends: Vec<((u64, u64), NodeId)>,
}

impl World {
    pub fn new(seed: u64) -> World {
        World {
            now: 0,
            rng: Rng::new(seed),
            net: SimNet::new(0, 5),
            nodes: BTreeMap::new(),
            queue: BTreeMap::new(),
            seq: 0,
            host: SimHost::default(),
            trace: Vec::new(),
            next_local: 1,
            answers: Vec::new(),
            join_requests: Vec::new(),
            seen: BTreeMap::new(),
            violation: None,
            first_home: None,
            skip_term_check: false,
            step_sends: Vec::new(),
        }
    }

    pub fn plant_double_start(&mut self) {
        self.host.double_start = true;
    }

    pub fn plant_skip_term_check(&mut self) {
        self.skip_term_check = true;
        for node in self.nodes.values_mut() {
            if let Some(core) = node.core.as_mut() {
                core.skip_term_check = true;
            }
        }
    }

    pub fn now(&self) -> u64 {
        self.now
    }

    pub fn trace(&self) -> &[String] {
        &self.trace
    }

    pub fn violation(&self) -> Option<&Violation> {
        self.violation.as_ref()
    }

    pub fn host(&self) -> &SimHost {
        &self.host
    }

    pub fn rng(&mut self) -> &mut Rng {
        &mut self.rng
    }

    pub fn node_ids(&self) -> Vec<NodeId> {
        self.nodes.keys().cloned().collect()
    }

    pub fn name_of(&self, n: &NodeId) -> String {
        self.nodes
            .get(n)
            .map(|x| x.name.clone())
            .unwrap_or_else(|| n.to_string())
    }

    pub fn is_client(&self, n: &NodeId) -> bool {
        self.nodes.get(n).is_some_and(|x| x.client)
    }

    pub fn is_up(&self, n: &NodeId) -> bool {
        self.nodes.get(n).is_some_and(|x| x.alive && !x.suspended)
    }

    pub fn is_alive(&self, n: &NodeId) -> bool {
        self.nodes.get(n).is_some_and(|x| x.alive)
    }

    pub fn is_suspended(&self, n: &NodeId) -> bool {
        self.nodes.get(n).is_some_and(|x| x.suspended)
    }

    pub fn core(&self, n: &NodeId) -> Option<&NodeCore> {
        self.nodes.get(n).and_then(|x| x.core.as_ref())
    }

    pub fn states(&self) -> Vec<(NodeId, ClusterState)> {
        self.nodes
            .values()
            .filter_map(|n| {
                n.core
                    .as_ref()
                    .and_then(|c| c.state())
                    .map(|s| (n.id.clone(), s.clone()))
            })
            .collect()
    }

    pub fn highest_seen(&self, n: &NodeId) -> Option<&Term> {
        self.seen.get(n)
    }

    pub fn boot_ms(&self, n: &NodeId) -> u64 {
        match self.nodes.get(n) {
            Some(node) => self.boot_of(node),
            None => 0,
        }
    }

    pub fn boot_id(&self, n: &NodeId) -> String {
        self.nodes
            .get(n)
            .map(|x| format!("boot-{}-{}", x.name, x.boot_n))
            .unwrap_or_default()
    }

    pub fn answers(&self) -> &[(NodeId, u64, Result<Value, WireError>)] {
        &self.answers
    }

    pub(super) fn boot_of(&self, node: &SimNode) -> u64 {
        let elapsed = self.now.saturating_sub(node.boot_origin) as i64;
        let drifted = elapsed + (elapsed / 10_000) * node.drift;
        drifted.max(0) as u64
    }

    pub(super) fn wall_of(&self, node: &SimNode) -> i64 {
        self.now as i64 + node.skew
    }

    pub fn add_node(&mut self, name: &str, client: bool) -> NodeId {
        let os = if client { "macos" } else { "linux" };
        let node_id = id(name);
        let drift = self.rng.below(3) as i64 - 1;
        let skew = self.rng.below(10_000) as i64 - 5_000;
        let node = SimNode {
            name: name.to_string(),
            id: node_id.clone(),
            client,
            os: os.to_string(),
            core: None,
            journal: None,
            alive: true,
            suspended: false,
            held: Vec::new(),
            boot_origin: self.now,
            boot_n: 0,
            drift,
            skew,
            tick_gen: 0,
            pins: BTreeSet::new(),
            join_open: false,
        };
        self.nodes.insert(node_id.clone(), node);
        self.boot_core(&node_id);
        node_id
    }

    pub(super) fn boot_core(&mut self, n: &NodeId) {
        let Some(node) = self.nodes.get_mut(n) else {
            return;
        };
        let mut core = NodeCore::new(
            node.id.clone(),
            versions(&node.os),
            node.journal.clone(),
            node.client,
        )
        .with_boot_id(format!("boot-{}-{}", node.name, node.boot_n));
        core.skip_term_check = self.skip_term_check;
        node.core = Some(core);
        node.tick_gen += 1;
        let gen = node.tick_gen;
        let at = self.now + self.rng.below(TICK_MS);
        self.schedule(
            at,
            Event::Tick {
                node: n.clone(),
                gen,
            },
        );
    }

    pub(super) fn schedule(&mut self, at: u64, event: Event) -> (u64, u64) {
        self.seq += 1;
        self.queue.insert((at, self.seq), event);
        (at, self.seq)
    }

    pub fn create(&mut self, home: &NodeId) {
        let node = self.nodes.get(home);
        let (name, os, client) = match node {
            Some(n) => (n.name.clone(), n.os.clone(), n.client),
            None => return,
        };
        let mut me = member(&name, &os, client);
        me.reference = true;
        self.first_home = Some(home.clone());
        self.local(
            home,
            LocalCmd::Create {
                id: "sim-cluster".to_string(),
                name: "sim".to_string(),
                me,
            },
        );
    }

    pub fn join(&mut self, joiner: &NodeId, can_be_master: bool) {
        let Some(home) = self.first_home.clone() else {
            return;
        };
        let (name, os, client) = match self.nodes.get(joiner) {
            Some(n) => (n.name.clone(), n.os.clone(), n.client),
            None => return,
        };
        let home_name = self.name_of(&home);
        let secret = format!("secret-{name}");
        self.local(
            &home,
            LocalCmd::IssueToken {
                secret: secret.clone(),
            },
        );
        let token = JoinToken {
            cluster: "sim-cluster".to_string(),
            home: home.clone(),
            fingerprint: fp(&home_name),
            addrs: vec!["100.64.0.1".to_string()],
            port: 47823,
            secret,
        };
        let me = member(&name, &os, client);
        self.local(joiner, LocalCmd::JoinWith { token, me });
        self.run_for(600);
        self.local(
            &home,
            LocalCmd::Approve {
                node: joiner.clone(),
                client,
                can_be_master,
            },
        );
        self.run_for(1_500);
    }

    pub fn partition(&mut self, a: &NodeId, b: &NodeId) {
        self.net.partition(a, b);
        self.drop_link(a, b);
        self.trace_line(a, &format!("partition {}", self.name_of(b)));
    }

    pub fn heal(&mut self) {
        self.net.heal();
    }

    fn drop_link(&mut self, a: &NodeId, b: &NodeId) {
        if !self.net.linked(a, b) {
            return;
        }
        self.net.disconnect(a, b);
        let at = self.now + 1;
        self.schedule(
            at,
            Event::LinkDown {
                to: a.clone(),
                peer: b.clone(),
            },
        );
        self.schedule(
            at,
            Event::LinkDown {
                to: b.clone(),
                peer: a.clone(),
            },
        );
    }

    pub fn crash(&mut self, n: &NodeId) {
        let peers = self.net.peers_of(n);
        let Some(node) = self.nodes.get_mut(n) else {
            return;
        };
        node.alive = false;
        node.suspended = false;
        node.core = None;
        node.held.clear();
        node.tick_gen += 1;
        node.pins.clear();
        node.join_open = false;
        for p in peers {
            self.net.disconnect(n, &p);
            let at = self.now + 1;
            self.schedule(
                at,
                Event::LinkDown {
                    to: p,
                    peer: n.clone(),
                },
            );
        }
        self.trace_line(n, "crash");
    }

    pub fn restart(&mut self, n: &NodeId) {
        let Some(node) = self.nodes.get_mut(n) else {
            return;
        };
        if node.alive {
            return;
        }
        node.alive = true;
        node.boot_origin = self.now;
        node.boot_n += 1;
        self.boot_core(n);
        self.trace_line(n, "restart");
    }

    pub fn suspend(&mut self, n: &NodeId) {
        let Some(node) = self.nodes.get_mut(n) else {
            return;
        };
        if !node.alive || node.suspended {
            return;
        }
        node.suspended = true;
        node.tick_gen += 1;
        self.trace_line(n, "suspend");
    }

    pub fn resume(&mut self, n: &NodeId) {
        let Some(node) = self.nodes.get_mut(n) else {
            return;
        };
        if !node.suspended {
            return;
        }
        node.suspended = false;
        node.tick_gen += 1;
        let gen = node.tick_gen;
        let held = std::mem::take(&mut node.held);
        let now = self.now;
        for event in held {
            self.schedule(now, event);
        }
        self.schedule(
            now,
            Event::Tick {
                node: n.clone(),
                gen,
            },
        );
        self.trace_line(n, "resume");
    }

    pub fn local(&mut self, n: &NodeId, cmd: LocalCmd) -> u64 {
        let call = self.next_local;
        self.next_local += 1;
        if self.is_up(n) {
            self.step(n, Input::Local { id: call, cmd });
        }
        call
    }

    pub fn bump_term(&mut self, to: &NodeId) {
        if !self.is_up(to) {
            return;
        }
        let before = self.core(to).and_then(|c| c.highest_term().cloned());
        let effects = match self.nodes.get_mut(to).and_then(|n| n.core.as_mut()) {
            Some(core) => core.bump_term(),
            None => return,
        };
        self.after_step(to, before, effects);
        self.trace_line(to, "bump_term");
    }

    pub fn run_for(&mut self, ms: u64) {
        let end = self.now + ms;
        loop {
            let next = self.queue.keys().next().copied();
            match next {
                Some((at, seq)) if at <= end => {
                    if let Some(event) = self.queue.remove(&(at, seq)) {
                        self.now = at.max(self.now);
                        self.handle(event);
                    }
                }
                _ => break,
            }
        }
        self.now = end;
    }

    pub(super) fn trace_line(&mut self, n: &NodeId, text: &str) {
        let name = self.name_of(n);
        self.trace
            .push(format!("{:>8} {:<4} {}", self.now, name, text));
    }

    pub fn pending_join(&self, joiner: &NodeId) -> bool {
        self.join_requests.iter().any(|(_, n)| n == joiner)
    }
}
