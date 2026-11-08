//! The small text rules Chat shows (`ChatFormat` in the Swift app).

use std::path::Path;

/// "now", "5m", "2h", "3d"; empty for an unknown time (0, the Swift app's `distantPast`).
pub fn age(now_ms: i64, since_ms: i64) -> String {
    if since_ms <= 0 {
        return String::new();
    }
    let s = (now_ms - since_ms).max(0) / 1000;
    if s < 60 {
        "now".to_owned()
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86_400 {
        format!("{}h", s / 3600)
    } else {
        format!("{}d", s / 86_400)
    }
}

/// "12s", "4m 05s", "1h 12m".
pub fn duration(seconds: f64) -> String {
    let s = seconds.round().max(0.0) as i64;
    if s < 60 {
        format!("{}s", s.max(1))
    } else if s < 3600 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{}h {}m", s / 3600, s % 3600 / 60)
    }
}

/// "claude-opus-5-5" -> "Opus 5.5"; "gpt-6-luna" -> "Gpt 6".
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

/// "12.4k", "1.2M", "400k", "950".
pub fn tokens(n: u64) -> String {
    fn short(v: f64, unit: &str) -> String {
        let mut s = format!("{v:.1}");
        if s.ends_with(".0") {
            s.truncate(s.len() - 2);
        }
        s + unit
    }
    if n >= 1_000_000 {
        short(n as f64 / 1_000_000.0, "M")
    } else if n >= 1000 {
        short(n as f64 / 1000.0, "k")
    } else {
        n.to_string()
    }
}

/// The model's context window in Claude Code, where it's known (Opus 5.5's /context says 400k).
/// Others give `None`, and the header shows the count alone rather than guess a percentage.
pub fn context_window(model: Option<&str>) -> Option<u64> {
    model
        .filter(|m| m.starts_with("claude-opus-5-5"))
        .map(|_| 400_000)
}

/// The header's context text: "148.8k / 400k · 37%" when the window is known, else "148.8k".
/// `None` when nothing has been counted yet.
pub fn context_text(context_tokens: u64, model: Option<&str>) -> Option<String> {
    if context_tokens == 0 {
        return None;
    }
    Some(match context_window(model) {
        Some(window) => format!(
            "{} / {} · {}%",
            tokens(context_tokens),
            tokens(window),
            (context_tokens * 100 / window).min(999)
        ),
        None => tokens(context_tokens),
    })
}

/// A folder with the home folder shortened to `~`.
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

    #[test]
    fn context_only_for_known_windows() {
        assert_eq!(context_window(Some("claude-opus-5-5")), Some(400_000));
        assert_eq!(context_window(Some("claude-sonnet-5-5")), None);
        assert_eq!(context_window(None), None);
        assert_eq!(
            context_text(148_800, Some("claude-opus-5-5")).as_deref(),
            Some("148.8k / 400k · 37%")
        );
        assert_eq!(
            context_text(148_800, Some("claude-sonnet-5-5")).as_deref(),
            Some("148.8k")
        );
        assert_eq!(context_text(0, Some("claude-opus-5-5")), None);
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
