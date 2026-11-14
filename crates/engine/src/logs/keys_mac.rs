use super::checkpoint::Checkpoint;
use super::keys::{account_for, KeyError, KeyRead, KeyStore};
use notchling_events::ids::random_bytes;
use security_framework::passwords::{get_generic_password, set_generic_password};
use std::io::ErrorKind;
use std::path::Path;

pub const KEY_SERVICE: &str = "dev.notchling.audit";
pub const HEAD_SERVICE: &str = "dev.notchling.audit.head";
const NOT_FOUND: i32 = -25300;
const DENIED: [i32; 3] = [-128, -25293, -25308];

pub struct KeychainKeyStore {
    account: String,
}

impl KeychainKeyStore {
    pub fn for_data_dir(data_dir: &Path) -> KeychainKeyStore {
        KeychainKeyStore {
            account: account_for(data_dir),
        }
    }

    pub fn account(&self) -> &str {
        &self.account
    }
}

fn error(e: security_framework::base::Error) -> KeyError {
    if DENIED.contains(&e.code()) {
        KeyError::Denied
    } else {
        KeyError::Io(ErrorKind::Other)
    }
}

impl KeyStore for KeychainKeyStore {
    fn read_key(&self) -> KeyRead {
        match get_generic_password(KEY_SERVICE, &self.account) {
            Ok(data) => match <[u8; 32]>::try_from(data.as_slice()) {
                Ok(key) => KeyRead::Found(key),
                Err(_) => KeyRead::Denied,
            },
            Err(e) if e.code() == NOT_FOUND => KeyRead::Missing,
            Err(_) => KeyRead::Denied,
        }
    }

    fn create_key(&self) -> Result<[u8; 32], KeyError> {
        let key = random_bytes::<32>().ok_or(KeyError::NoRandom)?;
        set_generic_password(KEY_SERVICE, &self.account, &key).map_err(error)?;
        Ok(key)
    }

    fn read_head(&self) -> Option<Checkpoint> {
        let data = get_generic_password(HEAD_SERVICE, &self.account).ok()?;
        serde_json::from_slice(&data).ok()
    }

    fn write_head(&self, checkpoint: &Checkpoint) -> Result<(), KeyError> {
        let bytes =
            serde_json::to_vec(checkpoint).map_err(|_| KeyError::Io(ErrorKind::InvalidData))?;
        set_generic_password(HEAD_SERVICE, &self.account, &bytes).map_err(error)
    }

    fn head_exists(&self) -> bool {
        match get_generic_password(HEAD_SERVICE, &self.account) {
            Ok(_) => true,
            Err(e) => e.code() != NOT_FOUND,
        }
    }
}
