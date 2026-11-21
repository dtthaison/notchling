use notchling_locale as t;
use serde_json::Value;

use crate::chat::format::duration;

pub const ALLOW_DELAY_MS: i64 = 1000;
const SECRET_REQUEST: &str = "secret_request";
const SWITCH: &str = "switch";
const SESSION_CHARS: usize = 8;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Env {
    pub now_ms: i64,
    pub vault_on: bool,
    pub shown_ms: i64,
    pub node: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detail {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Untrusted {
    pub label: String,
    pub note: Option<String>,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Allow {
    Ready,
    Wait(i64),
    Off,
}

impl Allow {
    pub fn enabled(self) -> bool {
        self == Allow::Ready
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConsentCard {
    pub id: String,
    pub facts: Vec<String>,
    pub details: Vec<Detail>,
    pub says: Option<Untrusted>,
    pub value: Option<Untrusted>,
    pub expiry: String,
    pub ended: bool,
    pub blocked: Option<String>,
    pub allow: Allow,
    pub answerable: bool,
    pub app_applied: bool,
    pub switch: Option<String>,
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

fn shown(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        None | Some(Value::Null) => t::consent::card_unset().to_owned(),
        Some(other) => other.to_string(),
    }
}

fn tool_name(tool: &str) -> &'static str {
    match tool {
        "claude" => t::consent::card_tool_claude(),
        "codex" => t::consent::card_tool_codex(),
        "antigravity" => t::consent::card_tool_antigravity(),
        _ => t::consent::card_tool_other(),
    }
}

fn who(view: &Value) -> String {
    let tool = view.get("tool").and_then(Value::as_str).unwrap_or("");
    let verified = view.get("verified").and_then(Value::as_bool) == Some(true);
    match text(view, "session") {
        Some(session) => {
            let short: String = session.chars().take(SESSION_CHARS).collect();
            if verified {
                t::consent::card_who_verified(tool_name(tool), &short)
            } else {
                t::consent::card_who_unverified(tool_name(tool), &short)
            }
        }
        None => match text(view, "root") {
            Some(app) => t::consent::card_who_outside_app(app),
            None => t::consent::card_who_outside().to_owned(),
        },
    }
}

fn switch_words(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name {
        "acting" => (
            t::consent::card_switch_acting(),
            t::consent::card_switch_acting_unlocks(),
        ),
        "bridge" => (
            t::consent::card_switch_bridge(),
            t::consent::card_switch_bridge_unlocks(),
        ),
        "accounts" => (
            t::consent::card_switch_accounts(),
            t::consent::card_switch_accounts_unlocks(),
        ),
        "vault" => (
            t::consent::card_switch_vault(),
            t::consent::card_switch_vault_unlocks(),
        ),
        "install" => (
            t::consent::card_switch_install(),
            t::consent::card_switch_install_unlocks(),
        ),
        "cluster" => (
            t::consent::card_switch_cluster(),
            t::consent::card_switch_cluster_unlocks(),
        ),
        _ => return None,
    })
}

fn switch_of(view: &Value) -> Option<String> {
    if view.get("kind").and_then(Value::as_str) != Some(SWITCH) {
        return None;
    }
    text(view, "target")
        .filter(|name| switch_words(name).is_some())
        .map(str::to_owned)
}

fn fact_line(line: &str, args: &Value) -> Option<String> {
    match line {
        "consent-switch-on" => {
            switch_words(text(args, "switch")?).map(|(title, _)| title.to_owned())
        }
        "consent-setting-change" => {
            let title = text(args, "title").or_else(|| text(args, "key"))?;
            Some(t::consent::card_fact_setting(title))
        }
        "consent-secret-request" => {
            let name = text(args, "name")?;
            let update = args.get("update").and_then(Value::as_bool) == Some(true);
            Some(if update {
                t::consent::card_fact_secret_update(name)
            } else {
                t::consent::card_fact_secret_add(name)
            })
        }
        _ => None,
    }
}

fn is_setting(view: &Value) -> bool {
    view.get("kind").and_then(Value::as_str) == Some("setting")
}

fn is_text(view: &Value) -> bool {
    view.get("change")
        .and_then(|c| c.get("text"))
        .and_then(Value::as_bool)
        == Some(true)
}

fn facts(view: &Value) -> Vec<String> {
    let mut out: Vec<String> = view
        .get("facts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|fact| {
            let line = fact.get("line").and_then(Value::as_str)?;
            fact_line(line, fact.get("args").unwrap_or(&Value::Null))
        })
        .collect();
    let change = view.get("change").unwrap_or(&Value::Null);
    if is_setting(view) && !is_text(view) && change.get("after").is_some() {
        out.push(t::consent::card_fact_before(&shown(change.get("before"))));
        out.push(t::consent::card_fact_after(&shown(change.get("after"))));
    }
    if let Some((_, unlocks)) = switch_of(view).as_deref().and_then(switch_words) {
        out.push(unlocks.to_owned());
        out.push(t::consent::card_switch_until_restart().to_owned());
    }
    out
}

fn details(view: &Value, env: &Env) -> Vec<Detail> {
    let mut out = vec![Detail {
        label: t::consent::card_label_who().to_owned(),
        value: who(view),
    }];
    let mut add = |label: &str, value: Option<String>| {
        if let Some(value) = value.filter(|v| !v.is_empty()) {
            out.push(Detail {
                label: label.to_owned(),
                value,
            });
        }
    };
    add(t::consent::card_label_node(), env.node.clone());
    add(
        t::consent::card_label_folder(),
        text(view, "folder").map(str::to_owned),
    );
    add(
        t::consent::card_label_organization(),
        text(view, "organization").map(str::to_owned),
    );
    add(
        t::consent::card_label_scope(),
        Some(t::consent::card_scope_once().to_owned()),
    );
    out
}

fn says(view: &Value) -> Option<Untrusted> {
    text(view, "reason").map(|reason| Untrusted {
        label: t::consent::card_untrusted_label().to_owned(),
        note: Some(t::consent::card_untrusted_note().to_owned()),
        text: reason.to_owned(),
    })
}

fn new_value(view: &Value) -> Option<Untrusted> {
    if !is_setting(view) || !is_text(view) {
        return None;
    }
    Some(Untrusted {
        label: t::consent::card_new_value_label().to_owned(),
        note: None,
        text: shown(view.get("change").and_then(|c| c.get("after"))),
    })
}

pub fn vault_moved(seen: Option<(u64, bool)>, now: (u64, bool)) -> bool {
    seen.is_some_and(|seen| seen != now)
}

fn seconds_left(left_ms: i64) -> f64 {
    ((left_ms + 999) / 1000) as f64
}

impl ConsentCard {
    pub fn build(view: &Value, env: &Env) -> Option<ConsentCard> {
        let id = view.get("id").and_then(Value::as_str)?.to_owned();
        let open = matches!(
            view.get("state").and_then(Value::as_str),
            Some("waiting" | "opened")
        );
        let expires = view.get("expiresMs").and_then(Value::as_i64)?;
        let left = expires - env.now_ms;
        let ended = left <= 0;
        let expiry = if ended {
            t::consent::card_ended().to_owned()
        } else {
            t::consent::card_expires_in(&duration(seconds_left(left)))
        };
        let secret = view.get("kind").and_then(Value::as_str) == Some(SECRET_REQUEST);
        let blocked = (secret && !env.vault_on).then(|| t::consent::card_vault_off().to_owned());
        let allow = if !open || ended || blocked.is_some() {
            Allow::Off
        } else if env.shown_ms < ALLOW_DELAY_MS {
            Allow::Wait(ALLOW_DELAY_MS - env.shown_ms.max(0))
        } else {
            Allow::Ready
        };
        Some(ConsentCard {
            id,
            facts: facts(view),
            details: details(view, env),
            says: says(view),
            value: new_value(view),
            expiry,
            ended,
            blocked,
            allow,
            answerable: open && !ended,
            app_applied: view.get("appliedBy").and_then(Value::as_str) == Some("app"),
            switch: switch_of(view),
        })
    }
}

#[cfg(test)]
#[path = "consent_card_tests.rs"]
mod tests;
