use std::collections::HashSet;

use notchling_engine::inbox::Place;
use notchling_locale as t;
use serde_json::Value;

use super::consent_family::{self, fact_line as family_line, Family, FamilyApply, Part};
use super::consent_lines::{account_line, cluster_line};
use crate::chat::format::duration;

pub const ALLOW_DELAY_MS: i64 = 1000;
const SECRET_REQUEST: &str = "secret_request";
const SWITCH: &str = "switch";
const CLUSTER: &str = "cluster";
const ACCOUNT: &str = "account";
pub(super) const SESSION_CHARS: usize = 8;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Env {
    pub now_ms: i64,
    pub vault_on: bool,
    pub shown_ms: i64,
    pub node: Option<String>,
    pub unticked: HashSet<String>,
    pub admin_ready: bool,
    pub place: Option<Place>,
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
    pub cluster_apply: Option<ClusterApply>,
    pub account_apply: Option<AccountApply>,
    pub family_apply: Option<FamilyApply>,
    pub parts: Vec<Part>,
    pub asked_again: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AccountApply {
    pub op: String,
    pub args: Value,
    pub action: String,
    pub tool: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClusterApply {
    pub op: String,
    pub args: Value,
}

pub(super) fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
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

pub(super) fn tool_name(tool: &str) -> &'static str {
    match tool {
        "claude" => t::consent::card_tool_claude(),
        "codex" => t::consent::card_tool_codex(),
        "antigravity" => t::consent::card_tool_antigravity(),
        _ => t::consent::card_tool_other(),
    }
}

fn job_tool(tool: &str) -> &'static str {
    match tool {
        "agy" => t::consent::card_tool_antigravity(),
        "shell" => t::consent::card_job_tool_shell(),
        other => tool_name(other),
    }
}

fn session_words(session: &str, place: Option<&Place>) -> String {
    let named = place.and_then(|place| {
        let name = place.session_name.as_deref()?;
        Some(match place.branch.as_deref() {
            Some(branch) => t::inbox::chip_session(name, branch),
            None => name.to_owned(),
        })
    });
    named.unwrap_or_else(|| session.chars().take(SESSION_CHARS).collect())
}

