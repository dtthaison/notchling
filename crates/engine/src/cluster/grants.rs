use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::jobs::{Job, JobSpec, JobState};
use super::members::{Member, MemberState};
use crate::vault::entry::valid_name;

pub const GRANT_TTL_MS: i64 = 12 * 3_600_000;
pub const MAX_ALLOWED_NAMES: usize = 256;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(from = "RawAllow")]
pub struct SecretAllow {
    pub names: BTreeSet<String>,
    pub any: bool,
}

#[derive(Deserialize)]
struct RawAllow {
    #[serde(default)]
    names: BTreeSet<String>,
    #[serde(default)]
    any: bool,
}

impl From<RawAllow> for SecretAllow {
    fn from(raw: RawAllow) -> SecretAllow {
        let names = raw
            .names
            .into_iter()
            .map(|n| n.to_lowercase())
            .filter(|n| valid_name(n))
            .take(MAX_ALLOWED_NAMES);
        SecretAllow::new(names, raw.any)
    }
}

impl SecretAllow {
    pub fn new(names: impl IntoIterator<Item = String>, any: bool) -> SecretAllow {
        SecretAllow {
            names: names.into_iter().map(|n| n.to_lowercase()).collect(),
            any,
        }
    }

    pub fn check(&self) -> Result<(), String> {
        if self.names.len() > MAX_ALLOWED_NAMES {
            return Err(format!("at most {MAX_ALLOWED_NAMES} names"));
        }
        match self.names.iter().find(|n| !valid_name(n)) {
            Some(name) => Err(format!("{} isn't a valid secret name", shown(name))),
            None => Ok(()),
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
    if !valid_name(a.name) {
        return GrantDecision::Refuse(format!("{} isn't a valid secret name", shown(a.name)));
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
    for name in &spec.secrets {
        if !valid_name(name) {
            return Err(format!("{} isn't a valid secret name", shown(name)));
        }
        if !allow.allows(name) {
            return Err(format!("{name} isn't allowed on that machine"));
        }
    }
    Ok(())
}

fn shown(name: &str) -> String {
    name.chars().take(64).collect()
}
