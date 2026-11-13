use notchling_engine::limits::report::{BarKind, UsageReport};

use super::limit_ring::{Kind, Limit};
use crate::accounts::store::Snapshot;

#[derive(Clone, Debug, PartialEq)]
pub struct LaneLimits {
    pub limits: Vec<Limit>,
    pub plan: Option<String>,
}

fn lane_account<'a>(s: &'a Snapshot, config_dir: &str) -> Option<(&'a str, Option<&'a str>)> {
    let lane = s.lanes.iter().find(|l| l.path == config_dir)?;
    let by_id = lane
        .account_id
        .as_deref()
        .filter(|id| !id.is_empty())
        .map(|id| (id, s.claude.accounts.iter().find(|a| a.id == id)));
    let (id, account) = by_id.or_else(|| {
        s.claude
            .accounts
            .iter()
            .find(|a| a.lane.as_deref() == Some(lane.name.as_str()))
            .map(|a| (a.id.as_str(), Some(a)))
    })?;
    Some((id, account.and_then(|a| a.plan.as_deref())))
}

fn report_limits(report: &UsageReport) -> Vec<Limit> {
    report
        .bars()
        .map(|b| Limit {
            name: b.label.clone(),
            kind: match b.kind {
                BarKind::Session => Kind::Session,
                BarKind::Week => Kind::Week,
                BarKind::Other => Kind::Other,
            },
            percent: b.percent,
            resets_at: b.resets_at,
        })
        .collect()
}

pub fn limits_for(s: &Snapshot, config_dir: &str) -> Option<LaneLimits> {
    let (id, plan) = lane_account(s, config_dir)?;
    let limits = report_limits(s.claude_usage.reports.get(id)?);
    (!limits.is_empty()).then(|| LaneLimits {
        limits,
        plan: plan.map(str::to_owned),
    })
}

#[cfg(test)]
#[path = "lane_limits_tests.rs"]
mod tests;
