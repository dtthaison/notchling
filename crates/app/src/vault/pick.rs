use notchling_engine::vault::entry::{Kind, VaultEntry};
use notchling_locale as t;

use super::store::Vault;
use crate::switches::{Switch, Switches};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickRow {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub checked: bool,
    pub gone: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickNote {
    Reading,
    Empty,
    Failed(String),
}

impl PickNote {
    pub fn text(&self) -> String {
        match self {
            PickNote::Reading => t::cluster::vault_reading().to_owned(),
            PickNote::Empty => t::cluster::vault_empty().to_owned(),
            PickNote::Failed(error) => t::cluster::vault_read_failed(error),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Load {
    #[default]
    Idle,
    Reading,
    Ready(Vec<VaultEntry>),
    Failed(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VaultPick {
    open: bool,
    load: Load,
}

fn detail_of(entry: &VaultEntry) -> String {
    let kind = match entry.kind {
        Kind::Login => t::cluster::vault_kind_login(),
        Kind::Token => t::cluster::vault_kind_token(),
    };
    match entry.host() {
        Some(host) => format!("{kind} · {host}"),
        None => kind.to_owned(),
    }
}

impl VaultPick {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn toggle(&mut self) -> bool {
        self.open = !self.open;
        if self.open {
            self.load = Load::Reading;
        }
        self.open
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn reread(&mut self) {
        self.load = Load::Reading;
    }

    pub fn loaded(&mut self, result: Result<Vec<VaultEntry>, String>) {
        if !self.open {
            return;
        }
        self.load = match result {
            Ok(entries) => Load::Ready(entries),
            Err(why) => Load::Failed(why),
        };
    }

    pub fn note(&self) -> Option<PickNote> {
        match &self.load {
            Load::Idle | Load::Reading => Some(PickNote::Reading),
            Load::Ready(entries) if entries.is_empty() => Some(PickNote::Empty),
            Load::Ready(_) => None,
            Load::Failed(why) => Some(PickNote::Failed(why.clone())),
        }
    }

    pub fn rows(&self, chosen: Option<&str>) -> Vec<PickRow> {
        let chosen = chosen.map(str::to_lowercase);
        let mut rows: Vec<PickRow> = match &self.load {
            Load::Ready(entries) => entries
                .iter()
                .map(|e| PickRow {
                    id: e.id(),
                    name: e.name.clone(),
                    detail: detail_of(e),
                    checked: chosen.as_deref() == Some(e.id().as_str()),
                    gone: false,
                })
                .collect(),
            _ => Vec::new(),
        };
        rows.sort_by_key(|r| r.name.to_lowercase());
        if let (Some(id), Load::Ready(_)) = (&chosen, &self.load) {
            if !rows.iter().any(|r| &r.id == id) {
                rows.push(PickRow {
                    id: id.clone(),
                    name: id.clone(),
                    detail: t::cluster::vault_gone().to_owned(),
                    checked: true,
                    gone: true,
                });
            }
        }
        rows
    }
}

pub fn using_line(chosen: &str) -> String {
    t::cluster::vault_using(chosen)
}

pub fn local_entries(local: &Vault, switches: &Switches) -> Result<Vec<VaultEntry>, String> {
    if let Some(why) = switches.message(Switch::Vault) {
        return Err(why.to_owned());
    }
    local.refresh(switches);
    let snapshot = local.snapshot();
    match snapshot.error {
        Some(error) => Err(error),
        None if !snapshot.attached => Err(crate::acting::DETACHED_MESSAGE.to_owned()),
        None => Ok(snapshot.entries),
    }
}

#[cfg(test)]
#[path = "pick_tests.rs"]
mod tests;
