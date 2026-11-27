use crate::transcript::util::parse_date;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageWindow {
    pub percent: f64,
    pub resets_at: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedWindow {
    pub name: String,
    pub window: UsageWindow,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AccountUsage {
    pub session: Option<UsageWindow>,
    pub week: Option<UsageWindow>,
    pub models: Vec<NamedWindow>,
    pub other: Vec<NamedWindow>,
    pub fetched: f64,
}

impl AccountUsage {
    pub fn stopping_windows(&self) -> Vec<UsageWindow> {
        self.session
            .iter()
            .chain(self.week.iter())
            .copied()
            .chain(self.other.iter().map(|o| o.window))
            .collect()
    }
}

const WEEKLY_MODEL: &str = "seven_day_";

fn window_at(json: &Value, key: &str) -> Option<UsageWindow> {
    let w = json.get(key)?.as_object()?;
    let percent = w.get("utilization")?.as_f64()?;
    Some(UsageWindow {
        percent,
        resets_at: w
            .get("resets_at")
            .and_then(Value::as_str)
            .and_then(parse_date),
    })
}

pub fn parse_usage(json: &Value, fetched: f64) -> AccountUsage {
    let mut usage = AccountUsage {
        session: window_at(json, "five_hour"),
        week: window_at(json, "seven_day"),
        fetched,
        ..AccountUsage::default()
    };
    for (key, name) in [("seven_day_opus", "Opus"), ("seven_day_sonnet", "Sonnet")] {
        if let Some(window) = window_at(json, key) {
            usage.models.push(NamedWindow {
                name: name.to_string(),
                window,
            });
        }
    }
    let mut other_models: Vec<&String> = json
        .as_object()
        .into_iter()
        .flat_map(|m| m.keys())
        .filter(|k| {
            k.strip_prefix(WEEKLY_MODEL)
                .is_some_and(|rest| !rest.is_empty())
                && !matches!(k.as_str(), "seven_day_opus" | "seven_day_sonnet")
        })
        .collect();
    other_models.sort();
    for key in other_models {
        if let Some(window) = window_at(json, key) {
            let rest = key.strip_prefix(WEEKLY_MODEL).unwrap_or(key);
            upsert(
                &mut usage.models,
                capitalized(&rest.replace('_', " ")),
                window,
            );
        }
    }
    if let Some(limits) = json.get("limits").and_then(Value::as_array) {
        apply_limits(limits, &mut usage);
    }
    usage
}

fn capitalized(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut start = true;
    for c in text.chars() {
        if c.is_alphanumeric() {
            if start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            start = false;
        } else {
            out.push(c);
            start = true;
        }
    }
    out
}

fn upsert(list: &mut Vec<NamedWindow>, name: String, window: UsageWindow) {
    match list.iter_mut().find(|n| n.name == name) {
        Some(existing) => existing.window = window,
        None => list.push(NamedWindow { name, window }),
    }
}

pub fn apply_limits(limits: &[Value], usage: &mut AccountUsage) {
    for limit in limits {
        let Some(percent) = limit.get("percent").and_then(Value::as_f64) else {
            continue;
        };
        let window = UsageWindow {
            percent,
            resets_at: limit
                .get("resets_at")
                .and_then(Value::as_str)
                .and_then(parse_date),
        };
        let kind = limit.get("kind").and_then(Value::as_str).unwrap_or("");
        let group = limit.get("group").and_then(Value::as_str).unwrap_or("");
        let readable = capitalized(&kind.replace('_', " "));
        if let Some(scope) = limit.get("scope").and_then(Value::as_object) {
            let named = ["model", "surface"].iter().find_map(|k| {
                scope
                    .get(*k)?
                    .get("display_name")?
                    .as_str()
                    .map(str::to_string)
            });
            upsert(&mut usage.models, named.unwrap_or(readable), window);
        } else if group == "weekly" {
            if percent >= usage.week.map_or(-1.0, |w| w.percent) {
                usage.week = Some(window);
            }
        } else if group.contains("session")
            || kind.contains("session")
            || kind.contains("five_hour")
        {
            if percent >= usage.session.map_or(-1.0, |w| w.percent) {
                usage.session = Some(window);
            }
        } else {
            usage.other.push(NamedWindow {
                name: readable,
                window,
            });
        }
    }
}
