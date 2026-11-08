//! `LimitWatcher`: watches the account Claude Code is using. Near a limit it says so; with Notify
//! and Switch it moves Claude Code to the saved account with the most room, then restarts and
//! nudges background agents that stopped on the limit so they read the new login. Each lane
//! switches on its own.
//!
//! It reads limits the store already fetched, never fetches one itself, never renews a token and
//! switches only through [`ClaudeSource::switch_to`] / [`ClaudeSource::switch_lane`]. It runs only
//! while the daemon's `accounts` switch is on (the loop checks it, see `notchlingd::limits_loop`).
//!
//! The Swift watcher switched asynchronously and returned at once; this one switches inside
//! `check`, on the loop's thread, and the next `check` does the nudging.

use super::logic::{
    announcement, blocked_until, lane_pool, may_switch, pick_target, Candidate, Mark,
    IN_USE_FRESH_FOR, SWITCH_GAP,
};
use super::mode::{LimitConfig, LimitMode, CLAUDE};
use super::sources::{AccountInfo, ClaudeSource, StoppedJob, StoppedJobs};
use super::text::{climb, clock, Zone};
use super::usage::{AccountUsage, UsageWindow};
use crate::transcript::errors::LimitHit;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// What a restarted job is told.
pub const NUDGE: &str =
    "Switched to another account after the usage limit. Continue where you stopped.";
/// Only limit errors this fresh start a switch; older ones were someone else's.
const FRESH_HIT: f64 = 10.0 * 60.0;
/// Errors this old still get their job restarted once a switch happens.
const NUDGE_WINDOW: f64 = 12.0 * 3600.0;
/// Nudges follow a switch for this long.
const NUDGE_AFTER_SWITCH: f64 = 2.0 * 3600.0;
const TRANSCRIPT_EVERY: f64 = 15.0;

/// Where messages go (sound, notch peek, notification: the app's job).
pub type Announce = Arc<dyn Fn(&str) + Send + Sync>;

/// What the watcher last saw, for `limits.status`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Observed {
    pub active_email: Option<String>,
    /// Name, percent and reset of each window it follows.
    pub windows: Vec<(String, f64, Option<f64>)>,
    pub last_switch: Option<f64>,
    pub checked: Option<f64>,
}

enum Outcome {
    /// A switch ran (or failed): the message has been said.
    Switched,
    /// A message about the lack of a target has been said.
    Announced,
    /// Nothing was said; the caller says the climb.
    Nothing,
}

pub struct ClaudeWatcher {
    source: Arc<dyn ClaudeSource>,
    stopped: Arc<dyn StoppedJobs>,
    config: Arc<LimitConfig>,
    announce: Announce,
    zone: Zone,
    marks: HashMap<String, Mark>,
    blocked: HashMap<String, f64>,
    last_switch: Option<f64>,
    last_no_room: Option<f64>,
    active_seen: Option<(String, f64)>,
    last_scan: f64,
    cached_hits: Vec<StoppedJob>,
    switched_hits: HashSet<String>,
    nudged: HashSet<String>,
    nudged_job_at: HashMap<String, f64>,
    lane_switched: HashMap<String, f64>,
    lane_no_room: HashMap<String, f64>,
    observed: Observed,
}

impl ClaudeWatcher {
    pub fn new(
        source: Arc<dyn ClaudeSource>,
        stopped: Arc<dyn StoppedJobs>,
        config: Arc<LimitConfig>,
        announce: Announce,
    ) -> ClaudeWatcher {
        ClaudeWatcher {
            source,
            stopped,
            config,
            announce,
            zone: Zone::LOCAL,
            marks: HashMap::new(),
            blocked: HashMap::new(),
            last_switch: None,
            last_no_room: None,
            active_seen: None,
            last_scan: f64::NEG_INFINITY,
            cached_hits: Vec::new(),
            switched_hits: HashSet::new(),
            nudged: HashSet::new(),
            nudged_job_at: HashMap::new(),
            lane_switched: HashMap::new(),
            lane_no_room: HashMap::new(),
            observed: Observed::default(),
        }
    }

