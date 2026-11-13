use notchling_engine::limits::report::{BarKind, UsageReport};
use notchling_engine::pages::clock::{civil_at, local_offset_secs};

use super::wire::Window;

#[derive(Clone, Copy)]
pub struct Zone(pub fn(f64) -> i64);

impl Zone {
    pub const LOCAL: Zone = Zone(local_offset_secs);
    pub const UTC: Zone = Zone(|_| 0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    Fine,
    Warm,
    Hot,
}

impl Fill {
    pub fn of(percent: f64) -> Fill {
        if percent >= 90.0 {
            Fill::Hot
        } else if percent >= 70.0 {
            Fill::Warm
        } else {
            Fill::Fine
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LimitLine {
    pub label: String,
    pub percent: f64,
    pub percent_text: String,
    pub resets: Option<String>,
    pub fill: Fill,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LimitGroup {
    pub name: Option<String>,
    pub lines: Vec<LimitLine>,
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

pub fn reset_text(resets_at: f64, now: f64, short: bool, zone: Zone) -> String {
    let left = resets_at - now;
    if left <= 0.0 {
        return "resetting".to_owned();
    }
    if short || left < 6.0 * 3600.0 {
        let minutes = ((left / 60.0).ceil() as i64).max(1);
        return if minutes >= 60 {
            format!("in {}h {}m", minutes / 60, minutes % 60)
        } else {
            format!("in {minutes}m")
        };
    }
    let offset = (zone.0)(resets_at);
    let c = civil_at(resets_at, offset);
    let days = (resets_at.floor() as i64 + offset).div_euclid(86_400);
    let weekday = (days + 4).rem_euclid(7) as usize;
    format!("{} {}:{:02}", WEEKDAYS[weekday], c.hour, c.minute)
}

fn line(
    label: &str,
    percent: f64,
    resets_at: Option<f64>,
    short: bool,
    now: f64,
    zone: Zone,
) -> LimitLine {
    let percent = percent.clamp(0.0, 100.0);
    LimitLine {
        label: label.to_owned(),
        percent,
        percent_text: format!("{}%", percent.round() as i64),
        resets: resets_at.map(|at| format!("resets {}", reset_text(at, now, short, zone))),
        fill: Fill::of(percent),
    }
}

fn window_label(label: &str, kind: BarKind) -> String {
    match kind {
        BarKind::Session => "5 hours".to_owned(),
        _ => label.to_owned(),
    }
}

pub fn groups(report: &UsageReport, now: f64, zone: Zone) -> Vec<LimitGroup> {
    report
        .groups
        .iter()
        .map(|g| LimitGroup {
            name: g.name.clone(),
            lines: g
                .bars
                .iter()
                .map(|b| {
                    line(
                        &window_label(&b.label, b.kind),
                        b.percent,
                        b.resets_at,
                        b.kind == BarKind::Session,
                        now,
                        zone,
                    )
                })
                .collect(),
        })
        .collect()
}

pub fn from_windows(windows: &[Window], now: f64, zone: Zone) -> Vec<LimitLine> {
    windows
        .iter()
        .map(|w| {
            let lower = w.name.to_ascii_lowercase();
            let short = lower.starts_with("5-hour") || lower == "5h" || lower == "session";
            let label = if short {
                "5 hours"
            } else if lower.starts_with("week") {
                "Week"
            } else {
                &w.name
            };
            line(label, w.percent, w.resets_at, short, now, zone)
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Meter {
    pub label: String,
    pub percent_text: String,
    pub percent: f64,
    pub resets: Option<String>,
    pub fill: Fill,
    pub tooltip: String,
}

fn meter(
    label: &str,
    name: &str,
    percent: f64,
    resets_at: Option<f64>,
    now: f64,
    zone: Zone,
) -> Meter {
    let percent = percent.clamp(0.0, 100.0);
    let short = label.ends_with('h');
    let resets = resets_at.map(|at| reset_text(at, now, short, zone));
    let percent_text = format!("{}%", percent.round() as i64);
    let tooltip = match &resets {
        Some(r) => format!("{name}: {percent_text} used, resets {r}"),
        None => format!("{name}: {percent_text} used"),
    };
    Meter {
        label: label.to_owned(),
        percent_text,
        percent,
        resets,
        fill: Fill::of(percent),
        tooltip,
    }
}

pub fn claude_meters(windows: &[Window], now: f64, zone: Zone) -> Vec<Meter> {
    windows
        .iter()
        .filter_map(|w| {
            let lower = w.name.to_ascii_lowercase();
            if lower.starts_with("5-hour") || lower == "5h" || lower == "session" {
                Some(meter(
                    "5h",
                    "5-hour limit",
                    w.percent,
                    w.resets_at,
                    now,
                    zone,
                ))
            } else if lower.starts_with("week") {
                Some(meter(
                    "Week",
                    "Weekly limit",
                    w.percent,
                    w.resets_at,
                    now,
                    zone,
                ))
            } else {
                None
            }
        })
        .collect()
}

pub fn codex_meters(report: &UsageReport, now: f64, zone: Zone) -> Vec<Meter> {
    let group = report
        .groups
        .iter()
        .find(|g| g.name.as_deref() == Some("Codex"))
        .or_else(|| report.groups.first());
    group
        .map(|g| {
            g.bars
                .iter()
                .map(|b| {
                    let name = match b.kind {
                        BarKind::Session => "5-hour limit".to_owned(),
                        BarKind::Week => "Weekly limit".to_owned(),
                        BarKind::Other => format!("{} limit", b.label),
                    };
                    meter(&b.label, &name, b.percent, b.resets_at, now, zone)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::limits::report::{Bar, Group};

    const NOW: f64 = 1_800_000_000.0;

    #[test]
    fn reset_text_follows_the_swift_rules() {
        assert_eq!(reset_text(NOW - 1.0, NOW, false, Zone::UTC), "resetting");
        assert_eq!(reset_text(NOW + 30.0, NOW, false, Zone::UTC), "in 1m");
        assert_eq!(
            reset_text(NOW + 2.0 * 3600.0 + 13.0 * 60.0, NOW, false, Zone::UTC),
            "in 2h 13m"
        );
        let far = NOW + 2.0 * 86_400.0;
        let text = reset_text(far, NOW, false, Zone::UTC);
        assert!(WEEKDAYS.iter().any(|d| text.starts_with(d)), "{text}");
        assert!(text.contains(':'), "{text}");
        assert_eq!(reset_text(far, NOW, true, Zone::UTC), "in 48h 0m");
    }

    #[test]
    fn fills_and_labels_follow_the_percent_and_kind() {
        assert_eq!(Fill::of(69.9), Fill::Fine);
        assert_eq!(Fill::of(70.0), Fill::Warm);
        assert_eq!(Fill::of(90.0), Fill::Hot);
        let report = UsageReport {
            groups: vec![Group {
                name: Some("Codex".into()),
                bars: vec![
                    Bar {
                        label: "5h".into(),
                        kind: BarKind::Session,
                        percent: 41.4,
                        resets_at: Some(NOW + 600.0),
                    },
                    Bar {
                        label: "Week".into(),
                        kind: BarKind::Week,
                        percent: 92.0,
                        resets_at: None,
                    },
                ],
            }],
            fetched: NOW,
        };
        let groups = groups(&report, NOW, Zone::UTC);
        assert_eq!(groups[0].name.as_deref(), Some("Codex"));
        let l = &groups[0].lines;
        assert_eq!(
            (l[0].label.as_str(), l[0].percent_text.as_str()),
            ("5 hours", "41%")
        );
        assert_eq!(l[0].resets.as_deref(), Some("resets in 10m"));
        assert_eq!((l[1].label.as_str(), l[1].fill), ("Week", Fill::Hot));
        assert_eq!(l[1].resets, None);
        let meters = codex_meters(&report, NOW, Zone::UTC);
        assert_eq!(meters[0].label, "5h");
        assert_eq!(meters[0].tooltip, "5-hour limit: 41% used, resets in 10m");
        assert_eq!(meters[1].tooltip, "Weekly limit: 92% used");
    }

    #[test]
    fn the_watchers_windows_make_claude_lines_and_meters() {
        let windows = vec![
            Window {
                name: "5-hour".into(),
                percent: 41.0,
                resets_at: Some(NOW + 3600.0),
            },
            Window {
                name: "Weekly".into(),
                percent: 12.0,
                resets_at: None,
            },
            Window {
                name: "Opus".into(),
                percent: 140.0,
                resets_at: None,
            },
        ];
        let lines = from_windows(&windows, NOW, Zone::UTC);
        assert_eq!(lines[0].label, "5 hours");
        assert_eq!(lines[0].resets.as_deref(), Some("resets in 1h 0m"));
        assert_eq!(lines[1].label, "Week");
        assert_eq!((lines[2].label.as_str(), lines[2].percent), ("Opus", 100.0));
        let meters = claude_meters(&windows, NOW, Zone::UTC);
        assert_eq!(meters.len(), 2, "the header shows only 5h and Week");
        assert_eq!(meters[0].resets.as_deref(), Some("in 1h 0m"));
        assert_eq!(meters[1].label, "Week");
    }
}
