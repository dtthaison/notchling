use notchling_engine::machines::addresses::{AddressKind, MachineAddress};
use notchling_engine::machines::reach::Pick;
use notchling_engine::machines::registry::{Machine, MachineOs};
use notchling_engine::machines::step::{CheckResult, StepId};
use notchling_locale as t;

use super::model::ClusterModel;
use super::replication::ago;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reach {
    #[default]
    Unchecked,
    Up,
    Down,
    Unsure,
}

impl Reach {
    pub fn of(result: Option<&CheckResult>) -> Reach {
        match result {
            None => Reach::Unchecked,
            Some(r) if r.state.is_pass() => Reach::Up,
            Some(r) if r.state.is_fail() => Reach::Down,
            Some(_) => Reach::Unsure,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RouteLine {
    #[default]
    Hidden,
    Via(String),
    Wrong {
        wanted: String,
        active: String,
    },
    NoAnswer,
}

pub fn via_text(address: &MachineAddress) -> String {
    match address.kind {
        AddressKind::TailscaleName | AddressKind::TailscaleIp => match address.tailnet_name() {
            Some(network) => t::cluster::machine_via_tailscale_network(network),
            None => t::cluster::machine_via_tailscale().to_owned(),
        },
        AddressKind::Lan => t::cluster::machine_via_lan(&address.host),
        AddressKind::Public => t::cluster::machine_via_public(&address.host),
        AddressKind::Jump => {
            t::cluster::machine_via_jump(address.via.as_deref().unwrap_or(&address.host))
        }
    }
}

pub fn route_line(machine: &Machine, pick: Option<&Pick>) -> RouteLine {
    match pick {
        Some(Pick::Reached(choice)) => RouteLine::Via(via_text(&choice.address)),
        Some(Pick::WrongNetwork { wanted, active }) => RouteLine::Wrong {
            wanted: wanted.clone(),
            active: active.clone(),
        },
        Some(Pick::Unreachable) => RouteLine::NoAnswer,
        None => machine
            .address_list()
            .first()
            .map(|a| RouteLine::Via(via_text(a)))
            .unwrap_or_default(),
    }
}

pub fn routed(
    list: Vec<NotJoined>,
    machines: &[Machine],
    pick: &dyn Fn(&Machine) -> Option<Pick>,
) -> Vec<NotJoined> {
    list.into_iter()
        .map(|row| {
            let route = machines
                .iter()
                .find(|m| m.name == row.name)
                .map(|m| route_line(m, pick(m).as_ref()))
                .unwrap_or_default();
            NotJoined { route, ..row }
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotJoined {
    pub name: String,
    pub host: String,
    pub user: String,
    pub os: Option<MachineOs>,
    pub node_on: bool,
    pub reach: Reach,
    pub checked_s: Option<i64>,
    pub route: RouteLine,
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
            reach: Reach::Unchecked,
            checked_s: None,
            route: RouteLine::Hidden,
        })
        .collect()
}

pub fn annotated(
    list: Vec<NotJoined>,
    reach: &dyn Fn(&str) -> Option<CheckResult>,
    checked: &dyn Fn(&str) -> Option<i64>,
) -> Vec<NotJoined> {
    list.into_iter()
        .map(|m| NotJoined {
            reach: Reach::of(reach(&m.name).as_ref()),
            checked_s: checked(&m.name),
            ..m
        })
        .collect()
}

pub fn checked_line(machine: &NotJoined, now_s: i64) -> String {
    let reach = match machine.reach {
        Reach::Unchecked => return t::cluster::machine_reach_none().to_owned(),
        Reach::Up => t::cluster::machine_reach_up(),
        Reach::Down => t::cluster::machine_reach_down(),
        Reach::Unsure => t::cluster::machine_reach_unsure(),
    };
    match machine.checked_s {
        Some(at) => {
            let age = u64::try_from((now_s - at).max(0)).unwrap_or(0);
            t::cluster::machine_checked(reach, &ago(age))
        }
        None => reach.to_owned(),
    }
}

pub fn joined_result(
    results: &std::collections::BTreeMap<StepId, CheckResult>,
) -> Option<CheckResult> {
    results.get(&StepId::NODE_JOINED).cloned()
}
