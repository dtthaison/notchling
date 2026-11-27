use std::collections::BTreeSet;

use super::Rng;
use crate::cluster::id::NodeId;

#[derive(Clone, Debug, Default)]
pub struct SimNet {
    pub drop_pct: u8,
    pub dup_pct: u8,
    partitions: BTreeSet<(NodeId, NodeId)>,
    links: BTreeSet<(NodeId, NodeId)>,
}

fn pair(a: &NodeId, b: &NodeId) -> (NodeId, NodeId) {
    if a <= b {
        (a.clone(), b.clone())
    } else {
        (b.clone(), a.clone())
    }
}

impl SimNet {
    pub fn new(drop_pct: u8, dup_pct: u8) -> SimNet {
        SimNet {
            drop_pct,
            dup_pct,
            ..SimNet::default()
        }
    }

    pub fn delay(&self, rng: &mut Rng) -> u64 {
        1 + rng.below(50)
    }

    pub fn linked(&self, a: &NodeId, b: &NodeId) -> bool {
        self.links.contains(&pair(a, b))
    }

    pub fn connect(&mut self, a: &NodeId, b: &NodeId) {
        self.links.insert(pair(a, b));
    }

    pub fn disconnect(&mut self, a: &NodeId, b: &NodeId) {
        self.links.remove(&pair(a, b));
    }

    pub fn peers_of(&self, a: &NodeId) -> Vec<NodeId> {
        self.links
            .iter()
            .filter_map(|(x, y)| {
                if x == a {
                    Some(y.clone())
                } else if y == a {
                    Some(x.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn partition(&mut self, a: &NodeId, b: &NodeId) {
        self.partitions.insert(pair(a, b));
    }

    pub fn blocked(&self, a: &NodeId, b: &NodeId) -> bool {
        self.partitions.contains(&pair(a, b))
    }

    pub fn heal(&mut self) {
        self.partitions.clear();
    }
}