    /// A fixed zone for the clock in messages (tests).
    pub fn with_zone(mut self, zone: Zone) -> ClaudeWatcher {
        self.zone = zone;
        self
    }

    pub fn observed(&self) -> Observed {
        Observed {
            last_switch: self.last_switch,
            ..self.observed.clone()
        }
    }

    pub fn mode(&self) -> LimitMode {
        self.config.mode(&CLAUDE)
    }

    fn say(&self, text: &str) {
        (self.announce)(text);
    }

    /// One round, at `now` (seconds since 1970).
    pub fn check(&mut self, now: f64) {
        let mode = self.mode();
        if mode == LimitMode::Off {
            return;
        }
        let switch_at = self.config.switch_at();
        self.check_lanes(now, mode, switch_at);
        let accounts = self.source.accounts();
        let Some(active) = self
            .source
            .active_id()
            .and_then(|id| accounts.iter().find(|a| a.id == id).cloned())
        else {
            return;
        };
        if self.active_seen.as_ref().map(|s| s.0.as_str()) != Some(active.id.as_str()) {
            self.active_seen = Some((active.id.clone(), now));
        }
        let usage = self.source.usage(&active.id);
        let fresh = usage
            .as_ref()
            .is_some_and(|u| now - u.fetched <= IN_USE_FRESH_FOR);
        let mut windows: Vec<(String, Option<UsageWindow>)> = vec![
            (
                "5-hour".into(),
                usage.as_ref().filter(|_| fresh).and_then(|u| u.session),
            ),
            (
                "weekly".into(),
                usage.as_ref().filter(|_| fresh).and_then(|u| u.week),
            ),
        ];
        if let Some(u) = usage.as_ref().filter(|_| fresh) {
            windows.extend(u.other.iter().map(|o| (o.name.clone(), Some(o.window))));
        }
        self.observed = Observed {
            active_email: Some(active.email.clone()),
            windows: windows
                .iter()
                .filter_map(|(n, w)| w.map(|w| (n.clone(), w.percent, w.resets_at)))
                .collect(),
            last_switch: self.last_switch,
            checked: Some(now),
        };
        let skip90 = mode == LimitMode::NotifyAndSwitch && switch_at <= 90;
        let fired = announcement(&mut self.marks, &active.id, &windows, skip90)
            .map(|f| climb("", &active.email, &f, now, self.zone));

        // What pushes toward a switch: a window at the chosen point, or a job stopped on the limit.
        let hits = self.limit_hits(now);
        let mut reason = windows
            .iter()
            .filter_map(|(_, w)| *w)
            .any(|w| w.percent >= switch_at as f64);
        if !reason {
            let since = self.active_seen.as_ref().map(|s| s.1);
            reason = hits.iter().any(|h| self.is_new(&h.hit, since, now));
        }

        if mode == LimitMode::NotifyAndSwitch && reason {
            match self.try_switch(&active, usage.as_ref(), &accounts, now, switch_at) {
                Outcome::Switched => return,
                Outcome::Announced => {}
                Outcome::Nothing => {
                    if let Some(text) = &fired {
                        self.say(text);
                    }
                }
            }
        } else if let Some(text) = &fired {
            self.say(text);
        }

        // Each switch restarts only the jobs it moved: the main login's, or one lane's.
        if let Some(switched) = self.last_switch {
            let main: Vec<_> = hits.iter().filter(|h| h.config_dir.is_none()).collect();
            self.nudge_stopped(&main, switched, now);
        }
        for lane in self.source.lanes() {
            let Some(switched) = self.lane_switched.get(&lane.name).copied() else {
                continue;
            };
            let theirs: Vec<_> = hits
                .iter()
                .filter(|h| h.config_dir.as_deref() == Some(lane.path.as_str()))
                .collect();
            self.nudge_stopped(&theirs, switched, now);
        }
    }

