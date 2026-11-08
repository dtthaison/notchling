//! E9, the Secret Vault (Swift `Vault/`): credentials agents use without them going through a chat.
//!
//! | Part | Where |
//! | --- | --- |
//! | An entry, its value, a request's fields, what a request changes in a form | [`entry`] |
//! | The list (`vault.json`, no values): read, write 0600, check, `find`, env, `describe` | [`list`] |
//! | Expiry days and their wording | [`expiry`] |
//! | Which reminders are due, and what Renew opens | [`reminders`], [`url`] |
//! | Hiding values in a byte stream | [`redact`] |
//! | Running a command with the values, its output hidden | [`runner`] |
//! | The values (one Keychain item) and the list together | [`store`], `keychain_mac` |
//! | The `notchling-vault` skill's text | [`skill`] |
//!
//! The daemon serves it behind its `vault` switch (`notchlingd`'s `vault_ops`); nothing here
//! decides who may read a value. No test touches the Keychain: they use a fake [`VaultKeychain`].

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

/// A fresh id nobody can guess: 128 bits from the OS's random source, in hex, after `prefix`.
/// `None` when the OS gives no randomness, so the caller refuses rather than use a weak id.
pub fn random_id(prefix: &str) -> Option<String> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut bytes = [0u8; 16];
    SystemRandom::new().fill(&mut bytes).ok()?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Some(format!("{prefix}{hex}"))
}

/// Now as ISO 8601 in UTC (`2026-11-08T15:04:05Z`), for an entry's `updated`.
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
