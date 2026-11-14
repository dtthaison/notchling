use std::collections::BTreeMap;

use notchling_engine::cost::summary::Summary;
use notchling_engine::limits::report::UsageReport;
use notchling_local_api::ops;
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub op: &'static str,
    pub args: Value,
}

impl Call {
    fn new(op: &'static str, args: Value) -> Call {
        Call { op, args }
    }

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

    pub fn accounts_move_all(target: Option<&str>, dry_run: bool) -> Call {
        Call::new(
            ops::ACCOUNTS_MOVE_ALL,
            json!({ "target": target, "dryRun": dry_run }),
        )
    }

    pub fn accounts_result(ticket: u64) -> Call {
        Call::new(ops::ACCOUNTS_RESULT, json!({ "ticket": ticket }))
    }

    pub fn lanes_list() -> Call {
        Call::new(ops::LANES_LIST, json!({}))
    }

    pub fn lanes_create(account: &str) -> Call {
        Call::new(ops::LANES_CREATE, json!({ "account": account }))
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

    pub fn limits_setting(key: &str, value: &Value) -> Option<Call> {
        let provider = match key {
            "limitMode" => "claude",
            "codexLimitMode" => "codex",
            "antigravityLimitMode" => "antigravity",
            "limitSwitchAt" => {
                let at = value.as_i64()?;
                return Some(Call::new(
                    "limits.set_mode",
                    json!({ "provider": "claude", "switch_at": at }),
                ));
            }
            _ => return None,
        };
        let mode = value.as_str()?;
        Some(Call::new(
            "limits.set_mode",
            json!({ "provider": provider, "mode": mode }),
        ))
    }

    pub const LIMIT_KEYS: [&'static str; 4] = [
        "limitMode",
        "codexLimitMode",
        "antigravityLimitMode",
        "limitSwitchAt",
    ];

    pub fn limits_status() -> Call {
        Call::new("limits.status", json!({}))
    }

    pub fn limits_usage(refresh: bool) -> Call {
        Call::new("limits.usage", json!({ "refresh": refresh }))
    }

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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClaudeAccount {
    pub id: String,
    pub email: String,
    pub plan: Option<String>,
    pub needs_login: bool,
    pub lane: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClaudeList {
    pub on: bool,
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
    pub id: String,
    pub title: String,
    pub capabilities: Vec<String>,
    pub available: bool,
    pub accounts: Vec<ProviderAccount>,
    pub active_id: Option<String>,
    pub busy: bool,
    pub adding: bool,
    pub sign_in_url: Option<String>,
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

pub fn still_refreshing(v: &Value) -> bool {
    flag(v, "refreshing")
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClaudeUsage {
    pub reports: BTreeMap<String, UsageReport>,
    pub errors: BTreeMap<String, String>,
}

pub fn parse_claude_usage(v: &Value) -> ClaudeUsage {
    ClaudeUsage {
        reports: parse_usage(v),
        errors: v
            .get("errors")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(id, why)| Some((id.clone(), why.as_str()?.to_owned())))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub name: String,
    pub percent: f64,
    pub resets_at: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LimitProvider {
    pub id: String,
    pub label: String,
    pub mode: String,
    pub modes: Vec<String>,
    pub registered: bool,
    pub active: Option<String>,
    pub active_id: Option<String>,
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

impl LimitsStatus {
    pub fn claude_for(&self, active: Option<&str>) -> Option<&LimitProvider> {
        let active = active?;
        self.providers
            .iter()
            .find(|p| p.id == "claude" && p.active_id.as_deref() == Some(active))
    }
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
                    active_id: text(p, "active_id"),
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CostReply {
    pub range: String,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done {
        state: String,
        sign_in_url: Option<String>,
    },
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddProgress {
    pub login: String,
    pub state: String,
    pub url: Option<String>,
    pub email: Option<String>,
    pub error: Option<String>,
}

pub fn working_ticket(v: &Value) -> Option<u64> {
    (text(v, "state").as_deref() == Some("working"))
        .then(|| v.get("ticket").and_then(Value::as_u64))
        .flatten()
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MovePlan {
    pub jobs: usize,
    pub close_lane: Option<String>,
    pub switch_main: bool,
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MoveResult {
    pub moved: Vec<String>,
    pub errors: Vec<(String, String)>,
    pub closed_lane: Option<String>,
    pub switched_to: Option<String>,
    pub pending: bool,
    has_fields: bool,
}

impl MoveResult {
    pub fn is_result(&self) -> bool {
        !self.pending && self.has_fields
    }
}

pub fn parse_move_result(v: &Value) -> MoveResult {
    let mut errors: Vec<(String, String)> = v
        .get("errors")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(k, e)| {
                    let why = e
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| e.to_string());
                    (k.clone(), why)
                })
                .collect()
        })
        .unwrap_or_default();
    errors.sort_by_key(|(k, _)| k == "accounts");
    let has = v.get("moved").is_some() || v.get("errors").is_some();
    MoveResult {
        moved: list(v, "moved")
            .iter()
            .filter_map(|s| s.as_str().map(str::to_owned))
            .collect(),
        errors,
        closed_lane: text(v, "closedLane"),
        switched_to: text(v, "switchedTo"),
        pending: !has && v.get("state").is_some(),
        has_fields: has,
    }
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
