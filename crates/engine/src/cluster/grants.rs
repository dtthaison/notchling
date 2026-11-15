use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::jobs::{Job, JobSpec, JobState};
use super::members::{Member, MemberState};

pub const GRANT_TTL_MS: i64 = 12 * 3_600_000;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct SecretAllow {
    pub names: BTreeSet<String>,
    pub any: bool,
}

impl SecretAllow {
    pub fn new(names: impl IntoIterator<Item = String>, any: bool) -> SecretAllow {
        SecretAllow {
            names: names.into_iter().map(|n| n.to_lowercase()).collect(),
            any,
        }
    }

    pub fn allows(&self, name: &str) -> bool {
        self.any || self.names.contains(&name.to_lowercase())
    }
}

pub struct GrantAsk<'a> {
    pub node: &'a Member,
    pub job: &'a Job,
    pub name: &'a str,
    pub exists: bool,
    pub asks: bool,
    pub vault_is_primary: bool,
    pub vault_name: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrantDecision {
    Release,
    Ask,
    Wait(String),
    Refuse(String),
}

pub fn decide(a: &GrantAsk) -> GrantDecision {
    if !matches!(a.job.state, JobState::Running | JobState::Dispatching) {
        return GrantDecision::Refuse(format!("job {} isn't running", a.job.id));
    }
    if a.node.state != MemberState::Active {
        return GrantDecision::Refuse(format!("{} isn't an active member", a.node.name));
    }
    if !a.node.secrets.allows(a.name) {
        return GrantDecision::Refuse(format!("{} isn't allowed on {}", a.name, a.node.name));
    }
    if !a.vault_is_primary {
        return GrantDecision::Wait(format!(
            "{} waits for {}: the vault is there",
            a.name, a.vault_name
        ));
    }
    if !a.exists {
        return GrantDecision::Refuse(format!(
            "No secret named {} in the vault on {}",
            a.name, a.vault_name
        ));
    }
    if a.asks {
        GrantDecision::Ask
    } else {
        GrantDecision::Release
    }
}

pub fn expires_ms(released_ms: i64) -> i64 {
    released_ms.saturating_add(GRANT_TTL_MS)
}

pub fn attachable(spec: &JobSpec, allow: &SecretAllow) -> Result<(), String> {
    match spec.secrets.iter().find(|name| !allow.allows(name)) {
        Some(name) => Err(format!("{name} isn't allowed on that machine")),
        None => Ok(()),
    }
}
