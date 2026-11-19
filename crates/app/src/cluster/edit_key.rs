use std::path::Path;

use notchling_engine::machines::addresses::{MachineAddress, Route};
use notchling_engine::machines::registry::Machine;
use notchling_engine::machines::ssh_key::{inspect, shape_ok, tidy, KeyIssue, KeyStatus};
use notchling_engine::machines::ssh_test::SshTest;

use super::add::commit;
use super::add_parts::AddParts;
use super::addresses_form::RowDraft;
use notchling_locale as t;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditResult {
    pub address: String,
    pub result: SshTest,
    pub route: Option<Route>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditTest {
    Idle,
    Running(u64),
    Done(u64, Vec<EditResult>),
}

pub fn signature(drafts: &[RowDraft], key: &str) -> String {
    format!("{drafts:?}|{}", key.trim())
}

pub fn key_text(text: &str, home: &Path) -> Result<Option<String>, KeyIssue> {
    let typed = text.trim();
    if typed.is_empty() {
        return Ok(None);
    }
    let key = tidy(typed, home);
    if shape_ok(&key) {
        Ok(Some(key))
    } else {
        Err(KeyIssue::BadShape)
    }
}

pub fn key_of(text: &str, home: &Path) -> Result<(Option<String>, KeyStatus), KeyIssue> {
    let typed = text.trim();
    if typed.is_empty() {
        return Ok((
            None,
            KeyStatus {
                issue: None,
                public: false,
            },
        ));
    }
    let key = tidy(typed, home);
    let status = inspect(&key, home);
    match status.issue {
        Some(issue) => Err(issue),
        None => Ok((Some(key), status)),
    }
}

pub fn draft_machine(
    machine: &Machine,
    addresses: Vec<MachineAddress>,
    key: Option<&str>,
) -> Machine {
    let mut draft = machine.clone();
    draft.set_addresses(addresses);
    draft.set_ssh_key(key);
    draft
}

pub fn save_edit(
    parts: &AddParts,
    name: &str,
    addresses: Vec<MachineAddress>,
    key: Option<&str>,
) -> Result<(), String> {
    commit(&*parts.registry, |list| {
        let Some(machine) = list.iter_mut().find(|m| m.name == name) else {
            return Err(t::cluster::address_edit_gone(name));
        };
        let same_key = machine.ssh_key() == key.map(str::trim).filter(|k| !k.is_empty());
        if machine.addresses == addresses && same_key {
            return Ok(false);
        }
        machine.set_addresses(addresses);
        machine.set_ssh_key(key);
        Ok(true)
    })?;
    parts.engine.router().forget(name);
    Ok(())
}