    // MARK: Switching

    fn try_switch(
        &mut self,
        active: &AccountInfo,
        usage: Option<&AccountUsage>,
        accounts: &[AccountInfo],
        now: f64,
        switch_at: i64,
    ) -> Outcome {
        if !may_switch(self.last_switch, now) {
            return Outcome::Nothing;
        }
        let candidates = self.candidates(accounts);
        let target = pick_target(&candidates, Some(&active.id), now, &self.blocked)
            .and_then(|t| accounts.iter().find(|a| a.id == t.id));
        let Some(to) = target else {
            // Say it once per half hour; the climb itself may already have been announced.
            if self.last_no_room.is_some_and(|t| now - t <= SWITCH_GAP) {
                return Outcome::Announced;
            }
            self.last_no_room = Some(now);
            let reset = usage
                .map(|u| u.stopping_windows())
                .unwrap_or_default()
                .into_iter()
                .filter(|w| w.percent >= switch_at as f64)
                .filter_map(|w| w.resets_at)
                .fold(None, |best: Option<f64>, r| {
                    Some(best.map_or(r, |b| b.max(r)))
                });
            let when = reset
                .map(|r| format!("; resets {}", clock(r, now, self.zone)))
                .unwrap_or_default();
            self.say(&format!(
                "{} is near its limit and no other account has room left{when}.",
                active.email
            ));
            return Outcome::Announced;
        };
        let until = blocked_until(
            usage.and_then(|u| u.session.as_ref()),
            usage.and_then(|u| u.week.as_ref()),
            &usage
                .map(|u| u.other.iter().map(|o| o.window).collect::<Vec<_>>())
                .unwrap_or_default(),
            switch_at as f64,
            now,
        );
        match self.source.switch_to(&to.id) {
            Err(e) => self.say(&format!("Couldn't switch accounts: {e}")),
            Ok(()) => {
                self.last_switch = Some(now);
                self.last_no_room = None;
                self.blocked.insert(active.id.clone(), until);
                let room = self
                    .source
                    .usage(&to.id)
                    .map(|u| {
                        u.session
                            .map_or(0.0, |w| w.percent)
                            .max(u.week.map_or(0.0, |w| w.percent))
                    })
                    .unwrap_or(0.0);
                self.say(&format!(
                    "Switched from {} to {} ({}% used) because of the usage limit.",
                    active.email,
                    to.email,
                    room.round() as i64
                ));
            }
        }
        Outcome::Switched
    }

    /// Every saved account as a switch target; an account live in a lane can't be taken from it.
    fn candidates(&self, accounts: &[AccountInfo]) -> Vec<Candidate> {
        let in_lanes: HashSet<String> = self
            .source
            .lanes()
            .into_iter()
            .map(|l| l.account_id)
            .collect();
        accounts
            .iter()
            .map(|a| {
                let u = self.source.usage(&a.id);
                // An unknown account-wide limit leaves no more room than the 5-hour one it sits beside.
                let short = u
                    .as_ref()
                    .map(|u| {
                        u.session
                            .iter()
                            .map(|w| w.percent)
                            .chain(u.other.iter().map(|o| o.window.percent))
                            .fold(None, |best: Option<f64>, p| {
                                Some(best.map_or(p, |b| b.max(p)))
                            })
                    })
                    .unwrap_or(None);
                Candidate {
                    id: a.id.clone(),
                    fetched: u.as_ref().map_or(f64::NEG_INFINITY, |u| u.fetched),
                    session: short,
                    week: u.as_ref().and_then(|u| u.week).map(|w| w.percent),
                    needs_login: a.needs_login || in_lanes.contains(&a.id),
                }
            })
            .collect()
    }

    // MARK: Lanes

