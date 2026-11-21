use crate::cluster_inbox::{InboxCalls, Net, NetLink, Outbox, Peer, NODE_INBOX};
use crate::cluster_node::LocalCalls;
use crate::cluster_wiring::node_caller;
use crate::inbox_ops_tests::{item, rig, Calls, Rig};
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_engine::cluster::NodeId;
use notchling_engine::inbox::Item;
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(crate) fn node(n: u8) -> NodeId {
    NodeId::parse(&format!("{n:016x}")).expect("a node id")
}

struct NoInner;

impl LocalCalls for NoInner {
    fn call(&self, op: &str, _args: &Value, _origin: &Origin) -> Result<Value, ApiError> {
        Err(ApiError::new(ErrorCode::UnknownOp, op))
    }
}

pub(crate) struct Machine {
    pub id: NodeId,
    pub name: String,
    pub client: bool,
    pub keeps: bool,
    pub rig: Rig,
    pub calls: InboxCalls,
    pub outbox: Arc<Outbox>,
    pub raised: Arc<Calls>,
}

#[derive(Default)]
pub(crate) struct Fabric {
    pub machines: Mutex<BTreeMap<NodeId, Arc<Machine>>>,
    pub home: Mutex<Option<NodeId>>,
    pub down: Mutex<BTreeSet<NodeId>>,
    pub failing: Mutex<BTreeSet<NodeId>>,
    pub removed: Mutex<BTreeSet<NodeId>>,
    pub not_keeping: Mutex<BTreeSet<NodeId>>,
    pub knows_consent: Mutex<BTreeSet<NodeId>>,
    pub attempts: Mutex<Vec<NodeId>>,
    pub sent: Mutex<Vec<(NodeId, NodeId, String, Value)>>,
    pub clock: Arc<AtomicI64>,
}

pub(crate) struct Wire {
    pub me: NodeId,
    pub fabric: Arc<Fabric>,
}

impl Fabric {
    pub fn machine(&self, id: &NodeId) -> Arc<Machine> {
        self.machines.lock().unwrap()[id].clone()
    }

    pub fn pass(&self, ms: i64) {
        self.clock.fetch_add(ms, Ordering::SeqCst);
    }

    pub fn remove(&self, id: &NodeId) {
        self.removed.lock().unwrap().insert(id.clone());
    }

    pub fn stop_keeping(&self, id: &NodeId) {
        self.not_keeping.lock().unwrap().insert(id.clone());
    }

    pub fn resume_keeping(&self, id: &NodeId) {
        self.not_keeping.lock().unwrap().remove(id);
    }

    pub fn attempts_to(&self, id: &NodeId) -> usize {
        self.attempts
            .lock()
            .unwrap()
            .iter()
            .filter(|to| *to == id)
            .count()
    }

    pub fn set_failing(&self, id: &NodeId, failing: bool) {
        let mut set = self.failing.lock().unwrap();
        if failing {
            set.insert(id.clone());
        } else {
            set.remove(id);
        }
    }

    pub fn set_down(&self, id: &NodeId, down: bool) {
        let mut set = self.down.lock().unwrap();
        if down {
            set.insert(id.clone());
        } else {
            set.remove(id);
        }
    }

    pub fn sent_to(&self, to: &NodeId) -> Vec<(NodeId, String, Value)> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, target, _, _)| target == to)
            .map(|(from, _, op, args)| (from.clone(), op.clone(), args.clone()))
            .collect()
    }
}

impl Net for Wire {
    fn me(&self) -> Option<NodeId> {
        Some(self.me.clone())
    }

    fn home(&self) -> Option<NodeId> {
        self.fabric.home.lock().unwrap().clone()
    }

    fn peers(&self) -> Vec<Peer> {
        let home = self.home();
        let down = self.fabric.down.lock().unwrap().clone();
        let removed = self.fabric.removed.lock().unwrap().clone();
        let not_keeping = self.fabric.not_keeping.lock().unwrap().clone();
        let knows = self.fabric.knows_consent.lock().unwrap().clone();
        self.fabric
            .machines
            .lock()
            .unwrap()
            .values()
            .filter(|m| m.id != self.me && !removed.contains(&m.id))
            .map(|m| Peer {
                id: m.id.clone(),
                name: m.name.clone(),
                online: !down.contains(&m.id),
                keeps: (m.keeps && !not_keeping.contains(&m.id)) || Some(&m.id) == home.as_ref(),
                consent: knows.contains(&m.id),
            })
            .collect()
    }

    fn is_client(&self) -> bool {
        self.fabric.machine(&self.me).client
    }

    fn keeps(&self) -> bool {
        self.home().as_ref() == Some(&self.me)
            || (self.fabric.machine(&self.me).keeps
                && !self.fabric.not_keeping.lock().unwrap().contains(&self.me))
    }

    fn call(
        &self,
        to: &NodeId,
        op: &str,
        args: Value,
        tier: OriginTier,
        _within: Duration,
    ) -> Result<Value, ApiError> {
        let target = self.fabric.machine(to);
        if self.fabric.down.lock().unwrap().contains(to) {
            return Err(ApiError::new(
                ErrorCode::Failed,
                format!("{} is offline", target.name),
            ));
        }
        self.fabric.attempts.lock().unwrap().push(to.clone());
        if self.fabric.failing.lock().unwrap().contains(to) {
            return Err(ApiError::new(
                ErrorCode::Failed,
                "the cluster node didn't answer",
            ));
        }
        self.fabric.sent.lock().unwrap().push((
            self.me.clone(),
            to.clone(),
            op.to_string(),
            args.clone(),
        ));
        let origin = Origin {
            node: self.me.clone(),
            tier,
        };
        match op {
            NODE_INBOX => target.calls.call(op, &args, &origin),
            _ => {
                use crate::op_group::OpGroup;
                target.rig.ops.call(&node_caller(&origin), op, &args)
            }
        }
    }
}

pub(crate) struct Spec {
    pub n: u8,
    pub client: bool,
    pub keeps: bool,
}

pub(crate) fn cluster(specs: &[Spec], home: u8) -> Arc<Fabric> {
    let fabric = Arc::new(Fabric::default());
    *fabric.home.lock().unwrap() = Some(node(home));
    for spec in specs {
        let id = node(spec.n);
        let wire = Arc::new(Wire {
            me: id.clone(),
            fabric: fabric.clone(),
        });
        let rig = rig();
        let ticks = fabric.clock.clone();
        let outbox = Outbox::new(
            wire.clone(),
            rig.ops.clone(),
            Arc::new(move || ticks.load(Ordering::SeqCst)),
        );
        rig.ops.set_link(Arc::new(NetLink::new(wire.clone())));
        rig.ops.set_tap(outbox.tap());
        let calls = InboxCalls::new(Arc::new(NoInner), wire, rig.ops.clone());
        let machine = Machine {
            id: id.clone(),
            name: format!("m{}", spec.n),
            client: spec.client,
            keeps: spec.keeps,
            rig,
            calls,
            outbox,
            raised: Arc::new(Calls::default()),
        };
        fabric
            .machines
            .lock()
            .unwrap()
            .insert(id, Arc::new(machine));
    }
    fabric
}

impl Machine {
    pub fn raise(&self, id: &str) -> Item {
        let made = item(id);
        self.rig.ops.raise(made.clone(), Some(self.raised.clone()));
        made
    }

    pub fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.rig.ops.items().into_iter().map(|i| i.id).collect();
        ids.sort();
        ids
    }
}
