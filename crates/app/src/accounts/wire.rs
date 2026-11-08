//! Milestone 6's account, provider, limit and cost ops as the app sends and reads them. The shapes
//! are `notchlingd`'s module docs (`accounts_ops`, `providers_ops`, `limits_ops`, `cost_ops`);
//! the readers take them tolerantly, so a field that is missing is left out and never fails the
//! whole read. No token, cookie or value is ever in a reply, so nothing here could show one.

use std::collections::BTreeMap;

use notchling_engine::cost::summary::Summary;
use notchling_engine::limits::report::UsageReport;
use notchling_local_api::ops;
use serde_json::{json, Value};

/// One op and its arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub op: &'static str,
    pub args: Value,
}

impl Call {
    fn new(op: &'static str, args: Value) -> Call {
        Call { op, args }
    }

    /// `sync` reads the live login first (only with the switch on).
    pub fn accounts_list(sync: bool) -> Call {
        Call::new(ops::ACCOUNTS_LIST, json!({ "sync": sync }))
    }

    pub fn accounts_switch(id: &str) -> Call {
        Call::new(ops::ACCOUNTS_SWITCH, json!({ "id": id }))
    }

    pub fn accounts_add_begin() -> Call {
        Call::new(ops::ACCOUNTS_ADD_BEGIN, json!({}))
    }

    pub fn accounts_add_finish(login: &str, cancel: bool) -> Call {
        let mut args = json!({ "login": login });
        if cancel {
            args["cancel"] = json!(true);
        }
        Call::new(ops::ACCOUNTS_ADD_FINISH, args)
    }

    pub fn accounts_remove(id: &str) -> Call {
        Call::new(ops::ACCOUNTS_REMOVE, json!({ "id": id }))
    }

    /// `target` is an account id; `None` is the main login's account.
    pub fn accounts_move_all(target: Option<&str>, dry_run: bool) -> Call {
        Call::new(
            ops::ACCOUNTS_MOVE_ALL,
            json!({ "target": target, "dryRun": dry_run }),
        )
    }

    pub fn lanes_list() -> Call {
        Call::new(ops::LANES_LIST, json!({}))
    }

    pub fn providers_list(sync: bool) -> Call {
        Call::new(ops::PROVIDERS_LIST, json!({ "sync": sync }))
    }

    pub fn providers_switch(provider: &str, id: &str, confirm: bool) -> Call {
        let mut args = json!({ "provider": provider, "id": id });
        if confirm {
            args["confirm"] = json!(true);
        }
        Call::new(ops::PROVIDERS_SWITCH, args)
    }

    pub fn providers_add_begin(provider: &str, confirm: bool) -> Call {
        let mut args = json!({ "provider": provider });
        if confirm {
            args["confirm"] = json!(true);
        }
        Call::new(ops::PROVIDERS_ADD_BEGIN, args)
    }

    pub fn providers_add_cancel(provider: &str) -> Call {
        Call::new(ops::PROVIDERS_ADD_CANCEL, json!({ "provider": provider }))
    }

    pub fn providers_remove(provider: &str, id: &str) -> Call {
        Call::new(
            ops::PROVIDERS_REMOVE,
            json!({ "provider": provider, "id": id }),
        )
    }

    pub fn providers_usage(provider: &str, refresh: bool) -> Call {
        Call::new(
            ops::PROVIDERS_USAGE,
            json!({ "provider": provider, "refresh": refresh }),
        )
    }

    pub fn limits_status() -> Call {
        Call::new("limits.status", json!({}))
    }

