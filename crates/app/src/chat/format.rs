use std::path::Path;

use notchling_engine::catalog::ModelInfo;
use notchling_locale as t;

use super::claude_picker;

pub fn age(now_ms: i64, since_ms: i64) -> String {
    if since_ms <= 0 {
        return String::new();
    }
    let s = (now_ms - since_ms).max(0) / 1000;
    if s < 60 {
        t::units::age_now().to_owned()
    } else if s < 3600 {
        t::units::age_minutes(s / 60)
    } else if s < 86_400 {
        t::units::age_hours(s / 3600)
    } else {
        t::units::age_days(s / 86_400)
    }
}

pub fn duration(seconds: f64) -> String {
    let s = seconds.round().max(0.0) as i64;
    if s < 60 {
        t::units::duration_seconds(s.max(1))
    } else if s < 3600 {
        t::units::duration_minutes(s / 60, &format!("{:02}", s % 60))
    } else {
        t::units::duration_hours(s / 3600, s % 3600 / 60)
    }
}

pub fn model_name(id: &str) -> String {
    let stripped = id.replace("claude-", "");
    let parts: Vec<&str> = stripped.split('-').collect();
    let Some(family) = parts.first().filter(|f| !f.is_empty()) else {
        return id.to_owned();
    };
    let version: Vec<&str> = parts[1..]
        .iter()
        .take_while(|p| p.len() <= 2 && p.chars().all(|c| c.is_ascii_digit()))
        .copied()
        .collect();
    let mut name: String = family
        .chars()
        .enumerate()
        .map(|(i, c)| if i == 0 { c.to_ascii_uppercase() } else { c })
        .collect();
    if !version.is_empty() {
        name.push(' ');
        name.push_str(&version.join("."));
    }
    name
}

pub fn tokens(n: u64) -> String {
    fn short(v: f64, unit: fn(&str) -> String) -> String {
        let s = format!("{v:.1}");
        let value = match s.strip_suffix(".0") {
            Some(whole) => match whole.parse::<i64>() {
                Ok(n) if n < 1000 => t::fmt::count(n),
                _ => whole.to_owned(),
            },
            None => t::fmt::decimal(s.parse().unwrap_or(0.0), 1),
        };
        unit(&value)
    }
    if n >= 1_000_000 {
        short(n as f64 / 1_000_000.0, t::units::millions)
    } else if n >= 1000 {
        short(n as f64 / 1000.0, t::units::thousands)
    } else {
        t::fmt::count(n)
    }
}

pub fn context_window(model: Option<&str>, catalog: &[ModelInfo]) -> Option<u64> {
    claude_picker::find(catalog, model?)?.context_window
}

pub fn context_window_source(model: Option<&str>, catalog: &[ModelInfo]) -> Option<String> {
    let entry = claude_picker::find(catalog, model?)?;
    entry.context_window?;
    entry.borrowed_from.clone().filter(|_| entry.borrowed)
}

pub fn context_text(
    context_tokens: u64,
    model: Option<&str>,
    catalog: &[ModelInfo],
) -> Option<String> {
    if context_tokens == 0 {
        return None;
    }
    Some(match context_window(model, catalog) {
        Some(window) => format!(
            "{} / {} · {}%",
            tokens(context_tokens),
            tokens(window),
            (context_tokens * 100 / window).min(999)
        ),
        None => tokens(context_tokens),
    })
}

pub fn short_path(path: &str, home: &Path) -> String {
    let home = home.to_string_lossy();
    let home = home.trim_end_matches('/');
    if !home.is_empty() {
        if path == home {
            return "~".to_owned();
        }
        if let Some(rest) = path.strip_prefix(&format!("{home}/")) {
            return format!("~/{rest}");
        }
    }
    path.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages() {
        let now = 1_000_000_000_000;
        assert_eq!(age(now, 0), "");
        assert_eq!(age(now, now - 30_000), "now");
        assert_eq!(age(now, now - 5 * 60_000), "5m");
        assert_eq!(age(now, now - 2 * 3_600_000), "2h");
        assert_eq!(age(now, now - 3 * 86_400_000), "3d");
        assert_eq!(
            age(now, now + 60_000),
            "now",
            "a clock ahead of the file is 'now'"
        );
    }

    #[test]
    fn durations() {
        assert_eq!(duration(0.2), "1s");
        assert_eq!(duration(12.0), "12s");
        assert_eq!(duration(245.0), "4m 05s");
        assert_eq!(duration(4320.0), "1h 12m");
    }

    #[test]
    fn model_names() {
        assert_eq!(model_name("claude-opus-5-5"), "Opus 5.5");
        assert_eq!(model_name("claude-sonnet-5-5-20261001"), "Sonnet 5.5");
        assert_eq!(model_name("claude-haiku-5-5"), "Haiku 5.5");
        assert_eq!(model_name("gpt-6-luna"), "Gpt 6");
        assert_eq!(model_name(""), "");
    }

    #[test]
    fn token_counts() {
        assert_eq!(tokens(950), "950");
        assert_eq!(tokens(12_400), "12.4k");
        assert_eq!(tokens(400_000), "400k");
        assert_eq!(tokens(1_200_000), "1.2M");
    }

    fn catalog() -> Vec<ModelInfo> {
        use notchling_engine::catalog::{Catalog, Tool};
        Catalog::built_in().models(Tool::Claude, &[])
    }

    #[test]
    fn context_only_for_known_windows() {
        let c = catalog();
        assert_eq!(context_window(Some("claude-opus-5-5"), &c), Some(400_000));
        assert_eq!(context_window(Some("claude-sonnet-5-5"), &c), None);
        assert_eq!(context_window(None, &c), None);
        assert_eq!(
            context_text(148_800, Some("claude-opus-5-5"), &c).as_deref(),
            Some("148.8k / 400k · 37%")
        );
        assert_eq!(
            context_text(148_800, Some("claude-sonnet-5-5"), &c).as_deref(),
            Some("148.8k")
        );
        assert_eq!(context_text(0, Some("claude-opus-5-5"), &c), None);
    }

    #[test]
    fn the_window_sees_through_a_suffix_and_falls_back_to_the_built_in_catalog() {
        assert_eq!(
            context_window(Some("claude-opus-5-5[1m]"), &[]),
            Some(400_000)
        );
        assert_eq!(
            context_window(Some("claude-opus-5-5-20261001"), &catalog()),
            Some(400_000)
        );
    }

    #[test]
    fn home_becomes_tilde() {
        let home = Path::new("/Users/me");
        assert_eq!(short_path("/Users/me/work/x", home), "~/work/x");
        assert_eq!(short_path("/Users/me", home), "~");
        assert_eq!(short_path("/Users/mel/work", home), "/Users/mel/work");
        assert_eq!(short_path("/tmp/x", Path::new("")), "/tmp/x");
    }
}
