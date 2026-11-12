//! The Limit Ring's data for a Claude Code session running in a lane, pure. The limit watcher's
//! windows are the main login's, so a lane's account is read from its own report
//! (`limits.usage`, what the Accounts window draws): the session's folder names the lane, the lane
//! names the account, and that account's report gives the bars.

use notchling_engine::limits::report::{BarKind, UsageReport};

use super::limit_ring::{Kind, Limit};
use crate::accounts::store::Snapshot;

/// A lane account's limits and plan, ready for the ring.
#[derive(Clone, Debug, PartialEq)]
pub struct LaneLimits {
    pub limits: Vec<Limit>,
    pub plan: Option<String>,
}

/// The account a lane runs: the lane's own account id, else the account that says it is live in
/// that lane. `None` for a folder no lane has (the main login is never matched here).
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

/// Every bar of a Claude Code report, the 5-hour one first as the report orders them.
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

/// The limits of the account the lane at `config_dir` runs. `None` until that account has a
/// report (or when it reports no limits), as for any session without data.
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