fn who(view: &Value, place: Option<&Place>) -> String {
    let tool = view.get("tool").and_then(Value::as_str).unwrap_or("");
    let verified = view.get("verified").and_then(Value::as_bool) == Some(true);
    match text(view, "session") {
        Some(session) => {
            let short = session_words(session, place);
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

pub(crate) fn switch_words(name: &str) -> Option<(&'static str, &'static str)> {
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
        "machines" => (
            t::consent::card_switch_machines(),
            t::consent::card_switch_machines_unlocks(),
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
        "consent-session-start" => {
            let tool = tool_name(text(args, "tool").unwrap_or(""));
            Some(t::consent::card_fact_session_start(
                tool,
                text(args, "folder")?,
            ))
        }
        "consent-session-say" => named(args, t::consent::card_fact_session_say),
        "consent-session-stop" => named(args, t::consent::card_fact_session_stop),
        "consent-session-interrupt" => named(args, t::consent::card_fact_session_interrupt),
        "consent-session-remove" => named(args, t::consent::card_fact_session_remove),
        "consent-session-resume" => named(args, t::consent::card_fact_session_resume),
        "consent-session-choose" => named(args, t::consent::card_fact_session_choose),
        "consent-session-run" => named(args, t::consent::card_fact_session_run),
        "consent-session-pick" => named(args, t::consent::card_fact_session_pick),
        "consent-session-dismiss" => named(args, t::consent::card_fact_session_dismiss),
        "consent-session-browse-on" => named(args, t::consent::card_fact_session_browse_on),
        "consent-session-browse-off" => named(args, t::consent::card_fact_session_browse_off),
        "consent-session-tune" => named(args, t::consent::card_fact_session_tune),
        "consent-session-model" => Some(t::consent::card_fact_session_model(text(args, "model")?)),
        "consent-session-trusts" => Some(t::consent::card_fact_session_trusts().to_owned()),
        "consent-session-child" => Some(t::consent::card_fact_session_child().to_owned()),
        "consent-job-dispatch" => Some(t::consent::card_fact_job_dispatch(
            job_tool(text(args, "tool").unwrap_or("")),
            text(args, "machine")?,
            text(args, "folder")?,
        )),
        "consent-job-machine-online" => Some(t::consent::card_fact_job_machine_online(
            text(args, "machine")?,
            text(args, "organization")?,
        )),
        "consent-job-machine-offline" => Some(t::consent::card_fact_job_machine_offline(
            text(args, "machine")?,
            text(args, "organization")?,
        )),
        "consent-job-model" => Some(t::consent::card_fact_job_model(text(args, "model")?)),
        "consent-job-secrets" => Some(t::consent::card_fact_job_secrets(text(args, "names")?)),
        "consent-job-tail" => job_line(args, t::consent::card_fact_job_tail),
        "consent-job-return" => job_line(args, t::consent::card_fact_job_return),
        "consent-job-stop" => job_line(args, t::consent::card_fact_job_stop),
        _ => cluster_line(line, args)
            .or_else(|| account_line(line, args))
            .or_else(|| family_line(line, args)),
    }
}

fn is_account(view: &Value) -> bool {
    view.get("kind").and_then(Value::as_str) == Some(ACCOUNT)
}

fn account_apply(view: &Value) -> Option<AccountApply> {
    if !is_account(view) {
        return None;
    }
    let change = view.get("change")?;
    let apply = change.get("apply")?;
    let args = apply.get("args").filter(|a| a.is_object())?.clone();
    Some(AccountApply {
        op: text(apply, "op")?.to_owned(),
        args,
        action: text(change, "action")?.to_owned(),
        tool: text(change, "tool")?.to_owned(),
    })
}

fn is_cluster(view: &Value) -> bool {
    view.get("kind").and_then(Value::as_str) == Some(CLUSTER)
}

fn cluster_apply(view: &Value) -> Option<ClusterApply> {
    if !is_cluster(view) {
        return None;
    }
    let apply = view.get("change")?.get("apply")?;
    let args = apply.get("args").filter(|a| a.is_object())?.clone();
    Some(ClusterApply {
        op: text(apply, "op")?.to_owned(),
        args,
    })
}

fn job_line(args: &Value, words: fn(&str, &str, &str) -> String) -> Option<String> {
    Some(words(
        text(args, "job")?,
        text(args, "machine")?,
        text(args, "folder")?,
    ))
}

fn named(args: &Value, words: fn(&str, &str) -> String) -> Option<String> {
    Some(words(text(args, "name")?, text(args, "folder")?))
}

fn is_setting(view: &Value) -> bool {
    view.get("kind").and_then(Value::as_str) == Some("setting")
}

fn is_session(view: &Value) -> bool {
    matches!(
        view.get("kind").and_then(Value::as_str),
        Some("session" | "session_stop" | "dispatch" | "job" | "job_stop")
    )
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
        value: who(view, env.place.as_ref()),
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
    let organization =
        if is_session(view) || is_cluster(view) || is_account(view) || Family::of(view).is_some() {
            view.get("change")
                .and_then(|change| text(change, "organization"))
                .or_else(|| text(view, "organization"))
        } else {
            text(view, "organization")
        };
    add(
        t::consent::card_label_organization(),
        organization.map(str::to_owned),
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
    if is_session(view) {
        let change = view.get("change")?;
        let message = text(change, "message")?;
        let label = match text(change, "action") {
            Some("choose") => t::consent::card_answers_label(),
            Some("run" | "browse") => t::consent::card_command_label(),
            Some("pick") => t::consent::card_dialog_label(),
            Some("tune") => t::consent::card_option_label(),
            _ => t::consent::card_message_label(),
        };
        return Some(Untrusted {
            label: label.to_owned(),
            note: Some(t::consent::card_untrusted_note().to_owned()),
            text: message.to_owned(),
        });
    }
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

fn family_block(apply: Option<&FamilyApply>, parts: &[Part], env: &Env) -> Option<String> {
    let apply = apply?;
    if apply.family.has_parts() && consent_family::kept_count(parts) == 0 {
        return Some(t::consent::install_none_kept().to_owned());
    }
    if apply.refused {
        return Some(t::consent::machine_admin_refused().to_owned());
    }
    (apply.admin && !env.admin_ready).then(|| t::consent::machine_password_needed().to_owned())
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
        let family_apply = consent_family::family_apply(view);
        let parts = consent_family::parts_of(view, &env.unticked);
        let blocked = blocked.or_else(|| family_block(family_apply.as_ref(), &parts, env));
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
            cluster_apply: cluster_apply(view),
            account_apply: account_apply(view),
            family_apply,
            parts,
            asked_again: view
                .get("change")
                .and_then(|change| text(change, "priorNote"))
                .map(str::to_owned),
        })
    }
}

#[cfg(test)]
#[path = "consent_card_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "consent_card_job_tests.rs"]
mod job_tests;

#[cfg(test)]
#[path = "consent_card_cluster_tests.rs"]
mod cluster_tests;

#[cfg(test)]
#[path = "consent_card_account_tests.rs"]
mod account_tests;

#[cfg(all(test, target_os = "macos"))]
#[path = "consent_card_family_tests.rs"]
mod family_tests;
