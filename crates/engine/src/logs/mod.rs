mod audit_files;
mod checkpoint;
mod keys;
#[cfg(target_os = "macos")]
mod keys_mac;
mod legacy;

pub use audit_files::{
    month_end_ms, month_name, month_of, month_start_ms, parse_month_name, AuditDir,
};
pub use checkpoint::{Checkpoint, UserDeleted, USER_DELETED_KEEP};
pub use keys::{
    account_for, FileKeyStore, KeyError, KeyRead, KeyStore, MemoryKeyStore, HEAD_FILE, KEY_FILE,
};
#[cfg(target_os = "macos")]
pub use keys_mac::KeychainKeyStore;
pub use legacy::{has_legacy, move_legacy, LegacyFile};

#[cfg(test)]
mod audit_files_tests;
#[cfg(test)]
mod keys_tests;
#[cfg(test)]
mod legacy_tests;
