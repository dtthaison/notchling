use std::collections::BTreeMap;
use std::sync::Arc;

use notchling_engine::machines::registry::{Machine, NodeSettings};
use notchling_engine::machines::step::{ClusterTokens, StepId};

use super::model::MachinesModel;

pub const JOIN_LINE: &str = "Join it from the Machines sheet in Notchling";

pub struct ClusterAsk {
    tokens: Arc<dyn ClusterTokens>,
    asks: Vec<(String, Option<String>)>,
}

impl ClusterAsk {
    pub fn run(&self) -> BTreeMap<String, Option<bool>> {
        self.tokens.can_be_master_all(&self.asks)
    }
}

impl MachinesModel {
    pub fn node_on(&self, machine: &Machine) -> bool {
        machine.node.is_some()
    }

    pub fn set_node(&mut self, machine: &str, on: bool) -> bool {
        let saved = self.commit(|list| match list.iter_mut().find(|m| m.name == machine) {
            Some(m) => {
                m.node = on.then(NodeSettings::default);
                true
            }
            None => false,
        });
        saved.is_ok() && on
    }

    pub fn join_cluster(&mut self, machine: &str) -> bool {
        match self.machine(machine) {
            Some(m) if m.node.is_none() => self.set_node(machine, true),
            _ => false,
        }
    }

    pub fn request_join_plan(&mut self, machine: &str) {
        self.request_set_up_all(machine);
    }

    pub fn join_line(&self, machine: &Machine) -> Option<&'static str> {
        if !self.node_on(machine) {
            return None;
        }
        let joined = self
            .parts
            .engine
            .results(&machine.name)
            .get(&StepId::NODE_JOINED)
            .is_some_and(|r| r.state.is_pass());
        (!joined).then_some(JOIN_LINE)
    }

    pub fn cluster_ask(&self) -> ClusterAsk {
        self.cluster_ask_with(|name| {
            self.parts
                .engine
                .results(name)
                .get(&StepId::NODE_JOINED)
                .and_then(|r| r.detected.node.clone())
        })
    }

    pub(super) fn cluster_ask_with(&self, node_of: impl Fn(&str) -> Option<String>) -> ClusterAsk {
        let asks = self
            .machines
            .iter()
            .filter(|m| m.node.is_some())
            .map(|m| (m.name.clone(), node_of(&m.name)))
            .collect();
        ClusterAsk {
            tokens: self.parts.cluster.clone(),
            asks,
        }
    }

    pub fn set_masters(&mut self, answer: BTreeMap<String, Option<bool>>) {
        self.masters = answer;
    }

    pub fn can_be_master_line(&self, machine: &Machine) -> Option<String> {
        self.node_on(machine).then(|| {
            let word = match self.masters.get(&machine.name).copied().flatten() {
                Some(true) => "Yes",
                Some(false) => "No",
                None => "Not in the cluster yet",
            };
            format!("Can Be Master: {word}")
        })
    }
}
