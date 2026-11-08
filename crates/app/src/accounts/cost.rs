//! The Token Cost page, pure (the Swift `CostPage`): three tiles, a range, the last days as bars,
//! and the breakdowns by account, channel, model and project, over `cost.summary`'s reply. Costs
//! are dollars at API list prices; nothing here reads a transcript.

use notchling_engine::cost::summary::Summary;
use notchling_engine::vault::expiry::day_number;

use super::wire::CostReply;

/// The footer under the page, the Swift app's words.
pub const FOOTER: &str = "What your tokens would cost at API prices, from Claude Code, Claude Desktop, Codex and Antigravity CLI transcripts. A Codex or Antigravity model without a known list price shows its tokens and no cost. Your plan isn't billed this way. Each message is counted once, so totals are lower than Claude Code's /stats, which counts a message again every time a conversation is compacted or resumed. Desktop only appears when you run Claude Code agents in it (normal Desktop chat has no local token record). Before Tracking is cost from before Notchling followed which account was in use.";

/// What the page says before the first scan finished.
pub const READING: &str = "Reading transcripts…";

/// The filter's first option.
pub const ALL_ACCOUNTS: &str = "All Accounts";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    Days7,
    Days30,
    All,
}

impl Range {
    pub const ALL: [Range; 3] = [Range::Days7, Range::Days30, Range::All];

    pub fn wire(self) -> &'static str {
        match self {
            Range::Days7 => "7",
            Range::Days30 => "30",
            Range::All => "all",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Range::Days7 => "Last 7 Days",
            Range::Days30 => "Last 30 Days",
            Range::All => "All Time",
        }
    }

    /// The third tile's title.
    pub fn label(self) -> &'static str {
        match self {
            Range::Days7 => "7 Days",
            Range::Days30 => "30 Days",
            Range::All => "All Time",
        }
    }
}

/// `$12.34`, `$123` from a hundred, `$1.2k` from a thousand (`CostChartView.dollars`).
pub fn dollars(x: f64) -> String {
    if x >= 1000.0 {
        format!("${:.1}k", x / 1000.0)
    } else if x >= 100.0 {
        format!("${}", x.round() as i64)
    } else {
        format!("${x:.2}")
    }
}

