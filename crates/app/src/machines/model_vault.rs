//! The Secret Vault as the Machines window sees it (Swift `MachinesModel`'s vault half): the update
//! offers of renewed values, the Rename banner of legacy names, Renew….

use std::sync::Arc;

use notchling_engine::machines::forms::rename_note;
use notchling_engine::machines::steps_secret::pending_renames;

use super::model::{MachinesModel, VaultList};
use super::pieces::UpdateOfferRow;

impl MachinesModel {
    // MARK: Renewed secrets, renames

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

    /// A rename ended: the vault's names again, and a line when one didn't move.
    pub fn renamed(&mut self, failed: bool) {
        self.renaming = false;
        self.refresh_vault();
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
