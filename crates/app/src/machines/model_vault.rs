//! The Secret Vault as the Machines window sees it (Swift `MachinesModel`'s vault half): the update
//! offers of renewed values, the Rename banner of legacy names, Renew….

use std::collections::BTreeSet;
use std::sync::Arc;

use notchling_engine::machines::forms::rename_note;
use notchling_engine::machines::steps_secret::pending_renames;

use super::model::{MachinesModel, VaultList};
use super::pieces::UpdateOfferRow;

impl MachinesModel {
    // MARK: Renewed secrets, renames

    /// Every tick: when the vault's list changed on disk, each name saved since it was last looked
    /// at (a new name, or a new `updated`) makes its update offer, and the names are read again
    /// (Swift: the `vaultValueChanged` notification). Only the list is read, never a value, and
    /// nothing is sent. Whether anything changed.
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

    /// Takes the list as it is now as seen, without an offer (the window opening, a Rename).
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

    /// The Rename button: what to move, for the background (the vault's value moves through the
    /// Keychain, never on the UI thread). `None` while a rename runs or when nothing is left.
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

    /// A rename ended: the vault's names again, and a line when one didn't move. The moved names
    /// hold the values the machines have already, so they make no update offer.
    pub fn renamed(&mut self, failed: bool) {
        self.renaming = false;
        self.refresh_vault();
        self.rebase_vault();
        self.rename_note = failed.then(|| "Rename didn't finish; try again".to_owned());
    }

    /// Renew…: the secret's page (GitHub's token page for a GitHub secret), opened in the
    /// background.
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

    /// Renew… ended: a line at the bottom when the page didn't open.
    pub fn renewed(&mut self, result: Result<(), String>) {
        self.notice = result.err();
    }
}

/// The Rename button's moves, run off the UI thread.
pub struct RenameWork {
    vault: Arc<dyn VaultList>,
    pub moves: Vec<(String, String)>,
}

impl RenameWork {
    /// Moves each legacy secret to its new name, keeping its value (blocking). Whether one failed.
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

/// Renew…'s page, opened off the UI thread.
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
