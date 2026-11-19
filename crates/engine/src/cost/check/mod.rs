mod cache;
mod data;
mod findings;
mod shares;

use crate::catalog::Catalog;
use crate::cost::ledger::Call;
use data::Data;
use serde::Serialize;
use std::collections::BTreeMap;

pub use data::{ttl_of, Tally};

pub const DEFAULT_DAYS: u32 = 14;
pub const MIN_REBUILD: u64 = 50_000;
pub const ADVICE: f64 = 0.5;
pub const TURN_FACTOR: f64 = 1.4;
pub const RUN_ADOPTION: f64 = 0.6;
pub const LONG_RUN: usize = 100;
pub const HANDOFF: f64 = 15_000.0;
pub const BIG_CONTEXT: u64 = 150_000;
pub const BUILDER_CAP: usize = 5;
pub const CAP_FACTOR: f64 = 0.05;
pub const SHOW_SHARE: f64 = 0.01;
pub const SHOW_FLOOR: f64 = 5.0;
pub const BUCKET: f64 = 600.0;
pub const SWITCH_WINDOW: f64 = 300.0;
pub const LOW_EFFORT_SHARE: f64 = 0.7;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Plan {
    Api,
    Subscription,
    #[default]
    Unknown,
}

impl Plan {
    pub fn parse(text: &str) -> Option<Plan> {
        match text {
            "api" => Some(Plan::Api),
            "subscription" => Some(Plan::Subscription),
            "unknown" | "auto" => Some(Plan::Unknown),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Ttl {
    #[serde(rename = "5m")]
    FiveMin,
    #[serde(rename = "1h")]
    OneHour,
    #[serde(rename = "mixed")]
    Mixed,
    #[serde(rename = "unknown")]
    Unknown,
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub days: u32,
    pub now: f64,
    pub plan: Plan,
}

impl Options {
    pub fn new(now: f64) -> Options {
        Options {
            days: DEFAULT_DAYS,
            now,
            plan: Plan::Unknown,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Ev {
    pub name: &'static str,
    pub value: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Finding {
    pub id: &'static str,
    pub saving: Option<f64>,
    pub saving_share: Option<f64>,
    pub tokens_avoided: Option<u64>,
    pub evidence: Vec<Ev>,
    pub detail: BTreeMap<String, String>,
}

impl Finding {
    pub fn new(id: &'static str) -> Finding {
        Finding {
            id,
            saving: None,
            saving_share: None,
            tokens_avoided: None,
            evidence: Vec::new(),
            detail: BTreeMap::new(),
        }
    }

    pub fn ev(&mut self, name: &'static str, value: f64) {
        self.evidence.push(Ev { name, value });
    }

    pub fn count(&self, name: &str) -> f64 {
        self.evidence
            .iter()
            .find(|e| e.name == name)
            .map_or(0.0, |e| e.value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Totals {
    pub calls: u64,
    pub tokens: u64,
    pub cost: Option<f64>,
    pub unpriced_calls: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Share {
    pub key: String,
    pub calls: u64,
    pub tokens: u64,
    pub cost: Option<f64>,
    pub cost_share: Option<f64>,
    pub token_share: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TtlCounts {
    pub five_min: u64,
    pub one_hour: u64,
    pub mixed: u64,
    pub unknown: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct CacheTimes {
    pub main: TtlCounts,
    pub sub: TtlCounts,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Unknown {
    pub id: &'static str,
    pub count: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Report {
    pub days: u32,
    pub from: f64,
    pub to: f64,
    pub plan: Plan,
    pub money: &'static str,
    pub totals: Totals,
    pub by_role: Vec<Share>,
    pub by_model: Vec<Share>,
    pub cache: CacheTimes,
    pub findings: Vec<Finding>,
    pub not_worth: Vec<Finding>,
    pub unknown: Vec<Unknown>,
    pub saving: Option<f64>,
    pub saving_share: Option<f64>,
}

fn sort_findings(list: &mut [Finding]) {
    list.sort_by(|a, b| {
        let key = |f: &Finding| f.saving.unwrap_or(-1.0);
        key(b).total_cmp(&key(a)).then(a.id.cmp(b.id))
    });
}

fn shown(finding: &Finding, total: Option<f64>) -> bool {
    match (finding.saving, total) {
        (Some(saving), Some(total)) => saving >= SHOW_FLOOR || saving >= SHOW_SHARE * total,
        (Some(saving), None) => saving >= SHOW_FLOOR,
        (None, _) => finding.evidence.iter().any(|e| e.value > 0.0),
    }
}

pub fn analyse(window: &[Call], catalog: &Catalog, options: &Options) -> Report {
    let from = options.now - f64::from(options.days) * 86_400.0;
    let inside: Vec<Call> = window
        .iter()
        .filter(|c| c.ts >= from && c.ts <= options.now)
        .cloned()
        .collect();
    let mut data = Data::new(&inside, catalog);
    let mut all = vec![
        findings::premium_subagents(&mut data),
        findings::repeated_reviews(&mut data),
        findings::cold_resumes(&mut data),
        findings::model_switches(&mut data),
        findings::long_runs(&mut data),
        findings::many_builders(&mut data),
    ];
    let total = data.total_cost;
    for finding in &mut all {
        finding.saving_share = finding
            .saving
            .zip(total)
            .filter(|(_, t)| *t > 0.0)
            .map(|(s, t)| s / t);
    }
    sort_findings(&mut all);
    all.retain(|f| f.evidence.iter().any(|e| e.value > 0.0));
    let (findings, mut not_worth): (Vec<Finding>, Vec<Finding>) =
        all.into_iter().partition(|f| shown(f, total));
    let (effort, effort_unknown) = findings::main_effort(&data);
    not_worth.extend(effort);
    sort_findings(&mut not_worth);
    let saving = findings
        .iter()
        .filter_map(|f| f.saving)
        .fold(None, |acc: Option<f64>, s| Some(acc.unwrap_or(0.0) + s));
    let saving_share = saving
        .zip(total)
        .filter(|(_, t)| *t > 0.0)
        .map(|(s, t)| s / t);
    let mut unknown = shares::unknown(&data);
    if effort_unknown > 0 {
        unknown.push(Unknown {
            id: "effort",
            count: effort_unknown,
        });
    }
    Report {
        days: options.days,
        from,
        to: options.now,
        plan: options.plan,
        money: match options.plan {
            Plan::Api => "estimate",
            _ => "api_equivalent",
        },
        totals: shares::totals(&data),
        by_role: shares::by_role(&data),
        by_model: shares::by_model(&data),
        cache: shares::cache_times(&data),
        findings,
        not_worth,
        unknown,
        saving,
        saving_share,
    }
}
