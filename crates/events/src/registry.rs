use crate::kinds::{
    accounts, agents, diagnostics, inbox, performance, phone, secrets, security, security_log,
    settings,
};
use crate::spec::KindSpec;

pub const OWNER_KINDS: &[&str] = &[
    "api.refused",
    "api.unknown_peer",
    "switch.changed",
    "daemon.started",
    "daemon.stopped",
    "app.attached",
    "app.detached",
    "audit.checked",
    "audit.chain_started",
    "audit.pruned",
    "vault.secret_used",
    "vault.secret_requested",
    "phone.paired",
    "phone.connected",
    "phone.acted",
    "phone.answered",
    "settings.agent",
    "log.setting_changed",
];

pub const COMMIT_KINDS: &[&str] = &[
    "vault.secret_used",
    "vault.secret_requested",
    "settings.agent",
    "session.reply",
    "session.control",
];

const LISTS: [&[&KindSpec]; 10] = [
    security::KINDS,
    security_log::KINDS,
    secrets::KINDS,
    agents::KINDS,
    inbox::KINDS,
    accounts::KINDS,
    phone::KINDS,
    settings::KINDS,
    diagnostics::KINDS,
    performance::KINDS,
];

pub fn all() -> Vec<&'static KindSpec> {
    LISTS.iter().flat_map(|list| list.iter().copied()).collect()
}

pub fn find(name: &str) -> Option<&'static KindSpec> {
    LISTS
        .iter()
        .flat_map(|list| list.iter().copied())
        .find(|spec| spec.name == name)
}
