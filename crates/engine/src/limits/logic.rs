//! `LimitLogic`: the decisions, as pure functions so they can be checked without any login. They
//! decide when a real account switch happens, so they are tested here and never by switching.
//!
//! Times are seconds since 1970 (`f64`), passed in as `now`; nothing here reads a clock.

use super::report::{Bar, BarKind, UsageReport};
use super::usage::UsageWindow;
use std::cmp::Ordering;
use std::collections::HashMap;

/// The switch point stays in this range (`limitSwitchAt`).
pub const SWITCH_RANGE: (i64, i64) = (80, 99);
pub const DEFAULT_SWITCH_AT: i64 = 95;
/// An account must have limits fetched this recently, and both under [`ROOM_BELOW`], to be switched
/// to.
pub const FRESH_FOR: f64 = 10.0 * 60.0;
/// How old the limits of an account already in use (main login or a lane) may be and still count.
/// Looser than [`FRESH_FOR`] on purpose: a switch target must be current before sessions move onto
/// it, while the account in use is refreshed every 5 minutes and skipped whenever Claude Code's
/// token is expired, so it needs a few missed rounds of slack before its warnings stop.
pub const IN_USE_FRESH_FOR: f64 = 15.0 * 60.0;
pub const ROOM_BELOW: f64 = 80.0;
/// At most one automatic switch per this long.
pub const SWITCH_GAP: f64 = 30.0 * 60.0;
/// With no window over the line, the account we left stays blocked this long.
pub const DEFAULT_BLOCK: f64 = 5.0 * 3600.0;

/// One window's notification state: the highest level already announced, and the window it
/// belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mark {
    pub level: u32,
    pub resets_at: Option<f64>,
}

/// Which level (90 or 100) a reading has reached.
pub fn level(percent: f64) -> u32 {
    if percent >= 100.0 {
        100
    } else if percent >= 90.0 {
        90
    } else {
        0
    }
}

/// Announces each level once per window. The mark falls back when usage drops or the window
/// resets, so the next climb is announced again. `skip90`: the account is about to be switched at
/// 90, so a separate warning would only repeat it.
pub fn step(mark: Mark, percent: f64, resets_at: Option<f64>, skip90: bool) -> (Mark, Option<u32>) {
    let mut m = mark;
    if let (Some(a), Some(b)) = (m.resets_at, resets_at) {
        if (a - b).abs() > 120.0 {
            m.level = 0;
        }
    }
    m.resets_at = resets_at;
    let now = level(percent);
    if now < m.level {
        m.level = now;
    }
    if now <= m.level {
        return (m, None);
    }
    m.level = now;
    (m, if now == 90 && skip90 { None } else { Some(now) })
}

/// A saved account as a switch target.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub id: String,
    pub fetched: f64,
    pub session: Option<f64>,
    pub week: Option<f64>,
    pub needs_login: bool,
}

impl Candidate {
    pub fn new(id: &str, fetched: f64, session: Option<f64>, week: Option<f64>) -> Candidate {
        Candidate {
            id: id.to_string(),
            fetched,
            session,
            week,
            needs_login: false,
        }
    }

    /// From a report: `session` is the highest short or unknown window, `week` the highest weekly
    /// one. No report is an account with nothing known, fetched never.
    pub fn from_report(
        id: &str,
        report: Option<&UsageReport>,
        primary_group: Option<&str>,
        needs_login: bool,
    ) -> Candidate {
        let bars: Vec<Bar> = report
            .map(|r| {
                watched_bars(r, primary_group)
                    .into_iter()
                    .map(|(_, b)| b.clone())
                    .collect()
            })
            .unwrap_or_default();
        let top = |pick: &dyn Fn(&Bar) -> bool| {
            bars.iter()
                .filter(|b| pick(b))
                .map(|b| b.percent)
                .fold(None, |best: Option<f64>, p| {
                    Some(best.map_or(p, |b| b.max(p)))
                })
        };
        Candidate {
            id: id.to_string(),
            fetched: report.map_or(f64::NEG_INFINITY, |r| r.fetched),
            session: top(&|b| b.kind != BarKind::Week),
            week: top(&|b| b.kind == BarKind::Week),
            needs_login,
        }
    }
}

fn room_used(c: &Candidate) -> f64 {
    c.session.unwrap_or(0.0).max(c.week.unwrap_or(0.0))
}

/// The saved account with the most room: fresh data, both windows under 80%, not the one in use,
/// and not one we left that hasn't reset yet. Lowest max(session, week) wins; ties go to the first
/// listed.
pub fn pick_target<'a>(
    all: &'a [Candidate],
    active_id: Option<&str>,
    now: f64,
    blocked_until: &HashMap<String, f64>,
) -> Option<&'a Candidate> {
    all.iter()
        .filter(|c| {
            if Some(c.id.as_str()) == active_id
                || c.needs_login
                || now - c.fetched > FRESH_FOR
                || (c.session.is_none() && c.week.is_none())
                || c.session.unwrap_or(0.0) >= ROOM_BELOW
                || c.week.unwrap_or(0.0) >= ROOM_BELOW
            {
                return false;
            }
            blocked_until.get(&c.id).is_none_or(|until| *until <= now)
        })
        .min_by(|a, b| {
            room_used(a)
                .partial_cmp(&room_used(b))
                .unwrap_or(Ordering::Equal)
        })
}

