//! `ProviderLimitWatcher`: watches the active account of a provider other than Claude Code (Codex,
//! Antigravity). Near a limit it says so; with Notify and Switch (only where the provider allows
//! it) it moves to the saved account with the most room. It reads limits the provider already
//! fetched, never renews a token, and a switch is the provider's own `switch_to`.
//!
//! Codex switches only while nothing would be interrupted (`can_switch_now`: no `codex` process,
//! the ChatGPT app closed); Antigravity is Off or Notify only, since a switch restarts the app.

use super::claude::{Announce, Observed};
use super::logic::{
    announcement, blocked_until, decide_switch, watched_windows, Candidate, Mark, SwitchDecision,
    IN_USE_FRESH_FOR, SWITCH_GAP,
};
use super::mode::{LimitConfig, LimitMode, WatchedProvider, ANTIGRAVITY, CODEX};
use super::sources::ProviderSource;
use super::text::{climb, Zone};
use super::usage::UsageWindow;
use std::collections::HashMap;
use std::sync::Arc;

/// The busy check may list processes; while busy, look again at most this often.
const BUSY_RECHECK: f64 = 60.0;

/// What differs between providers.
#[derive(Clone, Debug)]
pub struct Setup {
    pub provider: WatchedProvider,
    /// Codex: "Codex" (the 5h and week bars). `None` follows every group (Antigravity).
    pub primary_group: Option<String>,
    pub busy_text: String,
}

impl Setup {
    pub fn codex() -> Setup {
        Setup {
            provider: CODEX,
            primary_group: Some("Codex".into()),
            busy_text: "A Codex session is running, so Notchling won't switch now.".into(),
        }
    }

    pub fn antigravity() -> Setup {
        Setup {
            provider: ANTIGRAVITY,
            primary_group: None,
            busy_text: String::new(),
        }
    }
}

pub struct ProviderLimitWatcher {
    source: Arc<dyn ProviderSource>,
    setup: Setup,
    config: Arc<LimitConfig>,
    /// The switch point; Codex follows Claude's, Antigravity the default.
    switch_at: Box<dyn Fn() -> i64 + Send + Sync>,
    can_switch_now: Box<dyn Fn() -> bool + Send + Sync>,
    announce: Announce,
    zone: Zone,
    marks: HashMap<String, Mark>,
    blocked: HashMap<String, f64>,
    last_switch: Option<f64>,
    last_note: Option<f64>,
    busy_since: Option<f64>,
    observed: Observed,
}

impl ProviderLimitWatcher {
    pub fn new(
        source: Arc<dyn ProviderSource>,
        setup: Setup,
        config: Arc<LimitConfig>,
        switch_at: Box<dyn Fn() -> i64 + Send + Sync>,
        can_switch_now: Box<dyn Fn() -> bool + Send + Sync>,
        announce: Announce,
    ) -> ProviderLimitWatcher {
        ProviderLimitWatcher {
            source,
            setup,
            config,
            switch_at,
            can_switch_now,
            announce,
            zone: Zone::LOCAL,
            marks: HashMap::new(),
            blocked: HashMap::new(),
            last_switch: None,
            last_note: None,
            busy_since: None,
            observed: Observed::default(),
        }
    }

    pub fn with_zone(mut self, zone: Zone) -> ProviderLimitWatcher {
        self.zone = zone;
        self
    }

    pub fn observed(&self) -> Observed {
        Observed {
            last_switch: self.last_switch,
            ..self.observed.clone()
        }
    }

    pub fn setup(&self) -> &Setup {
        &self.setup
    }

    pub fn mode(&self) -> LimitMode {
        self.config.mode(&self.setup.provider)
    }

    fn say(&self, text: &str) {
        (self.announce)(text);
    }

    fn prefix(&self) -> String {
        format!("{}: ", self.setup.provider.label)
    }

