use crate::limits_ops::{COST_SUMMARY, LIMITS_SET_MODE, LIMITS_STATUS};
use crate::limits_usage::LIMITS_USAGE;
use notchling_local_api::{ops, OpHome};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Home {
    Kind(&'static str),
    Called,
    Quiet,
}

pub const HOMES: &[(&str, Home)] = &[
    (ops::HELLO, Home::Kind("app.attached")),
    (ops::DAEMON_STATUS, Home::Quiet),
    (ops::SESSIONS_LIST, Home::Quiet),
    (ops::EVENTS_SUBSCRIBE, Home::Called),
    (ops::SESSION_REPLY, Home::Kind("session.reply")),
    (ops::SESSION_ANSWER, Home::Kind("session.answered")),
    (ops::SESSION_PERMISSION, Home::Kind("session.answered")),
    (ops::SESSION_INTERRUPT, Home::Kind("session.control")),
    (ops::SESSION_STOP, Home::Kind("session.control")),
    (ops::SESSION_REMOVE, Home::Kind("session.control")),
    (ops::SESSION_TAKE_BACK, Home::Kind("session.control")),
    (ops::SESSION_NEW, Home::Kind("session.control")),
    (ops::FOLDER_TRUST, Home::Kind("session.control")),
    (ops::SESSION_MOVE, Home::Kind("session.control")),
    (ops::SESSION_COMMAND, Home::Kind("session.control")),
    (ops::SETTINGS_SET_ACTING, Home::Kind("switch.changed")),
    (ops::SETTINGS_GET_ACTING, Home::Quiet),
    (ops::PROMPTS_LIST, Home::Quiet),
    (ops::SESSION_QUESTION, Home::Called),
    (ops::HOOK_REQUEST, Home::Called),
    (ops::CODEX_SEND, Home::Called),
    (ops::CODEX_STOP, Home::Called),
    (ops::CODEX_DELETE, Home::Called),
    (ops::AGY_SESSIONS, Home::Quiet),
    (ops::AGY_STATE, Home::Quiet),
    (ops::AGY_CONVERSATION, Home::Quiet),
    (ops::AGY_OPEN, Home::Called),
    (ops::AGY_SEND, Home::Called),
    (ops::AGY_CANCEL, Home::Called),
    (ops::AGY_SET_OPTION, Home::Called),
    (ops::AGY_PERMISSION, Home::Called),
    (ops::BRIDGE_SET, Home::Kind("switch.changed")),
    (ops::BRIDGE_STATUS, Home::Quiet),
    (ops::BRIDGE_PAIR_BEGIN, Home::Called),
    (ops::BRIDGE_PAIR_ANSWER, Home::Called),
    (ops::BRIDGE_DEVICES, Home::Quiet),
    (ops::BRIDGE_REVOKE, Home::Called),
    (ops::BRIDGE_TUNNEL_STATUS, Home::Quiet),
    (ops::SEARCH_QUERY, Home::Quiet),
    (ops::PAGES_LIST, Home::Quiet),
    (ops::PAGES_FIND, Home::Quiet),
    (ops::INSTALL_PLAN, Home::Quiet),
    (ops::INSTALL_APPLY, Home::Kind("install.hook")),
    (ops::INSTALL_REMOVE, Home::Kind("install.hook")),
    (ops::SETTINGS_SET_SWITCH, Home::Kind("switch.changed")),
    (ops::SETTINGS_SWITCHES, Home::Quiet),
    (ops::SETTINGS_LIST, Home::Quiet),
    (ops::SETTINGS_GET, Home::Quiet),
    (ops::SETTINGS_SET, Home::Kind("settings.agent")),
    (ops::SETTINGS_ASKED, Home::Called),
    (ops::ACCOUNTS_LIST, Home::Quiet),
    (ops::ACCOUNTS_SWITCH, Home::Kind("account.switched")),
    (ops::ACCOUNTS_ADD_BEGIN, Home::Called),
    (ops::ACCOUNTS_ADD_FINISH, Home::Kind("account.login")),
    (ops::ACCOUNTS_REMOVE, Home::Kind("account.login")),
    (ops::ACCOUNTS_MOVE_ALL, Home::Kind("account.switched")),
    (ops::ACCOUNTS_RESULT, Home::Called),
    (ops::LANES_LIST, Home::Quiet),
    (ops::LANES_CREATE, Home::Kind("lane.changed")),
    (ops::LANES_REMOVE, Home::Kind("lane.changed")),
    (ops::LANES_SWITCH, Home::Kind("lane.changed")),
    (ops::PROVIDERS_LIST, Home::Quiet),
    (ops::PROVIDERS_SWITCH, Home::Called),
    (ops::PROVIDERS_ADD_BEGIN, Home::Called),
    (ops::PROVIDERS_ADD_CANCEL, Home::Called),
    (ops::PROVIDERS_REMOVE, Home::Called),
    (ops::PROVIDERS_USAGE, Home::Quiet),
    (ops::VAULT_FIND, Home::Quiet),
    (ops::VAULT_USE, Home::Kind("vault.secret_used")),
    (ops::VAULT_REQUEST, Home::Kind("vault.secret_requested")),
    (ops::VAULT_LIST, Home::Quiet),
    (ops::VAULT_SAVE, Home::Kind("vault.entry_changed")),
    (ops::VAULT_REMOVE, Home::Kind("vault.entry_changed")),
    (ops::VAULT_APPROVAL_ANSWER, Home::Kind("inbox.closed")),
    (ops::VAULT_REQUEST_CANCEL, Home::Kind("inbox.closed")),
    (ops::VAULT_PENDING, Home::Quiet),
    (ops::CATALOG_GET, Home::Quiet),
    (ops::INBOX_LIST, Home::Quiet),
    (ops::INBOX_ANSWER, Home::Kind("inbox.closed")),
    (ops::INBOX_SEEN, Home::Called),
    (ops::GIT_STATUS, Home::Quiet),
    (ops::GIT_DIFF, Home::Quiet),
    (ops::GIT_BRANCH_DIFF_STAT, Home::Quiet),
    (ops::GIT_BRANCH_DIFF_FILE, Home::Quiet),
    (ops::GIT_BRANCHES, Home::Quiet),
    (ops::ACTIVITY_LIST, Home::Quiet),
    (ops::VERSIONS_GET, Home::Called),
    (ops::VERSIONS_WORKERS, Home::Quiet),
    (ops::AWAKE_STATE, Home::Quiet),
    (ops::CODEX_CHILDREN, Home::Quiet),
    (ops::LOG_VERIFY, Home::Quiet),
    (ops::LOG_CHAIN_START, Home::Kind("audit.chain_started")),
    (ops::LOG_AUDIT_DELETE, Home::Kind("audit.pruned")),
    (LIMITS_STATUS, Home::Quiet),
    (LIMITS_USAGE, Home::Quiet),
    (LIMITS_SET_MODE, Home::Called),
    (COST_SUMMARY, Home::Quiet),
];

pub fn home(op: &str) -> OpHome {
    match HOMES.iter().find(|(name, _)| *name == op) {
        Some((_, Home::Kind(_))) => OpHome::Kind,
        Some((_, Home::Quiet)) => OpHome::Quiet,
        _ => OpHome::Called,
    }
}

pub fn known_ops() -> &'static [&'static str] {
    static NAMES: OnceLock<Vec<&'static str>> = OnceLock::new();
    NAMES.get_or_init(|| HOMES.iter().map(|(name, _)| *name).collect())
}

#[cfg(test)]
#[path = "op_kinds_tests.rs"]
mod tests;