/// At most one automatic switch per half hour.
pub fn may_switch(last_switch: Option<f64>, now: f64) -> bool {
    last_switch.is_none_or(|last| now - last >= SWITCH_GAP)
}

/// When the account we leave becomes usable again: the latest reset among its windows at or over
/// `at`; five hours from now when none has one.
pub fn blocked_until(
    session: Option<&UsageWindow>,
    week: Option<&UsageWindow>,
    other: &[UsageWindow],
    at: f64,
    now: f64,
) -> f64 {
    session
        .into_iter()
        .chain(week)
        .chain(other.iter())
        .filter(|w| w.percent >= at)
        .filter_map(|w| w.resets_at)
        .fold(None, |best: Option<f64>, r| {
            Some(best.map_or(r, |b| b.max(r)))
        })
        .unwrap_or(now + DEFAULT_BLOCK)
}

/// A window with the name a message reads it by.
pub type Named = (String, UsageWindow);

/// The bars a watcher follows. Codex: its main group ("Codex": the 5h and week bars), not Code
/// Review or a model-specific extra. Antigravity (`primary_group` `None`): every group, since each
/// is a pool of its own.
pub fn watched_bars<'a>(
    report: &'a UsageReport,
    primary_group: Option<&str>,
) -> Vec<(Option<&'a str>, &'a Bar)> {
    let groups: Vec<_> = match primary_group {
        Some(name) => report
            .groups
            .iter()
            .find(|g| g.name.as_deref() == Some(name))
            .or_else(|| report.groups.first())
            .into_iter()
            .collect(),
        None => report.groups.iter().collect(),
    };
    groups
        .into_iter()
        .flat_map(|g| g.bars.iter().map(move |b| (g.name.as_deref(), b)))
        .collect()
}

/// The windows to announce and switch on, named the way a message reads them ("5-hour", "weekly",
/// or "Gemini 3 Pro weekly" when the group matters).
pub fn watched_windows(report: &UsageReport, primary_group: Option<&str>) -> Vec<Named> {
    watched_bars(report, primary_group)
        .into_iter()
        .map(|(group, bar)| {
            let kind = match bar.kind {
                BarKind::Session => "5-hour".to_string(),
                BarKind::Week => "weekly".to_string(),
                BarKind::Other => bar.label.to_lowercase(),
            };
            let name = if primary_group.is_none() {
                match group {
                    Some(g) => format!("{g} {kind}"),
                    None => kind,
                }
            } else {
                kind
            };
            (name, bar.window())
        })
        .collect()
}

/// What fired: the level, the window's name and the window.
#[derive(Clone, Debug, PartialEq)]
pub struct Fired {
    pub level: u32,
    pub name: String,
    pub window: UsageWindow,
}

/// Steps every window's mark and returns the highest level that fired now (the later window wins a
/// tie). Claude's watcher and the provider watchers both announce through this.
pub fn announcement(
    marks: &mut HashMap<String, Mark>,
    key_prefix: &str,
    windows: &[(String, Option<UsageWindow>)],
    skip90: bool,
) -> Option<Fired> {
    let mut fired: Option<Fired> = None;
    for (name, window) in windows {
        let Some(window) = window else { continue };
        let key = format!("{key_prefix}|{name}");
        let (mark, fire) = step(
            marks.get(&key).copied().unwrap_or_default(),
            window.percent,
            window.resets_at,
            skip90,
        );
        marks.insert(key, mark);
        let Some(level) = fire else { continue };
        if level >= fired.as_ref().map_or(0, |f| f.level) {
            fired = Some(Fired {
                level,
                name: name.clone(),
                window: *window,
            });
        }
    }
    fired
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SwitchDecision {
    /// Nothing pushes toward a switch, or one happened too recently.
    Stay,
    /// A saved account has room, but the provider is working and a switch would interrupt it.
    Busy,
    NoRoom,
    SwitchTo(String),
}

/// Whether the account in use should be switched, and to which. `can_switch_now` runs only once a
/// target exists (it may be a process listing).
#[allow(clippy::too_many_arguments)]
pub fn decide_switch(
    windows: &[Named],
    switch_at: i64,
    active_id: Option<&str>,
    candidates: &[Candidate],
    blocked_until: &HashMap<String, f64>,
    last_switch: Option<f64>,
    now: f64,
    can_switch_now: &mut dyn FnMut() -> bool,
) -> SwitchDecision {
    if !windows.iter().any(|(_, w)| w.percent >= switch_at as f64) || !may_switch(last_switch, now)
    {
        return SwitchDecision::Stay;
    }
    let Some(target) = pick_target(candidates, active_id, now, blocked_until) else {
        return SwitchDecision::NoRoom;
    };
    if can_switch_now() {
        SwitchDecision::SwitchTo(target.id.clone())
    } else {
        SwitchDecision::Busy
    }
}

/// Codex: a switch only runs while nothing would be interrupted: no CLI or Chat turn (`codex`
/// processes), no ChatGPT app to quit. The managed app server alone doesn't count; it restarts
/// quietly.
pub fn codex_idle_for_auto_switch(app_running: bool, sessions: usize) -> bool {
    !app_running && sessions == 0
}

/// The pool a lane picks its next account from: the account the main login holds is live there,
/// so it can't be taken.
pub fn lane_pool(mut pool: Vec<Candidate>, main_active: Option<&str>) -> Vec<Candidate> {
    for c in &mut pool {
        if Some(c.id.as_str()) == main_active {
            c.needs_login = true;
        }
    }
    pool
}
