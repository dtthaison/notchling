use crate::acting::Control;
use crate::switches::{Switch, Switches};

use super::flows::{add_panel, AddPanel};
use super::limits::{self, LimitGroup, Zone};
use super::store::Snapshot;
use super::wire::Provider;
use notchling_engine::limits::report::UsageReport;

pub const CLAUDE_CODE: &str = "claudeCode";
pub const CLAUDE_DESKTOP: &str = "claudeDesktop";
pub const CODEX: &str = "codex";
pub const ANTIGRAVITY: &str = "antigravity";

pub const TAB_ORDER: [&str; 4] = [CLAUDE_CODE, CLAUDE_DESKTOP, CODEX, ANTIGRAVITY];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Accounts,
    TokenCost,
}

impl Tab {
    pub const ALL: [Tab; 2] = [Tab::Accounts, Tab::TokenCost];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Accounts => "Accounts",
            Tab::TokenCost => "Token Cost",
        }
    }
}

pub const REFRESH_TIP: &str = "Refresh accounts, limits and cost";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderTab {
    pub id: String,
    pub title: String,
}

pub fn title_of(id: &str) -> &'static str {
    match id {
        CLAUDE_CODE => "Claude Code",
        CLAUDE_DESKTOP => "Claude Desktop",
        CODEX => "Codex",
        ANTIGRAVITY => "Antigravity",
        _ => "Accounts",
    }
}

