//! The vault's Keychain item through the Security framework (`SecItemCopyMatching`, `SecItemAdd`,
//! `SecItemUpdate`), macOS only. An item this process adds trusts this build alone, so anything
//! else, `security` included, gets macOS's prompt instead of the values. A rebuilt binary is asked
//! once too ("Always Allow"), which is wanted here. Only the daemon's vault service calls it, and
//! only while the `vault` switch is on. Tests never do: they use a fake [`VaultKeychain`].

use super::store::{KeychainError, VaultKeychain, ACCOUNT, SERVICE};
use security_framework::passwords::{get_generic_password, set_generic_password};

/// `errSecItemNotFound`.
const NOT_FOUND: i32 = -25300;
/// `errSecUserCanceled`, `errSecAuthFailed`, `errSecInteractionNotAllowed`.
const DENIED: [i32; 3] = [-128, -25293, -25308];

/// The item, service [`SERVICE`] and account [`ACCOUNT`].
pub struct SecurityItem;

fn error(e: security_framework::base::Error) -> KeychainError {
    let code = e.code();
    if DENIED.contains(&code) {
        KeychainError::Denied
    } else {
        // The framework's message for the status: about the call, never the item's data.
        KeychainError::Other(code, e.message().unwrap_or_else(|| "error".into()))
    }
}

impl VaultKeychain for SecurityItem {
    fn read(&self) -> Result<Option<Vec<u8>>, KeychainError> {
        match get_generic_password(SERVICE, ACCOUNT) {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(e) => Err(error(e)),
        }
    }

    fn write(&self, data: &[u8]) -> Result<(), KeychainError> {
        set_generic_password(SERVICE, ACCOUNT, data).map_err(error)
    }
}
