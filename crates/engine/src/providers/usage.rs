//! What every provider reports about its limits, whatever its API calls them: groups of bars, each
//! bar a used percentage with a reset time and a kind (`UsageReport.swift`). Cards and menu rows
//! read this, never a provider's own type.
//!
//! Times are seconds since 1970 (the rest of the API's convention); `None` when unknown.

use serde_json::{json, Value};

/// A bar's meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The short window: Claude's 5 hours, Codex's and Antigravity's "5h".
    Session,
    /// A weekly window, account-wide or for one model.
    Week,
    /// Anything else (a month, a limit the app doesn't know yet).
    Other,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Session => "session",
            Kind::Week => "week",
            Kind::Other => "other",
        }
    }

    /// "5h" is the short window, "Week" the weekly one, anything else is other.
    pub fn of_label(label: &str) -> Kind {
        match label {
            "5h" => Kind::Session,
            "Week" => Kind::Week,
            _ => Kind::Other,
        }
    }
}

/// How much of a window is used and when it resets (`UsageWindow`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UsageWindow {
    pub percent: f64,
    pub resets_at: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub label: String,
    pub kind: Kind,
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

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// Shown over the bars; `None` for a provider with one unnamed set (Claude).
    pub name: Option<String>,
    pub bars: Vec<Bar>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UsageReport {
    pub groups: Vec<Group>,
    pub fetched: f64,
}

impl UsageReport {
    pub fn bars(&self) -> Vec<&Bar> {
        self.groups.iter().flat_map(|g| g.bars.iter()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.groups.iter().all(|g| g.bars.is_empty())
    }

    pub fn to_json(&self) -> Value {
        json!({
            "fetched": self.fetched,
            "groups": self.groups.iter().map(|g| json!({
                "name": g.name,
                "bars": g.bars.iter().map(|b| json!({
                    "label": b.label,
                    "kind": b.kind.as_str(),
                    "percent": b.percent,
                    "resets_at": b.resets_at,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }
}

/// A named set of bars (Codex's "Codex" and "Code Review", Antigravity's model groups), each bar a
/// label and a window (`GroupedUsage`).
#[derive(Clone, Debug, PartialEq)]
pub struct GroupedUsage {
    pub groups: Vec<GroupedGroup>,
    pub fetched: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GroupedGroup {
    pub name: String,
    pub bars: Vec<(String, UsageWindow)>,
}

impl From<&GroupedUsage> for UsageReport {
    /// Each bar is classified by its label ("5h", "Week", anything else).
    fn from(usage: &GroupedUsage) -> UsageReport {
        UsageReport {
            groups: usage
                .groups
                .iter()
                .map(|g| Group {
                    name: Some(g.name.clone()),
                    bars: g
                        .bars
                        .iter()
                        .map(|(label, w)| Bar {
                            label: label.clone(),
                            kind: Kind::of_label(label),
                            percent: w.percent,
                            resets_at: w.resets_at,
                        })
                        .collect(),
                })
                .collect(),
            fetched: usage.fetched,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESET: f64 = 1_800_000_000.0;

    fn window(percent: f64) -> UsageWindow {
        UsageWindow {
            percent,
            resets_at: Some(RESET),
        }
    }

    #[test]
    fn grouped_usage_keeps_groups_and_classifies_bars_by_label() {
        let grouped = GroupedUsage {
            groups: vec![
                GroupedGroup {
                    name: "Codex".into(),
                    bars: vec![("5h".into(), window(25.0)), ("Week".into(), window(25.0))],
                },
                GroupedGroup {
                    name: "Code Review".into(),
                    bars: vec![
                        ("Month".into(), window(25.0)),
                        ("3h".into(), window(25.0)),
                        ("Limit".into(), window(25.0)),
                    ],
                },
            ],
            fetched: 7.0,
        };
        let report = UsageReport::from(&grouped);
        let names: Vec<_> = report.groups.iter().map(|g| g.name.clone()).collect();
        assert_eq!(
            names,
            vec![Some("Codex".to_string()), Some("Code Review".to_string())]
        );
        let kinds = |i: usize| {
            report.groups[i]
                .bars
                .iter()
                .map(|b| b.kind)
                .collect::<Vec<_>>()
        };
        assert_eq!(kinds(0), vec![Kind::Session, Kind::Week]);
        assert_eq!(kinds(1), vec![Kind::Other; 3]);
        let labels: Vec<_> = report.groups[1]
            .bars
            .iter()
            .map(|b| b.label.as_str())
            .collect();
        assert_eq!(labels, vec!["Month", "3h", "Limit"]);
        assert_eq!(report.bars()[0].window(), window(25.0));
        assert_eq!(report.fetched, 7.0);
        assert!(!report.is_empty());
    }

    #[test]
    fn an_empty_report_is_empty_and_json_names_the_kinds() {
        let empty = UsageReport {
            groups: vec![Group {
                name: None,
                bars: vec![],
            }],
            fetched: 0.0,
        };
        assert!(empty.is_empty());
        let one = UsageReport::from(&GroupedUsage {
            groups: vec![GroupedGroup {
                name: "G".into(),
                bars: vec![("Week".into(), window(9.0))],
            }],
            fetched: 1.0,
        });
        let json = one.to_json();
        assert_eq!(json["groups"][0]["bars"][0]["kind"], "week");
        assert_eq!(json["groups"][0]["bars"][0]["percent"], 9.0);
        assert_eq!(json["groups"][0]["name"], "G");
    }
}
