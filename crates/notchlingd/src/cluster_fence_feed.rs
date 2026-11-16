use crate::cluster_fence_keeper::Feed;
use crate::cluster_move::Mover;
use crate::cluster_node::ClusterNode;
use crate::snap_ready::LastSet;
use crate::snap_send::SnapSender;
use notchling_engine::cluster::fence_gates::SetView;
use notchling_engine::cluster::ready::assess;
use notchling_engine::cluster::snapshot::Movable;
use notchling_engine::cluster::NodeId;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};

pub struct RealFeed {
    node: Arc<OnceLock<Arc<ClusterNode>>>,
    sender: Arc<SnapSender>,
    last: Arc<LastSet>,
    mover: Arc<Mover>,
}

impl RealFeed {
    pub fn new(
        node: Arc<OnceLock<Arc<ClusterNode>>>,
        sender: Arc<SnapSender>,
        last: Arc<LastSet>,
        mover: Arc<Mover>,
    ) -> RealFeed {
        RealFeed {
            node,
            sender,
            last,
            mover,
        }
    }
}

impl Feed for RealFeed {
    fn me(&self) -> Option<NodeId> {
        self.node.get().map(|node| node.me().clone())
    }

    fn ready(&self) -> BTreeSet<NodeId> {
        let Some(node) = self.node.get() else {
            return BTreeSet::new();
        };
        assess(&node.status(), self.last.mb())
            .into_iter()
            .filter(|(_, assessment)| assessment.ready.is_ok())
            .map(|(id, _)| id)
            .collect()
    }

    fn latest_set(&self) -> Option<SetView> {
        let manifest = self.sender.latest_manifest()?;
        Some(SetView {
            set: manifest.set,
            movable: manifest
                .sessions
                .into_iter()
                .map(|entry| (entry.id, entry.movable == Movable::Yes))
                .collect(),
        })
    }

    fn acked(&self, node: &NodeId) -> Option<String> {
        self.sender.book().get(node).map(|acked| acked.set)
    }

    fn waiting(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    fn live(&self, id: &str) -> bool {
        self.mover.live(id)
    }
}
