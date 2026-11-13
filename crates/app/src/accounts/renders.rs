use std::collections::BTreeMap;

use notchling_engine::cost::summary::{AccountCost, DayCost, ModelStat, NameCost, Summary};
use notchling_engine::limits::report::{Bar, BarKind, Group, UsageReport};

use super::store::Snapshot;
use super::wire::{
    AddProgress, ClaudeAccount, ClaudeList, ClaudeUsage, CostReply, Lane, LimitProvider,
    LimitsStatus, Provider, ProviderAccount, ProviderList, Window,
};
use crate::switches::Switches;
use crate::views::ViewSpec;

pub const NOW: f64 = 1_800_000_000.0;

pub const MENU_RENDER: &str = "accounts-window-menu";

pub const VIEWS: [ViewSpec; 6] = [
    ViewSpec {
        name: "accounts-window",
        size: (560.0, 640.0),
        about: "the Accounts window, Claude Code tab, accounts switch on (made-up data)",
    },
    ViewSpec {
        name: MENU_RENDER,
        size: (560.0, 640.0),
        about: "the Accounts window with the first Claude Desktop row's ⋯ menu open over the row below (made-up data)",
    },
    ViewSpec {
        name: "accounts-window-off",
        size: (560.0, 640.0),
        about: "the Accounts window with the accounts switch off: lists show, controls say why (made-up data)",
    },
    ViewSpec {
        name: "accounts-window-codex",
        size: (560.0, 640.0),
        about: "the Accounts window on the Codex tab, a sign-in waiting (made-up data)",
    },
    ViewSpec {
        name: "limit-bars",
        size: (420.0, 460.0),
        about: "grouped limit bars in every fill, and the Chat header's meters (made-up data)",
    },
    ViewSpec {
        name: "cost",
        size: (560.0, 900.0),
        about: "the Token Cost page over a made-up summary",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Claude,
    Off,
    Codex,
    LimitBars,
    Cost,
}

pub fn shown(name: &str) -> Option<Shown> {
    Some(match name {
        "accounts-window" | MENU_RENDER => Shown::Claude,
        "accounts-window-off" => Shown::Off,
        "accounts-window-codex" => Shown::Codex,
        "limit-bars" => Shown::LimitBars,
        "cost" => Shown::Cost,
        _ => return None,
    })
}

pub fn switches(shown: Shown) -> Switches {
    match shown {
        Shown::Off => Switches::all_off(),
        _ => Switches::all_on(),
    }
}

fn bar(label: &str, kind: BarKind, percent: f64, resets_in: Option<f64>) -> Bar {
    Bar {
        label: label.into(),
        kind,
        percent,
        resets_at: resets_in.map(|s| NOW + s),
    }
}

pub fn codex_report() -> UsageReport {
    UsageReport {
        groups: vec![Group {
            name: Some("Codex".into()),
            bars: vec![
                bar(
                    "5h",
                    BarKind::Session,
                    41.0,
                    Some(2.0 * 3600.0 + 13.0 * 60.0),
                ),
                bar("Week", BarKind::Week, 73.0, Some(3.0 * 86_400.0)),
            ],
        }],
        fetched: NOW,
    }
}

pub fn claude_report() -> UsageReport {
    UsageReport {
        groups: vec![Group {
            name: None,
            bars: vec![
                bar("5 hours", BarKind::Session, 12.0, Some(4.0 * 3600.0)),
                bar("Week", BarKind::Week, 58.0, Some(2.0 * 86_400.0)),
            ],
        }],
        fetched: NOW,
    }
}

pub fn antigravity_report() -> UsageReport {
    UsageReport {
        groups: vec![
            Group {
                name: Some("Gemini 3 Pro".into()),
                bars: vec![
                    bar("5h", BarKind::Session, 92.0, Some(40.0 * 60.0)),
                    bar("Week", BarKind::Week, 35.0, Some(5.0 * 86_400.0)),
                ],
            },
            Group {
                name: Some("Claude".into()),
                bars: vec![bar("5h", BarKind::Session, 12.0, Some(4.0 * 3600.0))],
            },
        ],
        fetched: NOW,
    }
}

fn cost_reply() -> CostReply {
    let days: Vec<DayCost> = (0..30)
        .map(|i| DayCost {
            day: format!("2026-10-{:02}", 10 + i % 20),
            cost: [1.2, 3.4, 0.6, 4.8, 2.1, 0.0, 5.3][i % 7],
        })
        .collect();
    let summary = Summary {
        days,
        today: 5.3,
        week: 17.4,
        month: 61.9,
        models: vec![],
        model_detail: vec![
            ModelStat {
                name: "Opus 5.5".into(),
                cost: 48.1,
                percent: 77.7,
                input: 812_000.0,
                output: 96_000.0,
                cache_read: 14_200_000.0,
                cache_write: 1_300_000.0,
            },
            ModelStat {
                name: "Sonnet 5.5".into(),
                cost: 11.2,
                percent: 18.1,
                input: 2_100_000.0,
                output: 240_000.0,
                cache_read: 9_800_000.0,
                cache_write: 600_000.0,
            },
            ModelStat {
                name: "gpt-5.3-codex".into(),
                cost: 2.6,
                percent: 4.2,
                input: 400_000.0,
                output: 50_000.0,
                cache_read: 0.0,
                cache_write: 0.0,
            },
        ],
        projects: vec![
            NameCost {
                name: "notchling".into(),
                cost: 40.2,
            },
            NameCost {
                name: "phone-app".into(),
                cost: 21.7,
            },
        ],
        accounts: vec![
            AccountCost {
                id: "a1".into(),
                cost: 44.0,
            },
            AccountCost {
                id: "a2".into(),
                cost: 12.9,
            },
            AccountCost {
                id: String::new(),
                cost: 5.0,
            },
        ],
        channels: vec![
            NameCost {
                name: "Claude Code".into(),
                cost: 59.3,
            },
            NameCost {
                name: "Codex".into(),
                cost: 2.6,
            },
            NameCost {
                name: "Antigravity CLI".into(),
                cost: 0.0,
            },
        ],
        channel_tokens: BTreeMap::from([("Antigravity CLI".to_owned(), 390_700.0)]),
        unpriced: vec![],
        borrowed: vec![],
    };
    let mut small = summary.clone();
    small.today = 0.4;
    small.week = 3.1;
    small.month = 12.9;
    CostReply {
        range: "30".into(),
        scanned: Some(NOW),
        scanning: false,
        by_account: BTreeMap::from([("a1".to_owned(), summary.clone()), ("a2".to_owned(), small)]),
        summary,
    }
}

pub fn snapshot(shown: Shown) -> Snapshot {
    let on = shown != Shown::Off;
    let account = |id: &str, email: &str, plan: Option<&str>| ClaudeAccount {
        id: id.into(),
        email: email.into(),
        plan: plan.map(str::to_owned),
        ..ClaudeAccount::default()
    };
    let mut claude = ClaudeList {
        on,
        active: on.then(|| "a1".to_owned()),
        accounts: vec![
            account("a1", "me@example.com", Some("Max 20x")),
            ClaudeAccount {
                lane: Some("night".into()),
                ..account("a2", "ops@example.com", Some("Max 5x"))
            },
            ClaudeAccount {
                needs_login: true,
                ..account("a3", "old@example.com", Some("Pro"))
            },
            account("a4", "team@example.com", Some("Team")),
        ],
    };
    if !on {
        claude.accounts[1].lane = None;
    }
    let provider =
        |id: &str, title: &str, caps: &[&str], accounts: Vec<ProviderAccount>| Provider {
            id: id.into(),
            title: title.into(),
            capabilities: caps.iter().map(|c| (*c).to_owned()).collect(),
            available: true,
            active_id: on.then(|| accounts[0].id.clone()),
            accounts,
            ..Provider::default()
        };
    let pa = |id: &str, email: &str, plan: Option<&str>| ProviderAccount {
        id: id.into(),
        email: email.into(),
        plan: plan.map(str::to_owned),
        needs_login: false,
        switchable: true,
    };
    let mut codex = provider(
        "codex",
        "Codex",
        &["addAccount", "usage", "autoSwitch", "cost"],
        vec![
            pa("u1:o1", "me@example.com", Some("Plus")),
            pa("u2:o2", "work@example.com", None),
        ],
    );
    if shown == Shown::Codex {
        codex.adding = true;
        codex.sign_in_url = Some("https://auth.openai.com/example".into());
    }
    let antigravity = provider(
        "antigravity",
        "Antigravity",
        &["addAccount", "restartsApp", "usage", "cost"],
        vec![pa("g1", "me@gmail.example", None)],
    );
    let mut desktop = provider(
        "claudeDesktop",
        "Claude Desktop",
        &["addAccount", "restartsApp"],
        vec![pa("a1", "me@example.com", Some("Max 20x"))],
    );
    desktop.accounts.push(ProviderAccount {
        switchable: false,
        ..pa("a4", "team@example.com", Some("Team"))
    });
    let mut usage = BTreeMap::new();
    if on {
        usage.insert(
            "codex".to_owned(),
            BTreeMap::from([("u1:o1".to_owned(), codex_report())]),
        );
        usage.insert(
            "antigravity".to_owned(),
            BTreeMap::from([("g1".to_owned(), antigravity_report())]),
        );
    }
    Snapshot {
        attached: true,
        claude,
        lanes: vec![Lane {
            name: "night".into(),
            path: "/Users/me/.claude-lanes/night".into(),
            account_id: Some("a2".into()),
            email: Some("ops@example.com".into()),
        }],
        providers: ProviderList {
            accounts_on: on,
            providers: vec![desktop, codex, antigravity],
            syncing: false,
        },
        usage,
        claude_usage: ClaudeUsage {
            reports: if on {
                BTreeMap::from([("a2".to_owned(), claude_report())])
            } else {
                BTreeMap::new()
            },
            errors: BTreeMap::new(),
        },
        limits: LimitsStatus {
            accounts_on: on,
            watching: on,
            switch_at: 95,
            providers: vec![LimitProvider {
                id: "claude".into(),
                label: "Claude Code".into(),
                mode: "notifyAndSwitch".into(),
                modes: vec![],
                registered: on,
                active: on.then(|| "me@example.com".to_owned()),
                windows: if on {
                    vec![
                        Window {
                            name: "5-hour".into(),
                            percent: 41.0,
                            resets_at: Some(NOW + 2.0 * 3600.0 + 13.0 * 60.0),
                        },
                        Window {
                            name: "Weekly".into(),
                            percent: 78.0,
                            resets_at: Some(NOW + 3.0 * 86_400.0),
                        },
                    ]
                } else {
                    vec![]
                },
                checked: on.then_some(NOW),
            }],
        },
        cost: Some(cost_reply()),
        add: (shown == Shown::Claude).then(|| AddProgress {
            login: "l1".into(),
            state: "added".into(),
            url: None,
            email: Some("team@example.com".into()),
            error: None,
        }),
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::limits::Zone;
    use crate::accounts::model::{page, tabs, CLAUDE_CODE};

    #[test]
    fn every_view_has_its_state() {
        for spec in VIEWS {
            assert!(shown(spec.name).is_some(), "{}", spec.name);
            assert!(crate::views::spec(spec.name).is_some(), "{}", spec.name);
        }
        assert_eq!(shown("chat"), None);
        let s = snapshot(Shown::Claude);
        assert_eq!(tabs(&s).len(), 4);
        let p = page(
            CLAUDE_CODE,
            &s,
            &switches(Shown::Claude),
            false,
            NOW,
            Zone::UTC,
        );
        assert_eq!(p.rows.len(), 4);
        assert!(p.rows[0].in_use);
        let off = page(
            CLAUDE_CODE,
            &snapshot(Shown::Off),
            &switches(Shown::Off),
            false,
            NOW,
            Zone::UTC,
        );
        assert_eq!(off.rows.len(), 4, "the saved names still list while off");
        assert!(off.rows.iter().all(|r| !r.in_use));
        assert!(snapshot(Shown::Codex).providers.providers[1].adding);
    }
}
