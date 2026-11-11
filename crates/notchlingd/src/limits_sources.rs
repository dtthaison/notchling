//! The limit watchers' sources ([`notchling_engine::limits::sources`]) over the daemon's real
//! stores, registered on [`crate::limits_ops::LimitsOps`] by the daemon's wiring:
//!
//! - [`ClaudeLimits`]: Claude Code's accounts and lanes ([`ClaudeAccounts`]), with each account's
//!   usage read from `/api/oauth/usage` with [`ClaudeAccounts::usage_token`], which never renews a
//!   token in use (the main login's or a lane's);
//! - [`ProviderLimits`]: Codex's or Antigravity's [`AccountProvider`];
//! - [`SessionJobs`]: the background jobs in the session list that stopped on a limit, and the
//!   nudge that restarts one (`claude respawn`, then a typed reply through M3's attach path).
//!
//! The watcher loop calls none of these while `accounts` is off. Each also checks the switch
//! itself before it touches a login, so a call that slips past the loop still does nothing; the
//! stores' Keychain and network are gated on the same switch underneath. A nudge also needs
//! `acting` on: it types into a session.

use crate::acting_ops::{now_ms, ActingOps, NoLog};
use crate::limits_ops::SYNC_EVERY;
use crate::switches::SwitchFlag;
use notchling_engine::accounts::claude::ClaudeAccounts;
use notchling_engine::accounts::http::{HttpRequest, USAGE_URL};
use notchling_engine::control::{job_state_path, plan_respawn};
use notchling_engine::limits::report::{Bar, BarKind, Group, UsageReport};
use notchling_engine::limits::sources::{
    stopped_in, AccountInfo, ClaudeSource, LaneInfo, ProviderAccountInfo, ProviderSource,
    StoppedJob, StoppedJobs,
};
use notchling_engine::limits::usage::{parse_usage, AccountUsage};

use notchling_engine::providers::usage::{Kind, UsageReport as ProviderReport};
use notchling_engine::providers::AccountProvider;

use notchling_engine::reply::plan as reply_plan;
use notchling_engine::sessions::AgentTarget;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_secs() -> f64 {
    now_ms() as f64 / 1000.0
}

/// Claude Code's accounts for the watcher.
pub struct ClaudeLimits {
    accounts: Arc<SwitchFlag>,
    store: Arc<ClaudeAccounts>,
    /// The last limits read, by account id.
    usage: Mutex<HashMap<String, AccountUsage>>,
    /// Why the last read of an account failed, by account id; gone once one works.
    errors: Mutex<HashMap<String, String>>,
    now: Arc<dyn Fn() -> f64 + Send + Sync>,
}

impl ClaudeLimits {
    pub fn new(accounts: Arc<SwitchFlag>, store: Arc<ClaudeAccounts>) -> ClaudeLimits {
        ClaudeLimits {
            accounts,
            store,
            usage: Mutex::new(HashMap::new()),
            errors: Mutex::new(HashMap::new()),
            now: Arc::new(now_secs),
        }
    }

    /// A fixed clock (tests).
    pub fn with_clock(mut self, now: Arc<dyn Fn() -> f64 + Send + Sync>) -> ClaudeLimits {
        self.now = now;
        self
    }

    /// Whether `id` is the main login's account or a lane's: its token is read, never renewed.
    fn in_use(&self, id: &str) -> bool {
        self.store.snapshot().active.as_deref() == Some(id)
            || self.store.lanes().iter().any(|l| l.account_id == id)
    }

    /// One account's limits, or why there are none (words for the user).
    fn fetch(&self, id: &str, now: f64) -> Result<AccountUsage, String> {
        let expired = || {
            if self.in_use(id) {
                "Its login has expired. Claude Code renews it the next time it runs."
            } else {
                "The saved login has expired. Add the account again to sign in."
            }
        };
        let token = match self.store.usage_token(id) {
            Ok(Some(token)) => token,
            Ok(None) => return Err(expired().into()),
            Err(_) => return Err("The login couldn't be read.".into()),
        };
        let reply = self
            .store
            .env()
            .http
            .send(&HttpRequest::get(USAGE_URL, &token));
        match reply.json_ok() {
            Some(json) => Ok(parse_usage(&Value::Object(json), now)),
            None if reply.status == 0 => Err("No answer from Anthropic. Try Refresh again.".into()),
            None if matches!(reply.status, 401 | 403) => Err(expired().into()),
            None => Err(format!(
                "Anthropic answered with an error ({}).",
                reply.status
            )),
        }
    }

