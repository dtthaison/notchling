use super::report::UsageReport;
use super::usage::AccountUsage;
use crate::transcript::errors::{limit_hit_in_file, LimitHit};
use crate::transcript::locate::successor;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountInfo {
    pub id: String,
    pub email: String,
    pub needs_login: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneInfo {
    pub name: String,
    pub path: String,
    pub account_id: String,
    pub email: String,
}

pub trait ClaudeSource: Send + Sync {
    fn sync_usage(&self);
    fn sync_usage_now(&self) {
        self.sync_usage();
    }
    fn accounts(&self) -> Vec<AccountInfo>;
    fn active_id(&self) -> Option<String>;
    fn usage(&self, id: &str) -> Option<AccountUsage>;
    fn usage_error(&self, _id: &str) -> Option<String> {
        None
    }
    fn lanes(&self) -> Vec<LaneInfo>;
    fn switch_to(&self, id: &str) -> Result<(), String>;
    fn switch_lane(&self, lane: &str, to: &str) -> Result<(), String>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderAccountInfo {
    pub id: String,
    pub email: String,
    pub needs_login: bool,
    pub switchable: bool,
}

pub trait ProviderSource: Send + Sync {
    fn sync_usage(&self);
    fn accounts(&self) -> Vec<ProviderAccountInfo>;
    fn active_id(&self) -> Option<String>;
    fn usage_report(&self, id: &str) -> Option<UsageReport>;
    fn switch_to(&self, id: &str) -> Result<(), String>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoppedJob {
    pub job_id: String,
    pub config_dir: Option<String>,
    pub hit: LimitHit,
}

pub trait StoppedJobs: Send + Sync {
    fn stopped(&self) -> Vec<StoppedJob>;
    fn nudge(&self, job_id: &str, text: &str) -> bool;
}

pub fn stopped_in(agents: &[crate::sessions::Agent]) -> Vec<StoppedJob> {
    use crate::sessions::{AgentGroup, AgentTarget};
    agents
        .iter()
        .filter(|a| a.group != AgentGroup::Working)
        .filter_map(|agent| {
            let AgentTarget::Job(job) = &agent.target else {
                return None;
            };
            let path = agent.transcript_path.as_deref()?;
            let path = successor(path, &agent.name).unwrap_or_else(|| path.to_string());
            Some(StoppedJob {
                job_id: job.clone(),
                config_dir: agent.config_dir.clone(),
                hit: limit_hit_in_file(&path)?,
            })
        })
        .collect()
}
