use notchling_locale as t;

use crate::cluster::model::ClusterModel;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailoverRow {
    pub title: &'static str,
    pub line: &'static str,
}

pub fn row(model: &ClusterModel) -> Option<FailoverRow> {
    model.cluster_name()?;
    Some(FailoverRow {
        title: t::settings::cluster_failover(),
        line: t::settings::cluster_failover_unavailable(),
    })
}
