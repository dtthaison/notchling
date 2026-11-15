use notchling_locale::{self as t, Text};

use crate::cluster::model::ClusterModel;
use crate::cluster::wire::{self, Request};

pub const KEY: &str = "clusterFailover";

const CHOICES: [(&str, Text); 3] = [
    ("off", t::settings::CLUSTER_FAILOVER_OFF),
    ("30m", t::settings::CLUSTER_FAILOVER_V30M),
    ("1h", t::settings::CLUSTER_FAILOVER_V1H),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailoverRow {
    pub title: &'static str,
    pub options: Vec<(&'static str, &'static str)>,
    pub selected: String,
    pub lines: Vec<String>,
    pub not_ready: Option<&'static str>,
}

impl FailoverRow {
    pub fn change(&self, option: &str) -> Option<Request> {
        self.options
            .iter()
            .any(|(value, _)| *value == option)
            .then(|| wire::failover(option))
    }
}

pub fn row(model: &ClusterModel) -> Option<FailoverRow> {
    model.cluster_name()?;
    let failover = model.failover()?;
    let on = failover.value != "off";
    let rows = model.rows();
    let name_of = |id: &str| {
        rows.iter()
            .find(|r| r.id == id && r.active)
            .map(|r| r.name.clone())
    };
    let standby = failover.target.as_deref().and_then(name_of);
    let primary = rows.iter().find(|r| r.primary && r.active);
    let line = match (on, &standby, primary) {
        (false, _, _) => t::settings::cluster_failover_off_line().to_owned(),
        (true, Some(standby), Some(primary)) => {
            t::settings::cluster_failover_line(standby, &primary.name)
        }
        (true, _, _) => t::settings::cluster_failover_no_standby().to_owned(),
    };
    let not_ready =
        (on && standby.is_some() && !failover.armed).then(t::settings::cluster_failover_not_ready);
    Some(FailoverRow {
        title: t::settings::cluster_failover(),
        options: CHOICES
            .iter()
            .map(|(value, label)| (*value, label.get()))
            .collect(),
        selected: failover.value.clone(),
        lines: vec![line],
        not_ready,
    })
}