    /// Reads one account and keeps the answer or the reason. A failed read leaves the last good
    /// one in place for the watcher.
    fn read(&self, id: &str, now: f64) {
        match self.fetch(id, now) {
            Ok(usage) => {
                lock(&self.usage).insert(id.to_string(), usage);
                lock(&self.errors).remove(id);
            }
            Err(why) => {
                lock(&self.errors).insert(id.to_string(), why);
            }
        }
    }

    /// Reads the main login (it may have changed under us), then every signed-in account's limits
    /// that were not read in the last round, or all of them when `force`.
    fn sync_with(&self, force: bool) {
        if !self.accounts.is_on() {
            return;
        }
        let _ = self.store.sync();
        let now = (self.now)();
        for account in self.store.saved_list() {
            if !self.accounts.is_on() {
                return;
            }
            if account.needs_login {
                lock(&self.usage).remove(&account.id);
                lock(&self.errors).remove(&account.id);
                continue;
            }
            let due = force
                || lock(&self.usage)
                    .get(&account.id)
                    .is_none_or(|u| now - u.fetched >= SYNC_EVERY);
            if due {
                self.read(&account.id, now);
            }
        }
    }
}

impl ClaudeSource for ClaudeLimits {
    /// Each account's limits not read in the last round (`AccountStore.fetchUsage`).
    fn sync_usage(&self) {
        self.sync_with(false);
    }

    fn sync_usage_now(&self) {
        self.sync_with(true);
    }

    fn usage_error(&self, id: &str) -> Option<String> {
        lock(&self.errors).get(id).cloned()
    }

    fn accounts(&self) -> Vec<AccountInfo> {
        self.store
            .saved_list()
            .into_iter()
            .map(|a| AccountInfo {
                id: a.id,
                email: a.email,
                needs_login: a.needs_login,
            })
            .collect()
    }

    fn active_id(&self) -> Option<String> {
        self.store.snapshot().active
    }

    fn usage(&self, id: &str) -> Option<AccountUsage> {
        lock(&self.usage).get(id).cloned()
    }

    fn lanes(&self) -> Vec<LaneInfo> {
        self.store
            .lanes()
            .into_iter()
            .map(|l| LaneInfo {
                name: l.name,
                path: l.path,
                account_id: l.account_id,
                email: l.email,
            })
            .collect()
    }

    fn switch_to(&self, id: &str) -> Result<(), String> {
        self.accounts.check().map_err(|e| e.message)?;
        self.store.switch_to(id).map(|_| ()).map_err(|e| e.0)
    }

    fn switch_lane(&self, lane: &str, to: &str) -> Result<(), String> {
        self.accounts.check().map_err(|e| e.message)?;
        self.store
            .switch_lane(lane, to)
            .map(|_| ())
            .map_err(|e| e.0)
    }
}

/// Codex's or Antigravity's accounts for the watcher.
pub struct ProviderLimits {
    accounts: Arc<SwitchFlag>,
    provider: Arc<dyn AccountProvider>,
    /// The watcher's own switch, when it differs from the provider's (Antigravity's leaves a
    /// closed app closed).
    switch: Option<SwitchFn>,
}

type SwitchFn = Box<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

impl ProviderLimits {
    pub fn new(accounts: Arc<SwitchFlag>, provider: Arc<dyn AccountProvider>) -> ProviderLimits {
        ProviderLimits {
            accounts,
            provider,
            switch: None,
        }
    }

    pub fn with_switch(mut self, switch: SwitchFn) -> ProviderLimits {
        self.switch = Some(switch);
        self
    }
}

/// The registry's report as the watchers' (the same fields; two parts made them apart).
pub fn report_of(report: &ProviderReport) -> UsageReport {
    let kind = |k: Kind| match k {
        Kind::Session => BarKind::Session,
        Kind::Week => BarKind::Week,
        Kind::Other => BarKind::Other,
    };
    UsageReport {
        groups: report
            .groups
            .iter()
            .map(|g| Group {
                name: g.name.clone(),
                bars: g
                    .bars
                    .iter()
                    .map(|b| Bar {
                        label: b.label.clone(),
                        kind: kind(b.kind),
                        percent: b.percent,
                        resets_at: b.resets_at,
                    })
                    .collect(),
            })
            .collect(),
        fetched: report.fetched,
    }
}

