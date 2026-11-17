use notchling_engine::machines::registry::{Machine, MachineOs};
use notchling_engine::machines::step::{CheckResult, StepId};

use super::model::ClusterModel;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotJoined {
    pub name: String,
    pub host: String,
    pub user: String,
    pub os: Option<MachineOs>,
    pub node_on: bool,
}

pub fn not_joined(
    machines: &[Machine],
    joined: &dyn Fn(&str) -> Option<CheckResult>,
    model: &ClusterModel,
) -> Vec<NotJoined> {
    let rows: Vec<_> = model.rows().into_iter().filter(|r| !r.removed).collect();
    machines
        .iter()
        .filter(|m| {
            let result = joined(&m.name);
            if result.as_ref().is_some_and(|r| r.state.is_pass()) {
                return false;
            }
            let node = result.and_then(|r| r.detected.node);
            let by_id = node
                .as_ref()
                .is_some_and(|id| rows.iter().any(|r| &r.id == id));
            !(by_id || rows.iter().any(|r| r.name == m.name))
        })
        .map(|m| NotJoined {
            name: m.name.clone(),
            host: m.host.clone(),
            user: m.user.clone(),
            os: m.os,
            node_on: m.node.is_some(),
        })
        .collect()
}

pub fn joined_result(
    results: &std::collections::BTreeMap<StepId, CheckResult>,
) -> Option<CheckResult> {
    results.get(&StepId::NODE_JOINED).cloned()
}
