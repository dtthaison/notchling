use notchling_locale as t;
use serde_json::Value;

use super::consent_card::{text, tool_name, SESSION_CHARS};

pub(super) fn setting_word(word: &str) -> String {
    match word {
        "on" => t::consent::card_word_on().to_owned(),
        "off" => t::consent::card_word_off().to_owned(),
        "master" => t::consent::card_word_master().to_owned(),
        "worker" => t::consent::card_word_worker().to_owned(),
        other => other.to_owned(),
    }
}

fn change_line(args: &Value, words: fn(&str, &str) -> String) -> Option<String> {
    Some(words(
        &setting_word(text(args, "before")?),
        &setting_word(text(args, "after")?),
    ))
}

pub(super) fn cluster_line(line: &str, args: &Value) -> Option<String> {
    Some(match line {
        "consent-cluster" => t::consent::card_fact_cluster(text(args, "cluster")?),
        "consent-cluster-machine" => {
            let (machine, organization) = (text(args, "machine")?, text(args, "organization")?);
            match text(args, "address") {
                Some(address) => {
                    t::consent::card_fact_cluster_machine(machine, address, organization)
                }
                None => t::consent::card_fact_cluster_machine_no_address(machine, organization),
            }
        }
        "consent-cluster-create" => {
            t::consent::card_fact_cluster_create(text(args, "name")?, text(args, "organization")?)
        }
        "consent-cluster-token" => t::consent::card_fact_cluster_token().to_owned(),
        "consent-cluster-invite" => t::consent::card_fact_cluster_invite(
            text(args, "name")?,
            text(args, "node")?,
            &setting_word(text(args, "role")?),
            &setting_word(text(args, "acting")?),
        ),
        "consent-cluster-role" => change_line(args, t::consent::card_fact_cluster_role)?,
        "consent-cluster-delegation" => {
            change_line(args, t::consent::card_fact_cluster_delegation)?
        }
        "consent-cluster-failover" => change_line(args, t::consent::card_fact_cluster_failover)?,
        "consent-cluster-main" => {
            let session: String = text(args, "session")?.chars().take(SESSION_CHARS).collect();
            t::consent::card_fact_cluster_main(
                &session,
                &setting_word(text(args, "before")?),
                &setting_word(text(args, "after")?),
            )
        }
        "consent-cluster-allow-secret" => {
            t::consent::card_fact_cluster_allow_secret(text(args, "name")?)
        }
        "consent-cluster-allow-none" => t::consent::card_fact_cluster_allow_none().to_owned(),
        "consent-cluster-remove" => t::consent::card_fact_cluster_remove(text(args, "machine")?),
        "consent-cluster-move-home" => {
            t::consent::card_fact_cluster_move_home(text(args, "machine")?)
        }
        "consent-cluster-move-now" => t::consent::card_fact_cluster_move_now().to_owned(),
        _ => return None,
    })
}

fn count(args: &Value, key: &str) -> Option<String> {
    let value = args.get(key)?;
    match value {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

pub(super) fn account_line(line: &str, args: &Value) -> Option<String> {
    let tool = || text(args, "tool").map(tool_name);
    Some(match line {
        "consent-account" => t::consent::card_fact_account(
            tool()?,
            text(args, "account")?,
            text(args, "organization")?,
        ),
        "consent-account-switch" => t::consent::card_fact_account_switch(
            tool()?,
            text(args, "account")?,
            text(args, "before")?,
        ),
        "consent-account-switch-first" => {
            t::consent::card_fact_account_switch_first(tool()?, text(args, "account")?)
        }
        "consent-account-switch-codex" => t::consent::card_fact_account_switch_codex().to_owned(),
        "consent-account-switch-antigravity" => {
            t::consent::card_fact_account_switch_antigravity().to_owned()
        }
        "consent-account-add" => {
            t::consent::card_fact_account_add(tool()?, text(args, "organization")?)
        }
        "consent-account-remove" => {
            t::consent::card_fact_account_remove(tool()?, text(args, "account")?)
        }
        "consent-account-move-all" => {
            t::consent::card_fact_account_move_all(text(args, "account")?, &count(args, "count")?)
        }
        "consent-account-signed-out" => {
            t::consent::card_fact_account_signed_out(&count(args, "count")?)
        }
        "consent-account-move-session" => t::consent::card_fact_account_move_session(
            text(args, "name")?,
            text(args, "folder")?,
            text(args, "account")?,
        ),
        "consent-lane-create" => t::consent::card_fact_lane_create(text(args, "account")?),
        "consent-lane-close" => t::consent::card_fact_lane_close(text(args, "account")?),
        "consent-lane-switch" => {
            t::consent::card_fact_lane_switch(text(args, "before")?, text(args, "account")?)
        }
        "consent-lane-sessions" => t::consent::card_fact_lane_sessions(&count(args, "count")?),
        _ => return None,
    })
}
