//! What every provider reports about its limits, whatever its API calls them: groups of bars, each
//! a used percentage with a reset time and a kind (`UsageReport` in Swift). The watchers read this,
//! never a provider's own type.
//!
//! Milestone 6's provider registry (`engine::providers`, task E8b) has its own `UsageReport`; the
//! daemon's wiring maps it onto this one (same fields), or replaces this by a re-export.

use super::usage::{AccountUsage, UsageWindow};
use serde::{Deserialize, Serialize};

/// A group's name and its `(label, window)` bars.
pub type GroupSpec = (Option<String>, Vec<(String, UsageWindow)>);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarKind {
    /// The short window: Claude's 5 hours, Codex's and Antigravity's "5h".
    Session,
    /// A weekly window, account-wide or for one model.
    Week,
    /// Anything else (a month, a limit the app doesn't know yet).
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bar {
    pub label: String,
    pub kind: BarKind,
    pub percent: f64,
    pub resets_at: Option<f64>,
}

impl Bar {
    pub fn window(&self) -> UsageWindow {
        UsageWindow {
            percent: self.percent,
            resets_at: self.resets_at,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    /// Shown over the bars; `None` for a provider with one unnamed set (Claude).
    pub name: Option<String>,
    pub bars: Vec<Bar>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageReport {
    pub groups: Vec<Group>,
    /// When it was read, seconds since 1970.
    pub fetched: f64,
}

impl UsageReport {
    pub fn bars(&self) -> impl Iterator<Item = &Bar> {
        self.groups.iter().flat_map(|g| g.bars.iter())
    }

    pub fn is_empty(&self) -> bool {
        self.bars().next().is_none()
    }

    /// Claude: one unnamed group, the 5-hour and week bars first, then the limits the app doesn't
    /// know (which stop the account like the two above), then the per-model weekly ones.
    pub fn from_account(usage: &AccountUsage) -> UsageReport {
        let bar = |label: &str, kind, w: &UsageWindow| Bar {
            label: label.to_string(),
            kind,
            percent: w.percent,
            resets_at: w.resets_at,
        };
        let mut bars = Vec::new();
        if let Some(s) = &usage.session {
            bars.push(bar("5 hours", BarKind::Session, s));
        }
        if let Some(w) = &usage.week {
            bars.push(bar("Week", BarKind::Week, w));
        }
        for o in &usage.other {
            bars.push(bar(&o.name, BarKind::Other, &o.window));
        }
        for m in &usage.models {
            bars.push(bar(&m.name, BarKind::Week, &m.window));
        }
        UsageReport {
            groups: vec![Group { name: None, bars }],
            fetched: usage.fetched,
        }
    }

    /// Codex and Antigravity: named groups of `(label, window)`, each bar's kind from its label.
    pub fn from_groups(groups: Vec<GroupSpec>, fetched: f64) -> UsageReport {
        UsageReport {
            groups: groups
                .into_iter()
                .map(|(name, bars)| Group {
                    name,
                    bars: bars
                        .into_iter()
                        .map(|(label, w)| Bar {
                            kind: kind_of_label(&label),
                            label,
                            percent: w.percent,
                            resets_at: w.resets_at,
                        })
                        .collect(),
                })
                .collect(),
            fetched,
        }
    }
}

/// "5h" is the short window, "Week" the weekly one, anything else is neither.
pub fn kind_of_label(label: &str) -> BarKind {
    match label {
        "5h" => BarKind::Session,
        "Week" => BarKind::Week,
        _ => BarKind::Other,
    }
}
