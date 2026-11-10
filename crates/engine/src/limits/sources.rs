//! What the watchers need from the account stores, as traits, so this part builds and tests with
//! fakes. The daemon wires the real stores in (`notchlingd`'s `limits_sources`: Claude Code's
//! accounts and lanes, the Codex and Antigravity providers, the session list's stopped jobs) behind
//! the `accounts` switch.
//!
//! Every method may be slow or touch a login (a Keychain read, an HTTPS request): the watcher
//! loop calls them only while `accounts` is on, and never otherwise. None of them may renew a
//! token that is in use.

use super::report::UsageReport;
use super::usage::AccountUsage;
use crate::transcript::errors::{limit_hit_in_file, LimitHit};
use crate::transcript::locate::successor;

/// A saved Claude Code account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountInfo {
    pub id: String,
    pub email: String,
    /// Signed out: no saved login to switch to.
    pub needs_login: bool,
}

/// A lane: a Claude Code config folder with its own login.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneInfo {
    pub name: String,
    /// `CLAUDE_CONFIG_DIR`.
    pub path: String,
    pub account_id: String,
    pub email: String,
}

/// Claude Code's accounts (`AccountStore` and `LaneStore`).
pub trait ClaudeSource: Send + Sync {
    /// Fetches every account's limits if they are due (`AccountStore`'s own 5-minute round).
    fn sync_usage(&self);
    /// Fetches every account's limits now, due or not (the Accounts window's Refresh). The same
    /// token rules as [`ClaudeSource::sync_usage`]: a login in use is read, never renewed.
    fn sync_usage_now(&self) {
        self.sync_usage();
    }
    fn accounts(&self) -> Vec<AccountInfo>;
    /// The main login's account.
    fn active_id(&self) -> Option<String>;
    /// The last limits fetched for an account.
    fn usage(&self, id: &str) -> Option<AccountUsage>;
    /// Why the last read of an account's limits failed, in words for the user; `None` when it
    /// worked or none was tried.
    fn usage_error(&self, _id: &str) -> Option<String> {
        None
    }
    fn lanes(&self) -> Vec<LaneInfo>;
    /// Switches the main login. Refuses an account that is live in a lane.
    fn switch_to(&self, id: &str) -> Result<(), String>;
    fn switch_lane(&self, lane: &str, to: &str) -> Result<(), String>;
}

/// An account of another provider (Codex, Antigravity).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderAccountInfo {
    pub id: String,
    pub email: String,
    pub needs_login: bool,
    /// False for an account the provider can't switch to now.
    pub switchable: bool,
}

/// A provider other than Claude Code (`AccountProvider`).
pub trait ProviderSource: Send + Sync {
    /// Fetches the limits if they are due.
    fn sync_usage(&self);
    fn accounts(&self) -> Vec<ProviderAccountInfo>;
    fn active_id(&self) -> Option<String>;
    fn usage_report(&self, id: &str) -> Option<UsageReport>;
    fn switch_to(&self, id: &str) -> Result<(), String>;
}

/// A background job that stopped on the account's limit.
#[derive(Clone, Debug, PartialEq)]
pub struct StoppedJob {
    pub job_id: String,
    /// The lane's folder; `None` for the main login.
    pub config_dir: Option<String>,
    pub hit: LimitHit,
}

/// The jobs that stopped on a limit, and what restarts one after a switch.
pub trait StoppedJobs: Send + Sync {
    /// Background jobs not working whose transcript ends in a usage-limit error.
    fn stopped(&self) -> Vec<StoppedJob>;
    /// Restarts the job on its folder's current login and tells it to go on (`claude respawn`,
    /// then a typed reply). Returns false when it is already being restarted. It must not block
    /// the caller for long: the real one starts a thread.
    fn nudge(&self, job_id: &str, text: &str) -> bool;
}

/// Finds the stopped jobs among the session list's rows by reading each transcript's end.
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