    /// Each lane switches on its own: when its account reaches the switch point, the lane moves to
    /// the saved account with the most room that isn't live anywhere, and its sessions carry on.
    fn check_lanes(&mut self, now: f64, mode: LimitMode, switch_at: i64) {
        if mode != LimitMode::NotifyAndSwitch {
            return;
        }
        let accounts = self.source.accounts();
        let active_id = self.source.active_id();
        for lane in self.source.lanes() {
            let Some(usage) = self
                .source
                .usage(&lane.account_id)
                .filter(|u| now - u.fetched <= IN_USE_FRESH_FOR)
            else {
                continue;
            };
            let windows = usage.stopping_windows();
            if !windows.iter().any(|w| w.percent >= switch_at as f64)
                || !may_switch(self.lane_switched.get(&lane.name).copied(), now)
            {
                continue;
            }
            let pool = lane_pool(self.candidates(&accounts), active_id.as_deref());
            let target = pick_target(&pool, Some(&lane.account_id), now, &self.blocked)
                .and_then(|t| accounts.iter().find(|a| a.id == t.id));
            let Some(to) = target else {
                let due = self
                    .lane_no_room
                    .get(&lane.name)
                    .is_none_or(|t| now - t > SWITCH_GAP);
                if due {
                    self.lane_no_room.insert(lane.name.clone(), now);
                    self.say(&format!(
                        "The {} lane ({}) is near its limit and no free account has room left.",
                        lane.name, lane.email
                    ));
                }
                continue;
            };
            let until = blocked_until(
                usage.session.as_ref(),
                usage.week.as_ref(),
                &usage.other.iter().map(|o| o.window).collect::<Vec<_>>(),
                switch_at as f64,
                now,
            );
            match self.source.switch_lane(&lane.name, &to.id) {
                Err(e) => self.say(&format!("Couldn't switch the {} lane: {e}", lane.name)),
                Ok(()) => {
                    self.lane_switched.insert(lane.name.clone(), now);
                    self.lane_no_room.remove(&lane.name);
                    self.blocked.insert(lane.account_id.clone(), until);
                    self.say(&format!(
                        "The {} lane switched from {} to {} because of the usage limit. Its sessions carry on.",
                        lane.name, lane.email, to.email
                    ));
                }
            }
        }
    }

    // MARK: Jobs stopped on the limit

    fn limit_hits(&mut self, now: f64) -> Vec<StoppedJob> {
        if now - self.last_scan >= TRANSCRIPT_EVERY {
            self.last_scan = now;
            self.cached_hits = self.stopped.stopped();
        }
        self.cached_hits.clone()
    }

    fn is_new(&self, hit: &LimitHit, since: Option<f64>, now: f64) -> bool {
        if self.switched_hits.contains(&hit.id) {
            return false;
        }
        let Some(at) = hit.at else { return false };
        now - at <= FRESH_HIT && since.is_none_or(|s| at > s)
    }

    /// After a switch: restart each background job that stopped on the limit, then tell it to go
    /// on. One nudge per error entry, and per job at most once in half an hour.
    fn nudge_stopped(&mut self, hits: &[&StoppedJob], switched: f64, now: f64) {
        if now - switched >= NUDGE_AFTER_SWITCH {
            return;
        }
        for job in hits {
            let recent = job.hit.at.is_some_and(|at| now - at <= NUDGE_WINDOW);
            let spaced = self
                .nudged_job_at
                .get(&job.job_id)
                .is_none_or(|t| now - t > SWITCH_GAP);
            if self.nudged.contains(&job.hit.id) || !recent || !spaced {
                continue;
            }
            if !self.stopped.nudge(&job.job_id, NUDGE) {
                continue;
            }
            self.nudged.insert(job.hit.id.clone());
            self.switched_hits.insert(job.hit.id.clone());
            self.nudged_job_at.insert(job.job_id.clone(), now);
        }
    }
}