    /// One round, at `now` (seconds since 1970).
    pub fn check(&mut self, now: f64) {
        let mode = self.mode();
        if mode == LimitMode::Off {
            return;
        }
        let accounts = self.source.accounts();
        let Some(active) = self
            .source
            .active_id()
            .and_then(|id| accounts.iter().find(|a| a.id == id).cloned())
        else {
            return;
        };
        let Some(report) = self
            .source
            .usage_report(&active.id)
            .filter(|r| now - r.fetched <= IN_USE_FRESH_FOR)
        else {
            return;
        };
        let group = self.setup.primary_group.as_deref();
        let windows = watched_windows(&report, group);
        self.observed = Observed {
            active_email: Some(active.email.clone()),
            windows: windows
                .iter()
                .map(|(n, w)| (n.clone(), w.percent, w.resets_at))
                .collect(),
            last_switch: self.last_switch,
            checked: Some(now),
        };
        let will_switch = mode == LimitMode::NotifyAndSwitch;
        let switch_at = (self.switch_at)();
        let skip90 = will_switch && switch_at <= 90;
        let named: Vec<(String, Option<UsageWindow>)> =
            windows.iter().map(|(n, w)| (n.clone(), Some(*w))).collect();
        let text = announcement(&mut self.marks, &active.id, &named, skip90)
            .map(|f| climb(&self.prefix(), &active.email, &f, now, self.zone));
        if !will_switch {
            if let Some(text) = text {
                self.say(&text);
            }
            return;
        }

        let candidates: Vec<Candidate> = accounts
            .iter()
            .map(|a| {
                Candidate::from_report(
                    &a.id,
                    self.source.usage_report(&a.id).as_ref(),
                    group,
                    a.needs_login || !a.switchable,
                )
            })
            .collect();
        // The idle check may list processes; while busy, look again at most once a minute.
        let mut busy_since = self.busy_since;
        let can_switch_now = &self.can_switch_now;
        let mut idle = || {
            if busy_since.is_some_and(|since| now - since < BUSY_RECHECK) {
                return false;
            }
            let ok = can_switch_now();
            busy_since = if ok { None } else { Some(now) };
            ok
        };
        let decision = decide_switch(
            &windows,
            switch_at,
            Some(&active.id),
            &candidates,
            &self.blocked,
            self.last_switch,
            now,
            &mut idle,
        );
        self.busy_since = busy_since;
        match decision {
            SwitchDecision::Stay => {
                if let Some(text) = text {
                    self.say(&text);
                }
            }
            SwitchDecision::Busy => {
                let why = format!(
                    "{} Switch accounts yourself when it's done.",
                    self.setup.busy_text
                );
                self.note(text, &why, now);
            }
            SwitchDecision::NoRoom => {
                let why = format!(
                    "No other {} account has room left.",
                    self.setup.provider.label
                );
                self.note(text, &why, now);
            }
            SwitchDecision::SwitchTo(id) => {
                let Some(to) = accounts.iter().find(|a| a.id == id) else {
                    return;
                };
                let others: Vec<UsageWindow> = windows.iter().map(|(_, w)| *w).collect();
                let until = blocked_until(None, None, &others, switch_at as f64, now);
                match self.source.switch_to(&id) {
                    Err(e) => self.say(&format!("{}couldn't switch accounts: {e}", self.prefix())),
                    Ok(()) => {
                        self.last_switch = Some(now);
                        self.blocked.insert(active.id.clone(), until);
                        let room = self
                            .source
                            .usage_report(&id)
                            .map(|r| {
                                watched_windows(&r, group)
                                    .iter()
                                    .map(|(_, w)| w.percent)
                                    .fold(0.0, f64::max)
                            })
                            .unwrap_or(0.0);
                        self.say(&format!(
                            "{}switched from {} to {} ({}% used) because of the usage limit.",
                            self.prefix(),
                            active.email,
                            to.email,
                            room.round() as i64
                        ));
                    }
                }
            }
        }
    }

    /// The climb itself when it just crossed 90 or 100, with the reason no switch followed;
    /// otherwise the reason alone, at most once per half hour.
    fn note(&mut self, text: Option<String>, reason: &str, now: f64) {
        if let Some(text) = text {
            self.last_note = Some(now);
            self.say(&format!("{text} {reason}"));
        } else if self.last_note.is_none_or(|t| now - t > SWITCH_GAP) {
            self.last_note = Some(now);
            self.say(&format!("{}{reason}", self.prefix()));
        }
    }
}