    /// `range` is "7", "30" or "all".
    pub fn cost_summary(range: &str) -> Call {
        Call::new("cost.summary", json!({ "range": range }))
    }
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn flag(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn list(v: &Value, key: &str) -> Vec<Value> {
    v.get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// A saved Claude Code account (`accounts.list`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClaudeAccount {
    pub id: String,
    pub email: String,
    pub plan: Option<String>,
    pub needs_login: bool,
    /// The lane it is live in, if any.
    pub lane: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClaudeList {
    /// The `accounts` switch, as the reply says.
    pub on: bool,
    /// The main login's account id.
    pub active: Option<String>,
    pub accounts: Vec<ClaudeAccount>,
}

pub fn parse_claude_list(v: &Value) -> ClaudeList {
    ClaudeList {
        on: flag(v, "on"),
        active: text(v, "active"),
        accounts: list(v, "accounts")
            .iter()
            .filter_map(|a| {
                Some(ClaudeAccount {
                    id: text(a, "id")?,
                    email: text(a, "email").unwrap_or_default(),
                    plan: text(a, "plan"),
                    needs_login: flag(a, "needsLogin"),
                    lane: text(a, "lane"),
                })
            })
            .collect(),
    }
}

/// A lane (`lanes.list`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lane {
    pub name: String,
    pub path: String,
    pub account_id: Option<String>,
    pub email: Option<String>,
}

pub fn parse_lanes(v: &Value) -> Vec<Lane> {
    list(v, "lanes")
        .iter()
        .filter_map(|l| {
            Some(Lane {
                name: text(l, "name")?,
                path: text(l, "path").unwrap_or_default(),
                account_id: text(l, "accountID"),
                email: text(l, "email"),
            })
        })
        .collect()
}

/// An account of another provider (`providers.list`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderAccount {
    pub id: String,
    pub email: String,
    pub plan: Option<String>,
    pub needs_login: bool,
    pub switchable: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Provider {
    /// `codex`, `antigravity` or `claudeDesktop`.
    pub id: String,
    pub title: String,
    pub capabilities: Vec<String>,
    pub available: bool,
    pub accounts: Vec<ProviderAccount>,
    pub active_id: Option<String>,
    pub busy: bool,
    pub adding: bool,
    pub sign_in_url: Option<String>,
    /// The account in use when it isn't a saved one ("Google Account", a ChatGPT login).
    pub unlisted_active_title: Option<String>,
}

impl Provider {
    pub fn can(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|c| c == capability)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderList {
    pub accounts_on: bool,
    pub providers: Vec<Provider>,
    pub syncing: bool,
}

pub fn parse_providers(v: &Value) -> ProviderList {
    let providers = list(v, "providers")
        .iter()
        .filter_map(|p| {
            let state = p.get("state").cloned().unwrap_or(Value::Null);
            Some(Provider {
                id: text(p, "id")?,
                title: text(p, "title").unwrap_or_default(),
                capabilities: list(p, "capabilities")
                    .iter()
                    .filter_map(|c| c.as_str().map(str::to_owned))
                    .collect(),
                available: flag(p, "available"),
                accounts: list(&state, "accounts")
                    .iter()
                    .filter_map(|a| {
                        Some(ProviderAccount {
                            id: text(a, "id")?,
                            email: text(a, "email").unwrap_or_default(),
                            plan: text(a, "plan"),
                            needs_login: flag(a, "needs_login"),
                            switchable: a
                                .get("switchable")
                                .and_then(Value::as_bool)
                                .unwrap_or(true),
                        })
                    })
                    .collect(),
                active_id: text(&state, "active_id"),
                busy: flag(&state, "busy"),
                adding: flag(&state, "adding"),
                sign_in_url: text(&state, "sign_in_url"),
                unlisted_active_title: text(&state, "unlisted_active_title"),
            })
        })
        .collect();
    ProviderList {
        accounts_on: flag(v, "accounts_on"),
        providers,
        syncing: flag(v, "syncing"),
    }
}

/// The limit reports of one provider's accounts (`providers.usage`), by account id.
pub fn parse_usage(v: &Value) -> BTreeMap<String, UsageReport> {
    v.get("reports")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(id, r)| {
                    serde_json::from_value::<UsageReport>(r.clone())
                        .ok()
                        .map(|r| (id.clone(), r))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A limit window the watcher last saw (`limits.status`).
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub name: String,
    pub percent: f64,
    pub resets_at: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LimitProvider {
    /// `claude`, `codex` or `antigravity`.
    pub id: String,
    pub label: String,
    pub mode: String,
    pub modes: Vec<String>,
    pub registered: bool,
    pub active: Option<String>,
    pub windows: Vec<Window>,
    pub checked: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LimitsStatus {
    pub accounts_on: bool,
    pub watching: bool,
    pub switch_at: i64,
    pub providers: Vec<LimitProvider>,
}

pub fn parse_limits(v: &Value) -> LimitsStatus {
    LimitsStatus {
        accounts_on: flag(v, "accounts_on"),
        watching: flag(v, "watching"),
        switch_at: v.get("switch_at").and_then(Value::as_i64).unwrap_or(95),
        providers: list(v, "providers")
            .iter()
            .filter_map(|p| {
                Some(LimitProvider {
                    id: text(p, "id")?,
                    label: text(p, "label").unwrap_or_default(),
                    mode: text(p, "mode").unwrap_or_default(),
                    modes: list(p, "modes")
                        .iter()
                        .filter_map(|m| m.as_str().map(str::to_owned))
                        .collect(),
                    registered: flag(p, "registered"),
                    active: text(p, "active"),
                    windows: list(p, "windows")
                        .iter()
                        .filter_map(|w| {
                            Some(Window {
                                name: text(w, "name")?,
                                percent: w.get("percent").and_then(Value::as_f64).unwrap_or(0.0),
                                resets_at: w.get("resets_at").and_then(Value::as_f64),
                            })
                        })
                        .collect(),
                    checked: p.get("checked").and_then(Value::as_f64),
                })
            })
            .collect(),
    }
}

/// `cost.summary`'s reply.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CostReply {
    pub range: String,
    /// `None` before the first scan finished.
    pub scanned: Option<f64>,
    pub scanning: bool,
    pub summary: Summary,
    pub by_account: BTreeMap<String, Summary>,
}

pub fn parse_cost(v: &Value) -> CostReply {
    let summary = v
        .get("summary")
        .and_then(|s| serde_json::from_value(s.clone()).ok())
        .unwrap_or_default();
    let by_account = v
        .get("by_account")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(id, s)| {
                    serde_json::from_value::<Summary>(s.clone())
                        .ok()
                        .map(|s| (id.clone(), s))
                })
                .collect()
        })
        .unwrap_or_default();
    CostReply {
        range: text(v, "range").unwrap_or_else(|| "30".into()),
        scanned: v.get("scanned").and_then(Value::as_f64),
        scanning: flag(v, "scanning"),
        summary,
        by_account,
    }
}

/// How a provider switch or add ended (`providers.switch`, `providers.add.begin`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// `switched`, `waiting` (the sign-in page, if any), `switching`, `starting`.
    Done {
        state: String,
        sign_in_url: Option<String>,
    },
    /// Ask the user, then call again with `confirm: true`.
    NeedsConfirmation {
        title: String,
        message: String,
        button: String,
    },
}

pub fn parse_outcome(v: &Value) -> Outcome {
    let state = text(v, "state").unwrap_or_default();
    if state == "needs_confirmation" {
        let c = v.get("confirmation").cloned().unwrap_or(Value::Null);
        return Outcome::NeedsConfirmation {
            title: text(&c, "title").unwrap_or_default(),
            message: text(&c, "message").unwrap_or_default(),
            button: text(&c, "button").unwrap_or_else(|| "Continue".into()),
        };
    }
    Outcome::Done {
        state,
        sign_in_url: text(v, "sign_in_url"),
    }
}

/// A Claude Code sign-in's progress (`accounts.add.begin`, `accounts.add.finish`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddProgress {
    pub login: String,
    /// `waiting`, `added`, `failed` or `cancelled`.
    pub state: String,
    pub url: Option<String>,
    pub email: Option<String>,
    pub error: Option<String>,
}