impl ProviderSource for ProviderLimits {
    fn sync_usage(&self) {
        if self.accounts.is_on() {
            self.provider.sync(false);
        }
    }

    fn accounts(&self) -> Vec<ProviderAccountInfo> {
        self.provider
            .state()
            .accounts
            .into_iter()
            .map(|a| ProviderAccountInfo {
                id: a.id,
                email: a.email,
                needs_login: a.needs_login,
                switchable: a.switchable,
            })
            .collect()
    }

    fn active_id(&self) -> Option<String> {
        self.provider.state().active_id
    }

    fn usage_report(&self, id: &str) -> Option<UsageReport> {
        self.provider.usage_report(id).as_ref().map(report_of)
    }

    fn switch_to(&self, id: &str) -> Result<(), String> {
        self.accounts.check().map_err(|e| e.message)?;
        if let Some(switch) = &self.switch {
            return switch(id);
        }
        self.provider
            .switch_to(id)
            .map_err(|e| e.message().to_string())
    }
}

/// How long a respawned job's state file may take to move, and how long it settles after.
#[derive(Clone, Copy, Debug)]
pub struct NudgeTiming {
    pub respawn_wait: Duration,
    pub poll: Duration,
    pub settle: Duration,
}

impl Default for NudgeTiming {
    /// `LimitWatcher.respawn`'s: up to 60 s, looking every 2 s, then 6 s.
    fn default() -> Self {
        NudgeTiming {
            respawn_wait: Duration::from_secs(60),
            poll: Duration::from_secs(2),
            settle: Duration::from_secs(6),
        }
    }
}

/// The session list's jobs that stopped on a limit, and the nudge.
pub struct SessionJobs {
    acting: Arc<ActingOps>,
    /// Jobs being restarted now.
    working: Arc<Mutex<HashSet<String>>>,
    timing: NudgeTiming,
}

impl SessionJobs {
    pub fn new(acting: Arc<ActingOps>) -> SessionJobs {
        SessionJobs {
            acting,
            working: Arc::new(Mutex::new(HashSet::new())),
            timing: NudgeTiming::default(),
        }
    }

    pub fn with_timing(mut self, timing: NudgeTiming) -> SessionJobs {
        self.timing = timing;
        self
    }

    /// `claude respawn`, a wait for the job to run again, then the reply. Each step looks at
    /// `acting` first: turned off meanwhile, the rest is dropped.
    fn restart(acting: &ActingOps, job_id: &str, text: &str, timing: NudgeTiming) {
        let state = job_state_path(&acting.roots, job_id);
        let stamp = || std::fs::metadata(&state).and_then(|m| m.modified()).ok();
        let before: Option<SystemTime> = stamp();
        if !acting.is_on()
            || acting
                .run_raw(&plan_respawn(job_id, &acting.roots))
                .is_err()
        {
            return;
        }
        let deadline = std::time::Instant::now() + timing.respawn_wait;
        while std::time::Instant::now() < deadline && stamp() == before {
            std::thread::sleep(timing.poll);
        }
        std::thread::sleep(timing.settle);
        if !acting.is_on() {
            return;
        }
        let agent = acting
            .agents_now()
            .into_iter()
            .find(|a| matches!(&a.target, AgentTarget::Job(id) if id == job_id));
        if let Some(agent) = agent {
            let _ = acting.play(&agent, &reply_plan::reply(text), &mut NoLog);
        }
    }
}

impl StoppedJobs for SessionJobs {
    fn stopped(&self) -> Vec<StoppedJob> {
        stopped_in(&self.acting.agents_now())
    }

    /// False when it is already being restarted, or `acting` is off (nothing may type into a
    /// session then).
    fn nudge(&self, job_id: &str, text: &str) -> bool {
        if !self.acting.is_on() || !lock(&self.working).insert(job_id.to_string()) {
            return false;
        }
        let (acting, working, timing) = (self.acting.clone(), self.working.clone(), self.timing);
        let (job, text) = (job_id.to_string(), text.to_string());
        let spawned = std::thread::Builder::new()
            .name("limits-nudge".into())
            .spawn(move || {
                SessionJobs::restart(&acting, &job, &text, timing);
                lock(&working).remove(&job);
            });
        if spawned.is_err() {
            lock(&self.working).remove(job_id);
            return false;
        }
        true
    }
}

#[cfg(test)]
#[path = "limits_sources_tests.rs"]
mod tests;
