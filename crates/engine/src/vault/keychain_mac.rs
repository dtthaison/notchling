use super::store::{KeychainError, VaultKeychain, ACCOUNT, SERVICE};
use security_framework::passwords::{get_generic_password, set_generic_password};

const NOT_FOUND: i32 = -25300;
const DENIED: [i32; 3] = [-128, -25293, -25308];

pub struct SecurityItem;

fn error(e: security_framework::base::Error) -> KeychainError {
    let code = e.code();
    if DENIED.contains(&code) {
        KeychainError::Denied
    } else {
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
