use notchling_engine::limits::report::{BarKind, UsageReport};

use super::limit_ring::{claude_limits, Kind, Limit};
use crate::accounts::store::Snapshot;

const ALL_MODELS: &str = "Week";

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
                BarKind::Week if b.label == ALL_MODELS => Kind::Week,
                BarKind::Week | BarKind::Other => Kind::Other,
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

pub fn active_limits(s: &Snapshot) -> Option<LaneLimits> {
    let id = s.claude.active.as_deref()?;
    let watched = s
        .limits
        .claude_for(Some(id))
        .map(|p| claude_limits(&p.windows))
        .filter(|l| !l.is_empty());
    let limits = watched.or_else(|| {
        s.claude_usage
            .reports
            .get(id)
            .map(report_limits)
            .filter(|l| !l.is_empty())
    })?;
    let plan = s
        .claude
        .accounts
        .iter()
        .find(|a| a.id == id)
        .and_then(|a| a.plan.clone());
    Some(LaneLimits { limits, plan })
}

#[cfg(test)]
#[path = "lane_limits_tests.rs"]
mod tests;
