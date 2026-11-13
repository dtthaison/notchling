use super::report::{Bar, BarKind, UsageReport};
use super::usage::UsageWindow;
use std::cmp::Ordering;
use std::collections::HashMap;

pub const SWITCH_RANGE: (i64, i64) = (80, 99);
pub const DEFAULT_SWITCH_AT: i64 = 95;
pub const FRESH_FOR: f64 = 10.0 * 60.0;
pub const IN_USE_FRESH_FOR: f64 = 15.0 * 60.0;
pub const ROOM_BELOW: f64 = 80.0;
pub const SWITCH_GAP: f64 = 30.0 * 60.0;
pub const DEFAULT_BLOCK: f64 = 5.0 * 3600.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mark {
    pub level: u32,
    pub resets_at: Option<f64>,
}

pub fn level(percent: f64) -> u32 {
    if percent >= 100.0 {
        100
    } else if percent >= 90.0 {
        90
    } else {
        0
    }
}

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

pub fn may_switch(last_switch: Option<f64>, now: f64) -> bool {
    last_switch.is_none_or(|last| now - last >= SWITCH_GAP)
}

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

pub type Named = (String, UsageWindow);

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

#[derive(Clone, Debug, PartialEq)]
pub struct Fired {
    pub level: u32,
    pub name: String,
    pub window: UsageWindow,
}

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
    Stay,
    Busy,
    NoRoom,
    SwitchTo(String),
}

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

pub fn codex_idle_for_auto_switch(app_running: bool, sessions: usize) -> bool {
    !app_running && sessions == 0
}

pub fn lane_pool(mut pool: Vec<Candidate>, main_active: Option<&str>) -> Vec<Candidate> {
    for c in &mut pool {
        if Some(c.id.as_str()) == main_active {
            c.needs_login = true;
        }
    }
    pool
}
