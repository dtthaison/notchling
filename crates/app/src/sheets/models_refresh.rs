use notchling_locale as t;
use serde_json::Value;

use super::skills_rows::SkillTool;

fn total(report: &Value, key: &str) -> usize {
    report
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|tool| tool.get(key).and_then(Value::as_array).map_or(0, Vec::len))
        .sum()
}

fn unread(report: &Value) -> Vec<String> {
    report
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|tool| tool.get("read").and_then(Value::as_bool) == Some(false))
        .filter_map(|tool| tool.get("tool").and_then(Value::as_str))
        .map(|id| SkillTool::of(id).map_or_else(|| id.to_owned(), |t| t.title().to_owned()))
        .collect()
}

pub fn summary(reply: Result<Value, String>) -> (String, bool) {
    let report = match reply {
        Ok(report) if report.get("tools").is_some() => report,
        Ok(_) => return (t::settings::reply_unreadable().to_owned(), true),
        Err(why) => return (t::settings::models_refresh_failed(&why), true),
    };
    let mut parts: Vec<String> = Vec::new();
    let counted = [
        (total(&report, "added"), 0),
        (total(&report, "removed"), 1),
        (total(&report, "efforts"), 2),
    ];
    for (count, which) in counted {
        if count == 0 {
            continue;
        }
        parts.push(match which {
            0 => t::settings::models_refresh_new(count),
            1 => t::settings::models_refresh_gone(count),
            _ => t::settings::models_refresh_efforts(count),
        });
    }
    let mut lines: Vec<String> = Vec::new();
    if parts.is_empty() {
        lines.push(t::settings::models_refreshed_none().to_owned());
    } else {
        lines.push(format!(
            "{} {}.",
            t::settings::models_refreshed(),
            parts.join(", ")
        ));
    }
    let unpriced = total(&report, "unpriced");
    if unpriced > 0 {
        lines.push(t::settings::models_refresh_unpriced(unpriced));
    }
    let picks = report
        .get("suggestions")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if picks > 0 {
        lines.push(t::settings::models_refresh_picks(picks));
    }
    let missed = unread(&report);
    if !missed.is_empty() {
        lines.push(t::settings::models_refresh_unread(&missed.join(", ")));
    }
    (lines.join(" "), false)
}

#[cfg(test)]
#[path = "models_refresh_tests.rs"]
mod tests;
