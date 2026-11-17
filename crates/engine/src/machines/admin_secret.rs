use super::step::SecretSource;
use crate::accounts::Secret;
use std::sync::Arc;

pub const VAULT_OFF: &str = "the Vault is off";

pub trait AdminSecrets: Send + Sync {
    fn read(&self, entry: &str, machine: &str) -> Result<Secret, String>;
}

pub struct NoAdminSecrets;

impl AdminSecrets for NoAdminSecrets {
    fn read(&self, _entry: &str, _machine: &str) -> Result<Secret, String> {
        Err(VAULT_OFF.into())
    }
}

pub struct FromSource(pub Arc<dyn SecretSource>);

impl AdminSecrets for FromSource {
    fn read(&self, entry: &str, _machine: &str) -> Result<Secret, String> {
        self.0
            .value(entry)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                format!(
                    "{entry} isn't in the Vault, has no password, or the Vault is off or locked"
                )
            })
    }
}

pub fn failure(entry: &str, why: &str) -> String {
    format!("Admin password from Vault entry {entry}: {why}")
}
