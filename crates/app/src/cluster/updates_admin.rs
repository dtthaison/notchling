use std::collections::BTreeMap;

use notchling_engine::accounts::Secret;
use notchling_engine::machines::engine::SetupEngine;
use notchling_engine::machines::registry::{AdminAccess, Machine};
use notchling_engine::vault::entry::{valid_name, Kind, VaultEntry, VaultValue};
use notchling_locale as t;

use crate::switches::Switches;
use crate::vault::store::Vault;

const NAME_MAX: usize = 64;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum VaultState {
    #[default]
    Reading,
    Unavailable,
    Items(Vec<String>),
}

impl VaultState {
    pub fn from_result(result: &Result<Vec<VaultEntry>, String>) -> VaultState {
        match result {
            Ok(entries) => VaultState::Items(entries.iter().map(|e| e.name.clone()).collect()),
            Err(_) => VaultState::Unavailable,
        }
    }

    fn has(&self, name: &str) -> bool {
        match self {
            VaultState::Items(items) => items.iter().any(|i| i.eq_ignore_ascii_case(name)),
            _ => false,
        }
    }

    fn taken(&self) -> &[String] {
        match self {
            VaultState::Items(items) => items,
            _ => &[],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Need {
    pub machine: String,
    pub user: String,
    pub host: String,
    pub link: Option<String>,
}

pub fn needs(machines: &[Machine], admin_machines: &[String]) -> Vec<Need> {
    admin_machines
        .iter()
        .filter_map(|name| machines.iter().find(|m| &m.name == name))
        .filter(|m| m.admin() == AdminAccess::Sudo)
        .map(|m| Need {
            machine: m.name.clone(),
            user: m.user.clone(),
            host: m.host.clone(),
            link: m.admin_secret().map(str::to_owned),
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub gone: Option<String>,
    pub saves_as: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Linked(String),
    Ask(Ask),
}

pub fn item_name(machine: &str, taken: &[String]) -> String {
    let mut base: String = machine
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
                c
            } else {
                '-'
            }
        })
        .take(NAME_MAX - 16)
        .collect();
    if !base.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        base.insert_str(0, "machine-");
    }
    let first = format!("{base}-admin");
    let is_taken = |name: &str| taken.iter().any(|t| t.eq_ignore_ascii_case(name));
    if !is_taken(&first) {
        return first;
    }
    (2..)
        .map(|n| format!("{first}-{n}"))
        .find(|name| !is_taken(name))
        .unwrap_or(first)
}

pub fn source(need: &Need, vault: &VaultState) -> Source {
    let saves_as = match vault {
        VaultState::Unavailable => None,
        _ => Some(item_name(&need.machine, vault.taken())),
    };
    match (&need.link, vault) {
        (Some(link), VaultState::Reading) => Source::Linked(link.clone()),
        (Some(link), v) if v.has(link) => Source::Linked(link.clone()),
        (Some(link), VaultState::Items(_)) => Source::Ask(Ask {
            gone: Some(link.clone()),
            saves_as,
        }),
        _ => Source::Ask(Ask {
            gone: None,
            saves_as,
        }),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Blocked {
    Reading,
    NeedsPassword(Vec<String>),
}

impl Blocked {
    pub fn reason(&self) -> Option<String> {
        match self {
            Blocked::Reading => None,
            Blocked::NeedsPassword(names) => {
                Some(t::cluster::update_needs_password(&names.join(", ")))
            }
        }
    }
}

pub fn blocked(
    sources: &[(Need, Source)],
    typed: &dyn Fn(&str) -> bool,
    vault: &VaultState,
) -> Option<Blocked> {
    let missing: Vec<String> = sources
        .iter()
        .filter(|(need, source)| matches!(source, Source::Ask(_)) && !typed(&need.machine))
        .map(|(need, _)| need.machine.clone())
        .collect();
    if !missing.is_empty() {
        return Some(Blocked::NeedsPassword(missing));
    }
    (*vault == VaultState::Reading && !sources.is_empty()).then_some(Blocked::Reading)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Save {
    pub machine: String,
    pub user: String,
    pub host: String,
    pub item: String,
    secret: Secret,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Prepared {
    pub typed: BTreeMap<String, Secret>,
    pub saves: Vec<Save>,
    pub unlink: Vec<String>,
}

pub fn prepare(sources: &[(Need, Source)], mut passwords: BTreeMap<String, Secret>) -> Prepared {
    let mut out = Prepared::default();
    for (need, source) in sources {
        let Source::Ask(ask) = source else {
            continue;
        };
        let Some(secret) = passwords.remove(&need.machine).filter(|s| !s.is_empty()) else {
            continue;
        };
        match &ask.saves_as {
            Some(item) => out.saves.push(Save {
                machine: need.machine.clone(),
                user: need.user.clone(),
                host: need.host.clone(),
                item: item.clone(),
                secret,
            }),
            None => {
                if need.link.is_some() {
                    out.unlink.push(need.machine.clone());
                }
                out.typed.insert(need.machine.clone(), secret);
            }
        }
    }
    out
}

pub fn admin_entry(save: &Save) -> (VaultEntry, VaultValue) {
    let mut entry = VaultEntry::new(&save.item, Kind::Login);
    entry.note = t::cluster::update_item_note(&save.machine);
    entry.url = Some(save.host.clone()).filter(|h| !h.is_empty());
    let value = VaultValue {
        username: save.user.clone(),
        secret: save.secret.expose().to_owned(),
    };
    (entry, value)
}

pub trait AdminLinks {
    fn link(&self, machine: &str, item: Option<&str>) -> Result<(), String>;
}

pub trait ItemSaver {
    fn save(&self, entry: &VaultEntry, value: &VaultValue) -> Result<(), String>;
}

impl AdminLinks for SetupEngine {
    fn link(&self, machine: &str, item: Option<&str>) -> Result<(), String> {
        self.set_admin_secret(machine, item)
    }
}

pub struct VaultSaver<'a> {
    pub vault: &'a Vault,
    pub switches: &'a Switches,
}

impl ItemSaver for VaultSaver<'_> {
    fn save(&self, entry: &VaultEntry, value: &VaultValue) -> Result<(), String> {
        self.vault
            .save(self.switches, entry, Some(value), None, None)
    }
}

pub fn apply(
    prepared: Prepared,
    links: &dyn AdminLinks,
    saver: &dyn ItemSaver,
) -> Result<BTreeMap<String, Secret>, String> {
    let Prepared {
        typed,
        saves,
        unlink,
    } = prepared;
    for save in &saves {
        if !valid_name(&save.item) {
            return Err(t::cluster::update_save_failed(&save.item));
        }
        let (entry, value) = admin_entry(save);
        saver
            .save(&entry, &value)
            .map_err(|why| t::cluster::update_save_failed(&why))?;
        links
            .link(&save.machine, Some(&save.item))
            .map_err(|why| t::cluster::update_save_failed(&why))?;
    }
    for machine in &unlink {
        links.link(machine, None)?;
    }
    Ok(typed)
}

#[cfg(test)]
#[path = "updates_admin_tests.rs"]
mod tests;