pub fn parse_add(v: &Value) -> Option<AddProgress> {
    Some(AddProgress {
        login: text(v, "login")?,
        state: text(v, "state").unwrap_or_else(|| "waiting".into()),
        url: text(v, "url"),
        email: v.get("account").and_then(|a| text(a, "email")),
        error: text(v, "error"),
    })
}

/// A Move All dry run's plan (`accounts.move_all {dryRun: true}`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MovePlan {
    /// Background sessions in lanes that stop and resume.
    pub jobs: usize,
    pub close_lane: Option<String>,
    pub switch_main: bool,
    /// Terminal sessions in the closed lane that are signed out.
    pub signed_out: Vec<String>,
}

pub fn parse_move_plan(v: &Value) -> MovePlan {
    let plan = v.get("plan").cloned().unwrap_or(Value::Null);
    MovePlan {
        jobs: plan
            .get("jobs")
            .map(|j| match j {
                Value::Array(a) => a.len(),
                other => other.as_u64().unwrap_or(0) as usize,
            })
            .unwrap_or(0),
        close_lane: text(&plan, "closeLane"),
        switch_main: flag(&plan, "switchMain"),
        signed_out: list(&plan, "signedOut")
            .iter()
            .filter_map(|s| s.as_str().map(str::to_owned))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_calls_have_the_module_docs_shapes() {
        assert_eq!(Call::accounts_list(true).args, json!({"sync": true}));
        assert_eq!(Call::accounts_switch("a1").args, json!({"id": "a1"}));
        assert_eq!(
            Call::accounts_add_finish("l1", true).args,
            json!({"login": "l1", "cancel": true})
        );
        assert_eq!(
            Call::accounts_add_finish("l1", false).args,
            json!({"login": "l1"})
        );
        assert_eq!(
            Call::accounts_move_all(None, true).args,
            json!({"target": null, "dryRun": true})
        );
        assert_eq!(
            Call::providers_switch("codex", "u:a", true).args,
            json!({"provider": "codex", "id": "u:a", "confirm": true})
        );
        assert_eq!(
            Call::providers_add_begin("antigravity", false).args,
            json!({"provider": "antigravity"})
        );
        assert_eq!(
            Call::providers_usage("codex", false).args,
            json!({"provider": "codex", "refresh": false})
        );
        assert_eq!(Call::limits_status().op, "limits.status");
        assert_eq!(Call::cost_summary("all").args, json!({"range": "all"}));
    }

    #[test]
    fn the_claude_list_and_lanes_are_read() {
        let list = parse_claude_list(&json!({"on": true, "active": "a1", "accounts": [
            {"id": "a1", "email": "me@example.com", "plan": "Max 20x", "needsLogin": false, "lane": null},
            {"id": "a2", "email": "ops@example.com", "needsLogin": true, "lane": "night"},
            {"email": "no id"}
        ]}));
        assert!(list.on);
        assert_eq!(list.active.as_deref(), Some("a1"));
        assert_eq!(list.accounts.len(), 2);
        assert_eq!(list.accounts[0].plan.as_deref(), Some("Max 20x"));
        assert!(list.accounts[1].needs_login);
        assert_eq!(list.accounts[1].lane.as_deref(), Some("night"));
        let lanes = parse_lanes(
            &json!({"lanes": [{"name": "night", "path": "/h/.claude-lanes/night",
            "accountID": "a2", "email": "ops@example.com", "folder": "/h/.claude-lanes/night"}]}),
        );
        assert_eq!(lanes[0].email.as_deref(), Some("ops@example.com"));
        assert_eq!(parse_claude_list(&Value::Null), ClaudeList::default());
    }

    #[test]
    fn providers_usage_limits_and_cost_are_read() {
        let p = parse_providers(&json!({"accounts_on": false, "providers": [
            {"id": "codex", "title": "Codex", "capabilities": ["addAccount", "usage"], "available": true,
             "state": {"accounts": [{"id": "u:a", "email": "me@example.com", "plan": "Plus",
                       "needs_login": false, "switchable": true}],
                       "active_id": null, "busy": false, "adding": true, "sign_in_url": "https://x",
                       "unlisted_active_title": null}}]}));
        assert!(!p.accounts_on);
        let codex = &p.providers[0];
        assert!(codex.can("usage") && !codex.can("cost"));
        assert!(codex.adding && codex.active_id.is_none());
        assert_eq!(codex.accounts[0].plan.as_deref(), Some("Plus"));
        let usage = parse_usage(
            &json!({"provider": "codex", "reports": {"u:a": {"fetched": 1.0,
            "groups": [{"name": "Codex", "bars": [{"label": "5h", "kind": "session", "percent": 41.0,
            "resets_at": 2.0}]}]}}}),
        );
        assert_eq!(usage["u:a"].groups[0].bars[0].percent, 41.0);
        let limits = parse_limits(
            &json!({"accounts_on": true, "watching": true, "switch_at": 90,
            "providers": [{"id": "claude", "label": "Claude Code", "mode": "notify",
            "modes": ["off", "notify"], "registered": true, "active": "me@example.com",
            "windows": [{"name": "5-hour", "percent": 41.0, "resets_at": 3.0}], "checked": 4.0}]}),
        );
        assert_eq!(limits.switch_at, 90);
        assert_eq!(limits.providers[0].windows[0].name, "5-hour");
        let cost = parse_cost(&json!({"range": "7", "scanned": null, "scanning": true,
            "summary": {"days": [{"day": "2026-11-08", "cost": 1.5}], "today": 1.5, "week": 9.0,
                        "month": 31.0, "models": [], "model_detail": [], "projects": [],
                        "accounts": [], "channels": [], "channel_tokens": {}},
            "by_account": {}}));
        assert!(cost.scanning && cost.scanned.is_none());
        assert_eq!(cost.summary.week, 9.0);
        assert_eq!(cost.range, "7");
    }

    #[test]
    fn outcomes_sign_ins_and_move_plans_are_read() {
        assert_eq!(
            parse_outcome(
                &json!({"state": "needs_confirmation", "confirmation": {"title": "Restart Antigravity?",
                "message": "Antigravity quits.", "button": "Restart"}})
            ),
            Outcome::NeedsConfirmation {
                title: "Restart Antigravity?".into(),
                message: "Antigravity quits.".into(),
                button: "Restart".into()
            }
        );
        assert_eq!(
            parse_outcome(
                &json!({"state": "waiting", "provider": "codex", "sign_in_url": "https://x"})
            ),
            Outcome::Done {
                state: "waiting".into(),
                sign_in_url: Some("https://x".into())
            }
        );
        let add = parse_add(
            &json!({"login": "l1", "state": "added", "account": {"email": "me@example.com"}}),
        );
        assert_eq!(add.unwrap().email.as_deref(), Some("me@example.com"));
        assert_eq!(parse_add(&json!({"state": "waiting"})), None);
        let plan = parse_move_plan(&json!({"plan": {"jobs": ["j1", "j2"], "closeLane": "night",
            "switchMain": true, "signedOut": ["t1"]}}));
        assert_eq!(plan.jobs, 2);
        assert_eq!(plan.close_lane.as_deref(), Some("night"));
        assert!(plan.switch_main);
        assert_eq!(parse_move_plan(&json!({"plan": {"jobs": 3}})).jobs, 3);
    }
}
