//! The Secret Vault as the Machines window sees it (Swift `MachinesModel`'s vault half): the update
//! offers of renewed values, the Rename banner of legacy names, Renew….

use notchling_engine::machines::forms::rename_note;
use notchling_engine::machines::steps_secret::pending_renames;

use super::model::MachinesModel;
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

    /// The Rename button: moves each legacy secret to its new name, keeping its value.
    pub fn rename_secrets(&mut self) {
        let mut failed = false;
        for (entry, name) in pending_renames(&self.vault_entries) {
            if self.parts.vault.rename(&entry.name, &name).is_err() {
                failed = true;
            }
        }
        self.refresh_vault();
        self.rename_note = failed.then(|| "Rename didn't finish; try again".to_owned());
    }

    /// Renew…: the secret's page (GitHub's token page for a GitHub secret).
    pub fn renew(&mut self, name: &str) {
        let url = self
            .vault_entries
            .iter()
            .find(|e| e.id() == name.to_lowercase())
            .and_then(|e| e.url.clone());
        self.notice = self.parts.vault.renew(name, url.as_deref()).err();
    }
}
