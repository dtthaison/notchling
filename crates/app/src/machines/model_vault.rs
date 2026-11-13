use std::collections::BTreeSet;
use std::sync::Arc;

use notchling_engine::machines::forms::rename_note;
use notchling_engine::machines::steps_secret::pending_renames;

use super::model::{MachinesModel, VaultList};
use super::pieces::UpdateOfferRow;

impl MachinesModel {
    pub fn watch_vault(&mut self) -> bool {
        if self.renaming {
            return false;
        }
        let stamp = self.parts.vault.stamp();
        if stamp == self.vault_stamp {
            return false;
        }
        self.vault_stamp = stamp;
        self.refresh_vault();
        for entry in &self.vault_entries {
            let seen = self.vault_seen.insert(entry.id(), entry.updated.clone());
            if seen.as_ref() != Some(&entry.updated) {
                self.parts.engine.vault_value_changed(&entry.name);
            }
        }
        let ids: BTreeSet<String> = self.vault_entries.iter().map(|e| e.id()).collect();
        self.vault_seen.retain(|id, _| ids.contains(id));
        true
    }

    pub(super) fn rebase_vault(&mut self) {
        self.vault_stamp = self.parts.vault.stamp();
        self.vault_seen = self
            .vault_entries
            .iter()
            .map(|e| (e.id(), e.updated.clone()))
            .collect();
    }

    pub fn update_offers(&self) -> Vec<UpdateOfferRow> {
        self.parts
            .engine
            .update_offers()
            .into_iter()
            .map(|(secret, usages)| UpdateOfferRow {
                secret,
                count: usages.len(),
            })
            .collect()
    }

    pub fn rename_banner(&self) -> Option<String> {
        let names: Vec<String> = pending_renames(&self.vault_entries)
            .into_iter()
            .map(|(e, _)| e.name)
            .collect();
        rename_note(&names)
    }

    pub fn rename_work(&mut self) -> Option<RenameWork> {
        if self.renaming {
            return None;
        }
        let moves: Vec<(String, String)> = pending_renames(&self.vault_entries)
            .into_iter()
            .map(|(entry, name)| (entry.name, name))
            .collect();
        if moves.is_empty() {
            return None;
        }
        self.renaming = true;
        Some(RenameWork {
            vault: self.parts.vault.clone(),
            moves,
        })
    }

    pub fn renamed(&mut self, failed: bool) {
        self.renaming = false;
        self.refresh_vault();
        self.rebase_vault();
        self.rename_note = failed.then(|| "Rename didn't finish; try again".to_owned());
    }

    pub fn renew_work(&self, name: &str) -> RenewWork {
        let url = self
            .vault_entries
            .iter()
            .find(|e| e.id() == name.to_lowercase())
            .and_then(|e| e.url.clone());
        RenewWork {
            vault: self.parts.vault.clone(),
            name: name.to_owned(),
            url,
        }
    }

    pub fn renewed(&mut self, result: Result<(), String>) {
        self.notice = result.err();
    }
}

pub struct RenameWork {
    vault: Arc<dyn VaultList>,
    pub moves: Vec<(String, String)>,
}

impl RenameWork {
    pub fn run(&self) -> bool {
        let mut failed = false;
        for (old, new) in &self.moves {
            if self.vault.rename(old, new).is_err() {
                failed = true;
            }
        }
        failed
    }
}

pub struct RenewWork {
    vault: Arc<dyn VaultList>,
    name: String,
    url: Option<String>,
}

impl RenewWork {
    pub fn run(&self) -> Result<(), String> {
        self.vault.renew(&self.name, self.url.as_deref())
    }
}
