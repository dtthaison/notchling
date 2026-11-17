use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use notchling_engine::cluster::build_id::build_id;
use notchling_engine::machines::registry::Machine;
use notchling_engine::machines::step::{ClusterTokens, StepId};
use notchling_locale as t;

use super::model::MachinesModel;
use crate::cluster::model::ClusterModel;
use crate::cluster::updates::{banner, rows_for};

pub struct UpdateAsk {
    tokens: Arc<dyn ClusterTokens>,
    machines: Vec<Machine>,
    nodes: BTreeMap<String, String>,
    failed_binary: BTreeSet<String>,
}

impl UpdateAsk {
    pub fn new(
        tokens: Arc<dyn ClusterTokens>,
        machines: Vec<Machine>,
        nodes: BTreeMap<String, String>,
        failed_binary: BTreeSet<String>,
    ) -> UpdateAsk {
        UpdateAsk {
            tokens,
            machines,
            nodes,
            failed_binary,
        }
    }

    pub fn run(&self, this_build: Option<&str>) -> Option<String> {
        let status = self.tokens.status_json()?;
        let me = status.get("me").and_then(|me| me.as_str())?.to_owned();
        let model = ClusterModel::from_status(&status, &me);
        let node_of = |name: &str| self.nodes.get(name).cloned();
        let rows = rows_for(
            this_build,
            &model,
            &self.machines,
            &node_of,
            None,
            &self.failed_binary,
        );
        let shown = banner(&rows)?;
        Some(format!(
            "{}. {}",
            shown.text,
            t::cluster::update_from_cluster()
        ))
    }

    pub fn run_here(&self) -> Option<String> {
        self.run(build_id())
    }
}

impl MachinesModel {
    pub fn update_ask(&self) -> UpdateAsk {
        let nodes = self
            .machines
            .iter()
            .filter_map(|m| {
                let node = self
                    .parts
                    .engine
                    .results(&m.name)
                    .get(&StepId::NODE_JOINED)
                    .and_then(|r| r.detected.node.clone())?;
                Some((m.name.clone(), node))
            })
            .collect();
        UpdateAsk::new(
            self.parts.cluster.clone(),
            self.machines.clone(),
            nodes,
            self.parts.engine.binary_failures(),
        )
    }

    pub fn set_update_banner(&mut self, text: Option<String>) {
        self.update_banner = text;
    }

    pub fn update_banner(&self) -> Option<&str> {
        self.update_banner.as_deref()
    }
}
