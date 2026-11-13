pub mod entry;
pub mod expiry;
#[cfg(target_os = "macos")]
mod keychain_mac;
pub mod list;
pub mod redact;
pub mod reminders;
pub mod runner;
pub mod skill;
pub mod store;
pub mod url;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_redact;
#[cfg(test)]
mod tests_reminders;
#[cfg(test)]
mod tests_store;
#[cfg(test)]
mod tests_store_lock;

pub use entry::{
    default_vars, prefix, valid_name, valid_var, Field, HostChange, Kind, PrefillChanges,
    VaultEntry, VaultRequest, VaultValue,
};
pub use redact::Redactor;
pub use store::{
    system_keychain, KeychainError, Saved, Unsupported, VaultError, VaultKeychain, VaultStore,
};

pub fn random_id(prefix: &str) -> Option<String> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut bytes = [0u8; 16];
    SystemRandom::new().fill(&mut bytes).ok()?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Some(format!("{prefix}{hex}"))
}

pub fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let c = crate::pages::clock::civil_at(secs, 0);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        c.year, c.month, c.day, c.hour, c.minute, c.second
    )
}