pub fn tabs(s: &Snapshot) -> Vec<ProviderTab> {
    TAB_ORDER
        .iter()
        .filter(|id| **id == CLAUDE_CODE || s.providers.providers.iter().any(|p| p.id == **id))
        .map(|id| ProviderTab {
            id: (*id).to_owned(),
            title: title_of(id).to_owned(),
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowAction {
    Remove,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItem {
    pub title: &'static str,
    pub action: RowAction,
    pub control: Control,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RowContent {
    None,
    Text { text: String, warning: bool },
    Limits(Vec<LimitGroup>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct AccountRow {
    pub id: String,
    pub provider: String,
    pub email: String,
    pub subtitle: String,
    pub in_use: bool,
    pub switch: Option<Control>,
    pub menu: Vec<MenuItem>,
    pub content: RowContent,
    pub cost: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub provider: String,
    pub title: &'static str,
    pub empty: Option<&'static str>,
    pub rows: Vec<AccountRow>,
    pub note: Option<String>,
    pub add: AddPanel,
    pub off: Option<&'static str>,
    pub error: Option<String>,
}

fn gate(switches: &Switches, busy: bool) -> Control {
    let control = switches.control(Switch::Accounts);
    if busy && control.enabled {
        Control {
            enabled: false,
            tooltip: None,
        }
    } else {
        control
    }
}

fn cost_line(s: &Snapshot, account: &str) -> Option<String> {
    let summary = s.cost.as_ref()?.by_account.get(account)?;
    (summary.month > 0.0).then(|| {
        format!(
            "Today {} · 7 days {} · 30 days {}",
            super::cost::dollars(summary.today),
            super::cost::dollars(summary.week),
            super::cost::dollars(summary.month)
        )
    })
}

fn claude_page(s: &Snapshot, switches: &Switches, busy: bool, now: f64, zone: Zone) -> Page {
    let on = switches.is_on(Switch::Accounts);
    let watched = s.limits.claude_for(s.claude.active.as_deref());
    let windows = watched
        .map(|p| limits::from_windows(&p.windows, now, zone))
        .unwrap_or_default();
    let checked = watched.and_then(|p| p.checked);
    let rows = s
        .claude
        .accounts
        .iter()
        .map(|a| {
            let in_use = s.claude.active.as_deref() == Some(a.id.as_str());
            let mut subtitle = a.plan.clone().unwrap_or_default();
            if let Some(lane) = &a.lane {
                if !subtitle.is_empty() {
                    subtitle.push_str(" · ");
                }
                subtitle.push_str(&format!("{lane} lane"));
            }
            let report = s.claude_usage.reports.get(&a.id);
            let watched_is_newer = report.is_none_or(|r| checked.is_some_and(|at| at > r.fetched));
            let content = if a.needs_login {
                RowContent::Text {
                    text: "Signed out. Add this account again to sign in.".into(),
                    warning: true,
                }
            } else if in_use && !windows.is_empty() && watched_is_newer {
                RowContent::Limits(vec![LimitGroup {
                    name: None,
                    lines: windows.clone(),
                }])
            } else if let Some(report) = report {
                if report.is_empty() {
                    RowContent::Text {
                        text: "This account reports no limits.".into(),
                        warning: false,
                    }
                } else {
                    RowContent::Limits(limits::groups(report, now, zone))
                }
            } else if let Some(why) = s.claude_usage.errors.get(&a.id) {
                RowContent::Text {
                    text: format!("Couldn't read limits. {why}"),
                    warning: true,
                }
            } else if on {
                RowContent::Text {
                    text: "Checking limits…".into(),
                    warning: false,
                }
            } else {
                RowContent::Text {
                    text: "Limits show once Accounts is on.".into(),
                    warning: false,
                }
            };
            AccountRow {
                id: a.id.clone(),
                provider: CLAUDE_CODE.into(),
                email: a.email.clone(),
                subtitle,
                in_use,
                switch: (!in_use && !a.needs_login && a.lane.is_none())
                    .then(|| gate(switches, busy)),
                menu: vec![MenuItem {
                    title: "Remove…",
                    action: RowAction::Remove,
                    control: gate(switches, busy),
                }],
                content,
                cost: cost_line(s, &a.id),
            }
        })
        .collect::<Vec<_>>();
    Page {
        provider: CLAUDE_CODE.into(),
        title: title_of(CLAUDE_CODE),
        empty: rows
            .is_empty()
            .then_some("No accounts yet. Add one to see its limits and switch to it."),
        rows,
        note: None,
        add: add_panel(CLAUDE_CODE, s, switches, busy),
        off: switches.message(Switch::Accounts),
        error: s.error.clone(),
    }
}

fn default_plan(provider: &str) -> &'static str {
    match provider {
        CODEX => "ChatGPT",
        ANTIGRAVITY => "Google Account",
        _ => "",
    }
}

fn empty_text(provider: &str) -> &'static str {
    match provider {
        CLAUDE_DESKTOP => "No accounts in Claude Desktop yet. Use Add Account to sign in to one.",
        CODEX => "Sign in to Codex and Notchling saves the account here.",
        ANTIGRAVITY => "Sign in to Antigravity and Notchling saves the account here.",
        _ => "No accounts yet.",
    }
}

fn desktop_limits<'a>(s: &'a Snapshot, uuid: &str) -> Option<&'a UsageReport> {
    s.claude
        .accounts
        .iter()
        .filter(|c| c.id.split(':').next() == Some(uuid))
        .find_map(|c| s.claude_usage.reports.get(&c.id))
}

fn provider_page(
    p: &Provider,
    s: &Snapshot,
    switches: &Switches,
    busy: bool,
    now: f64,
    zone: Zone,
) -> Page {
    let reports = s.usage.get(&p.id);
    let rows = p
        .accounts
        .iter()
        .map(|a| {
            let in_use = p.active_id.as_deref() == Some(a.id.as_str());
            let report = reports.and_then(|r| r.get(&a.id)).or_else(|| {
                (p.id == CLAUDE_DESKTOP)
                    .then(|| desktop_limits(s, &a.id))
                    .flatten()
            });
            let content = match report {
                Some(report) => RowContent::Limits(limits::groups(report, now, zone)),
                None => match (p.id.as_str(), in_use) {
                    (CODEX, false) => RowContent::Text {
                        text: "Limits show again once you switch to it.".into(),
                        warning: false,
                    },
                    (ANTIGRAVITY, true) => RowContent::Text {
                        text: "Open Antigravity or an Antigravity chat to read its limits.".into(),
                        warning: false,
                    },
                    _ => RowContent::None,
                },
            };
            let content = if a.needs_login {
                RowContent::Text {
                    text: "Signed out. Add this account again to sign in.".into(),
                    warning: true,
                }
            } else {
                content
            };
            AccountRow {
                id: a.id.clone(),
                provider: p.id.clone(),
                email: a.email.clone(),
                subtitle: a
                    .plan
                    .clone()
                    .unwrap_or_else(|| default_plan(&p.id).to_owned()),
                in_use,
                switch: (!in_use && a.switchable && !a.needs_login)
                    .then(|| gate(switches, busy || p.busy || p.adding)),
                menu: vec![MenuItem {
                    title: if p.id == CLAUDE_DESKTOP {
                        "Remove from List…"
                    } else {
                        "Remove…"
                    },
                    action: RowAction::Remove,
                    control: gate(switches, busy || p.busy || p.adding),
                }],
                content,
                cost: cost_line(s, &a.id),
            }
        })
        .collect::<Vec<_>>();
    Page {
        provider: p.id.clone(),
        title: title_of(&p.id),
        empty: rows.is_empty().then(|| empty_text(&p.id)),
        rows,
        note: p
            .unlisted_active_title
            .as_ref()
            .filter(|_| p.active_id.is_none())
            .map(|t| format!("In use: {t} (not saved)")),
        add: add_panel(&p.id, s, switches, busy),
        off: switches.message(Switch::Accounts),
        error: s.error.clone(),
    }
}

pub fn page(
    provider: &str,
    s: &Snapshot,
    switches: &Switches,
    busy: bool,
    now: f64,
    zone: Zone,
) -> Page {
    if provider == CLAUDE_CODE {
        return claude_page(s, switches, busy, now, zone);
    }
    match s.providers.providers.iter().find(|p| p.id == provider) {
        Some(p) => provider_page(p, s, switches, busy, now, zone),
        None => Page {
            provider: provider.to_owned(),
            title: title_of(provider),
            empty: Some(empty_text(provider)),
            rows: Vec::new(),
            note: None,
            add: add_panel(provider, s, switches, busy),
            off: switches.message(Switch::Accounts),
            error: s.error.clone(),
        },
    }
}

pub fn initial(email: &str) -> String {
    email
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into())
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
