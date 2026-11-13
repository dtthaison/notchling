use super::usage::{AccountUsage, UsageWindow};
use serde::{Deserialize, Serialize};

pub type GroupSpec = (Option<String>, Vec<(String, UsageWindow)>);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarKind {
    Session,
    Week,
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
    pub name: Option<String>,
    pub bars: Vec<Bar>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageReport {
    pub groups: Vec<Group>,
    pub fetched: f64,
}

impl UsageReport {
    pub fn bars(&self) -> impl Iterator<Item = &Bar> {
        self.groups.iter().flat_map(|g| g.bars.iter())
    }

    pub fn is_empty(&self) -> bool {
        self.bars().next().is_none()
    }

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

pub fn kind_of_label(label: &str) -> BarKind {
    match label {
        "5h" => BarKind::Session,
        "Week" => BarKind::Week,
        _ => BarKind::Other,
    }
}
