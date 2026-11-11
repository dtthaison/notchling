//! The summaries over the cache (`CostTracker.summarize`): per window (7 days, 30 days, all time),
//! for every account together or for one alone.

use super::cache::Cache;
use crate::catalog::{Catalog, Tool};
use crate::pages::clock::civil_at;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// The three ranges the Token Cost view can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Window {
    Days(u32),
    All,
}

impl Window {
    pub const ALL: [Window; 3] = [Window::Days(7), Window::Days(30), Window::All];

    /// "7", "30" or "all" (a JSON number 7 or 30 reads the same).
    pub fn parse(text: &str) -> Option<Window> {
        match text {
            "7" => Some(Window::Days(7)),
            "30" => Some(Window::Days(30)),
            "all" => Some(Window::All),
            _ => None,
        }
    }

    pub fn name(self) -> String {
        match self {
            Window::Days(n) => n.to_string(),
            Window::All => "all".to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DayCost {
    pub day: String,
    pub cost: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NameCost {
    pub name: String,
    pub cost: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountCost {
    pub id: String,
    pub cost: f64,
}

/// One model's share of the cost and its token totals over the window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelStat {
    pub name: String,
    pub cost: f64,
    pub percent: f64,
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    /// The last 7 or 30 days (30 for all time), oldest first.
    pub days: Vec<DayCost>,
    pub today: f64,
    pub week: f64,
    /// The window's total.
    pub month: f64,
    /// Most expensive first.
    pub models: Vec<NameCost>,
    pub model_detail: Vec<ModelStat>,
    pub projects: Vec<NameCost>,
    /// By account id; "" is cost from before accounts were tracked.
    pub accounts: Vec<AccountCost>,
    /// By channel (Claude Code, Desktop, Codex, Antigravity CLI).
    pub channels: Vec<NameCost>,
    /// All tokens (input, output, cache) per channel, so a channel whose models have no known
    /// price (cost 0) still shows what it used.
    pub channel_tokens: BTreeMap<String, f64>,
    /// Model ids counted in the window with no price: their tokens are in, their dollars are not.
    /// Sorted, once each. Absent from an older daemon's reply, which the app still reads.
    #[serde(default)]
    pub unpriced: Vec<String>,
    /// Model ids in the window priced as another, older model's (they are newer than any the
    /// catalog knows). Sorted, once each.
    #[serde(default)]
    pub borrowed: Vec<String>,
}

/// "yyyy-MM-dd" for a day number (days since 1970-01-01 in the zone the days are counted in).
pub fn day_key(day: i64) -> String {
    let c = civil_at(day as f64 * 86_400.0, 0);
    format!("{:04}-{:02}-{:02}", c.year, c.month, c.day)
}

/// "account\tname", the key of the cost tables.
pub fn key(account: &str, name: &str) -> String {
    format!("{account}\t{name}")
}

pub fn account_of(key: &str) -> &str {
    key.split('\t').next().unwrap_or("")
}

pub fn name_of(key: &str) -> &str {
    key.split_once('\t').map_or(key, |(_, name)| name)
}

fn by_value_then_name(a: &(String, f64), b: &(String, f64)) -> Ordering {
    b.1.partial_cmp(&a.1)
        .unwrap_or(Ordering::Equal)
        .then_with(|| a.0.cmp(&b.0))
}

/// Every account that has cost, for the per-account summaries.
pub fn owners(cache: &Cache) -> BTreeSet<String> {
    cache
        .models
        .values()
        .flat_map(|m| m.keys().map(|k| account_of(k).to_string()))
        .collect()
}

/// Totals over `window`, for one account or all of them, as of `today` (a day number). `today` and
/// `week` stay the same across windows; `month` is the window's total and the bars and breakdowns
/// cover the window. `catalog` says which of the models counted had their price borrowed.
pub fn summarize(
    cache: &Cache,
    account: Option<&str>,
    window: Window,
    today: i64,
    catalog: &Catalog,
) -> Summary {
    let included = |key: &str| account.is_none_or(|a| account_of(key) == a);
    let total = |day: &str| -> f64 {
        cache.models.get(day).map_or(0.0, |m| {
            m.iter().filter(|(k, _)| included(k)).map(|(_, v)| v).sum()
        })
    };
    let window_days: BTreeSet<String> = match window {
        Window::All => cache.models.keys().cloned().collect(),
        Window::Days(n) => (0..i64::from(n)).map(|i| day_key(today - i)).collect(),
    };
    let bars = match window {
        Window::All => 30,
        Window::Days(n) => i64::from(n).min(30),
    };
    let mut s = Summary {
        days: (0..bars)
            .rev()
            .map(|i| {
                let day = day_key(today - i);
                DayCost {
                    cost: total(&day),
                    day,
                }
            })
            .collect(),
        today: total(&day_key(today)),
        week: (0..7).map(|i| total(&day_key(today - i))).sum(),
        month: window_days.iter().map(|d| total(d)).sum(),
        ..Summary::default()
    };

    let ranked = |table: &BTreeMap<String, BTreeMap<String, f64>>,
                  name: &dyn Fn(&str) -> String|
     -> Vec<(String, f64)> {
        let mut sums: BTreeMap<String, f64> = BTreeMap::new();
        for day in &window_days {
            for (k, v) in table.get(day).into_iter().flatten() {
                if included(k) {
                    *sums.entry(name(k)).or_default() += v;
                }
            }
        }
        let mut out: Vec<_> = sums.into_iter().filter(|(_, v)| *v >= 0.01).collect();
        out.sort_by(by_value_then_name);
        out
    };
    let named = |rows: Vec<(String, f64)>| {
        rows.into_iter()
            .map(|(name, cost)| NameCost { name, cost })
            .collect::<Vec<_>>()
    };
    s.models = named(ranked(&cache.models, &|k| {
        super::prices::display_name(name_of(k))
    }));
    s.projects = named(ranked(&cache.projects, &|k| name_of(k).to_string()));
    s.accounts = ranked(&cache.models, &|k| account_of(k).to_string())
        .into_iter()
        .map(|(id, cost)| AccountCost { id, cost })
        .collect();

    let mut channel_cost: BTreeMap<String, f64> = BTreeMap::new();
    let mut channel_tokens: BTreeMap<String, f64> = BTreeMap::new();
    for day in &window_days {
        for (k, v) in cache.channels.get(day).into_iter().flatten() {
            if included(k) {
                *channel_cost.entry(name_of(k).to_string()).or_default() += v;
            }
        }
        for (k, v) in cache.channel_tokens.get(day).into_iter().flatten() {
            if included(k) {
                *channel_tokens.entry(name_of(k).to_string()).or_default() += v;
            }
        }
    }
    // A channel with no priced model has cost 0 but tokens, and still belongs in the list.
    let mut channels: Vec<_> = channel_cost
        .into_iter()
        .filter(|(name, v)| *v >= 0.01 || channel_tokens.get(name).copied().unwrap_or(0.0) > 0.0)
        .collect();
    channels.sort_by(|a, b| {
        let ta = channel_tokens.get(&a.0).copied().unwrap_or(0.0);
        let tb = channel_tokens.get(&b.0).copied().unwrap_or(0.0);
        (b.1, tb)
            .partial_cmp(&(a.1, ta))
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    s.channels = named(channels);
    s.channel_tokens = channel_tokens;

    // Model detail: cost share plus token totals, keyed by the model id.
    let mut cost_by_model: BTreeMap<String, f64> = BTreeMap::new();
    let mut tokens_by_model: BTreeMap<String, [f64; 4]> = BTreeMap::new();
    for day in &window_days {
        for (k, v) in cache.models.get(day).into_iter().flatten() {
            if included(k) {
                *cost_by_model.entry(name_of(k).to_string()).or_default() += v;
            }
        }
        for (k, v) in cache.tokens.get(day).into_iter().flatten() {
            if included(k) {
                let cur = tokens_by_model.entry(name_of(k).to_string()).or_default();
                for i in 0..4 {
                    cur[i] += v[i];
                }
            }
        }
    }
    let token_sum =
        |model: &str| -> f64 { tokens_by_model.get(model).map_or(0.0, |t| t.iter().sum()) };
    // A model with no known price has cost 0 but tokens: list it by its tokens.
    let mut detail: Vec<_> = cost_by_model
        .iter()
        .filter(|(m, v)| **v >= 0.01 || (**v == 0.0 && token_sum(m) > 0.0))
        .collect();
    detail.sort_by(|a, b| {
        (*b.1, token_sum(b.0))
            .partial_cmp(&(*a.1, token_sum(a.0)))
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.0.cmp(b.0))
    });
    s.unpriced = window_days
        .iter()
        .filter_map(|day| cache.unpriced.get(day))
        .flatten()
        .filter(|k| included(k))
        .map(|k| name_of(k).to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    s.borrowed = tokens_by_model
        .keys()
        .filter(|m| catalog.price_borrowed(Tool::Claude, m))
        .cloned()
        .collect();
    s.model_detail = detail
        .into_iter()
        .map(|(model, cost)| {
            let t = tokens_by_model.get(model).copied().unwrap_or_default();
            ModelStat {
                name: super::prices::display_name(model),
                cost: *cost,
                percent: if s.month > 0.0 {
                    cost / s.month * 100.0
                } else {
                    0.0
                },
                input: t[0],
                output: t[1],
                cache_read: t[2],
                cache_write: t[3],
            }
        })
        .collect();
    s
}