/// `390.7k`, `212.4m`, `54.7b` tokens.
pub fn tokens(n: f64) -> String {
    if n >= 1e9 {
        format!("{:.1}b", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.1}m", n / 1e6)
    } else if n >= 1e3 {
        format!("{:.1}k", n / 1e3)
    } else {
        format!("{}", n.round() as i64)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub title: &'static str,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DayBar {
    /// The weekday's letter.
    pub letter: &'static str,
    pub cost: f64,
    /// Of the tallest bar, 0 to 1.
    pub fraction: f64,
    /// "2026-11-08 · $1.50".
    pub tooltip: String,
    /// The last day is drawn in the accent.
    pub last: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BreakRow {
    pub name: String,
    pub value: String,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Breakdown {
    pub title: String,
    pub rows: Vec<BreakRow>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CostView {
    /// The filter's options: `("", "All Accounts")`, then each account with its name.
    pub accounts: Vec<(String, String)>,
    pub account: String,
    pub tiles: [Tile; 3],
    pub range: Range,
    pub reading: bool,
    pub bars_title: &'static str,
    pub bars: Vec<DayBar>,
    pub sections: Vec<Breakdown>,
    pub footer: &'static str,
}

const LETTERS: [&str; 7] = ["S", "M", "T", "W", "T", "F", "S"];

fn weekday_letter(day: &str) -> &'static str {
    day_number(day)
        .map(|d| LETTERS[(d + 4).rem_euclid(7) as usize])
        .unwrap_or("·")
}

fn bars(summary: &Summary, range: Range) -> Vec<DayBar> {
    let days = match range {
        Range::Days7 => 7,
        _ => 30,
    };
    let start = summary.days.len().saturating_sub(days);
    let shown = &summary.days[start..];
    let peak = shown.iter().map(|d| d.cost).fold(0.0, f64::max);
    shown
        .iter()
        .enumerate()
        .map(|(i, d)| DayBar {
            letter: weekday_letter(&d.day),
            cost: d.cost,
            fraction: if peak > 0.0 { d.cost / peak } else { 0.0 },
            tooltip: format!("{} · {}", d.day, dollars(d.cost)),
            last: i + 1 == shown.len(),
        })
        .collect()
}

/// The name of a cost account for the filter: its email when a store knows it, else the Swift
/// app's stand-ins.
pub fn account_name(id: &str, email_of: &dyn Fn(&str) -> Option<String>) -> String {
    match id {
        "" => "Before Tracking".to_owned(),
        "agy" => "Antigravity".to_owned(),
        other => email_of(other).unwrap_or_else(|| "Removed Account".to_owned()),
    }
}

fn breakdowns(
    summary: &Summary,
    range: Range,
    all_accounts: bool,
    email_of: &dyn Fn(&str) -> Option<String>,
) -> Vec<Breakdown> {
    let label = range.title();
    let mut sections = Vec::new();
    if all_accounts && !summary.accounts.is_empty() {
        sections.push(Breakdown {
            title: format!("By Account, {label}"),
            rows: summary
                .accounts
                .iter()
                .map(|a| BreakRow {
                    name: account_name(&a.id, email_of),
                    value: dollars(a.cost),
                    detail: None,
                })
                .collect(),
        });
    }
    if summary.channels.len() > 1 {
        sections.push(Breakdown {
            title: format!("By Channel, {label}"),
            rows: summary
                .channels
                .iter()
                .map(|c| BreakRow {
                    name: c.name.clone(),
                    value: if c.cost < 0.01 {
                        let n = summary.channel_tokens.get(&c.name).copied().unwrap_or(0.0);
                        format!("{} tokens", tokens(n))
                    } else {
                        dollars(c.cost)
                    },
                    detail: None,
                })
                .collect(),
        });
    }
    if !summary.model_detail.is_empty() {
        sections.push(Breakdown {
            title: format!("By Model, {label}"),
            rows: summary
                .model_detail
                .iter()
                .take(8)
                .map(|m| BreakRow {
                    name: format!("{} ({:.1}%)", m.name, m.percent),
                    value: dollars(m.cost),
                    detail: Some(format!(
                        "In: {} · Out: {} · Cache: {} read · {} write",
                        tokens(m.input),
                        tokens(m.output),
                        tokens(m.cache_read),
                        tokens(m.cache_write)
                    )),
                })
                .collect(),
        });
    }
    if !summary.projects.is_empty() {
        sections.push(Breakdown {
            title: format!("By Project, {label}"),
            rows: summary
                .projects
                .iter()
                .take(8)
                .map(|p| BreakRow {
                    name: p.name.clone(),
                    value: dollars(p.cost),
                    detail: None,
                })
                .collect(),
        });
    }
    sections
}

/// The page over the daemon's reply (`None` before one), for `range` and the account filter
/// (`""` is every account). `email_of` names a cost account from the stores.
pub fn cost_view(
    reply: Option<&CostReply>,
    range: Range,
    account: &str,
    email_of: &dyn Fn(&str) -> Option<String>,
) -> CostView {
    let empty = Summary::default();
    let summary = match reply {
        Some(r) if !account.is_empty() => r.by_account.get(account).unwrap_or(&empty),
        Some(r) => &r.summary,
        None => &empty,
    };
    let mut accounts = vec![(String::new(), ALL_ACCOUNTS.to_owned())];
    if let Some(r) = reply {
        for id in r.by_account.keys() {
            accounts.push((id.clone(), account_name(id, email_of)));
        }
    }
    CostView {
        accounts,
        account: account.to_owned(),
        tiles: [
            Tile {
                title: "Today",
                value: dollars(summary.today),
            },
            Tile {
                title: "7 Days",
                value: dollars(summary.week),
            },
            Tile {
                title: range.label(),
                value: dollars(summary.month),
            },
        ],
        range,
        reading: reply.is_none_or(|r| r.scanned.is_none()),
        bars_title: match range {
            Range::Days7 => "Last 7 Days",
            _ => "Last 30 Days",
        },
        bars: bars(summary, range),
        sections: breakdowns(summary, range, account.is_empty(), email_of),
        footer: FOOTER,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::cost::summary::{AccountCost, DayCost, ModelStat, NameCost};
    use std::collections::BTreeMap;

    #[test]
    fn money_and_tokens_read_like_the_swift_app() {
        assert_eq!(dollars(12.345), "$12.35");
        assert_eq!(dollars(123.4), "$123");
        assert_eq!(dollars(1234.0), "$1.2k");
        assert_eq!(tokens(390_700.0), "390.7k");
        assert_eq!(tokens(212_400_000.0), "212.4m");
        assert_eq!(tokens(54_700_000_000.0), "54.7b");
        assert_eq!(tokens(12.0), "12");
    }

    fn reply() -> CostReply {
        let summary = Summary {
            days: vec![
                DayCost {
                    day: "2026-11-07".into(),
                    cost: 2.0,
                },
                DayCost {
                    day: "2026-11-08".into(),
                    cost: 1.5,
                },
            ],
            today: 1.5,
            week: 9.0,
            month: 31.0,
            models: vec![],
            model_detail: vec![ModelStat {
                name: "Opus 5.5".into(),
                cost: 30.0,
                percent: 96.8,
                input: 1000.0,
                output: 2000.0,
                cache_read: 3_000_000.0,
                cache_write: 4000.0,
            }],
            projects: vec![NameCost {
                name: "notchling".into(),
                cost: 20.0,
            }],
            accounts: vec![
                AccountCost {
                    id: "u1:o1".into(),
                    cost: 25.0,
                },
                AccountCost {
                    id: String::new(),
                    cost: 6.0,
                },
            ],
            channels: vec![
                NameCost {
                    name: "Claude Code".into(),
                    cost: 30.0,
                },
                NameCost {
                    name: "Codex".into(),
                    cost: 0.0,
                },
            ],
            channel_tokens: BTreeMap::from([("Codex".to_owned(), 123_456.0)]),
        };
        CostReply {
            range: "30".into(),
            scanned: Some(1.0),
            scanning: false,
            by_account: BTreeMap::from([("u1:o1".to_owned(), summary.clone())]),
            summary,
        }
    }

    #[test]
    fn the_page_has_its_tiles_bars_and_breakdowns() {
        let r = reply();
        let names = |id: &str| (id == "u1:o1").then(|| "me@example.com".to_owned());
        let v = cost_view(Some(&r), Range::Days30, "", &names);
        assert_eq!(v.tiles[0].value, "$1.50");
        assert_eq!(
            (v.tiles[2].title, v.tiles[2].value.as_str()),
            ("30 Days", "$31.00")
        );
        assert!(!v.reading);
        assert_eq!(v.bars.len(), 2);
        assert_eq!(v.bars[0].letter, "S", "2026-11-07 is a Saturday");
        assert_eq!(v.bars[1].letter, "S", "2026-11-08 is a Sunday");
        assert!(v.bars[1].last && v.bars[1].fraction == 0.75);
        assert_eq!(v.bars[1].tooltip, "2026-11-08 · $1.50");
        let titles: Vec<&str> = v.sections.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "By Account, Last 30 Days",
                "By Channel, Last 30 Days",
                "By Model, Last 30 Days",
                "By Project, Last 30 Days"
            ]
        );
        assert_eq!(v.sections[0].rows[0].name, "me@example.com");
        assert_eq!(v.sections[0].rows[1].name, "Before Tracking");
        assert_eq!(v.sections[1].rows[1].value, "123.5k tokens");
        assert_eq!(v.sections[2].rows[0].name, "Opus 5.5 (96.8%)");
        assert_eq!(
            v.sections[2].rows[0].detail.as_deref(),
            Some("In: 1.0k · Out: 2.0k · Cache: 3.0m read · 4.0k write")
        );
        assert_eq!(
            v.accounts,
            vec![
                (String::new(), "All Accounts".to_owned()),
                ("u1:o1".to_owned(), "me@example.com".to_owned())
            ]
        );
        // One account: no By Account section; the 7-day range titles follow.
        let one = cost_view(Some(&r), Range::Days7, "u1:o1", &names);
        assert!(one
            .sections
            .iter()
            .all(|s| !s.title.starts_with("By Account")));
        assert_eq!(
            (one.bars_title, one.tiles[2].title),
            ("Last 7 Days", "7 Days")
        );
        // Nothing yet: reading.
        let none = cost_view(None, Range::All, "", &names);
        assert!(none.reading && none.bars.is_empty());
        assert_eq!(none.tiles[0].value, "$0.00");
        assert_eq!(account_name("agy", &names), "Antigravity");
        assert_eq!(account_name("gone", &names), "Removed Account");
    }
}
