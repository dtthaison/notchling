use notchling_engine::vault::entry::VaultValue;
use notchling_engine::vault::list::VaultIndex;
use notchling_engine::vault::store::{VaultKeychain, VaultStore};

pub trait SecretReader: Send + Sync {
    fn read(&self, name: &str, rev: &str) -> Result<VaultValue, String>;
}

pub struct StoreReader {
    store: VaultStore,
}

impl StoreReader {
    pub fn new(index: VaultIndex, keychain: Box<dyn VaultKeychain>) -> StoreReader {
        StoreReader {
            store: VaultStore::new(index, keychain),
        }
    }
}

impl SecretReader for StoreReader {
    fn read(&self, name: &str, rev: &str) -> Result<VaultValue, String> {
        let entry = self
            .store
            .entry(name)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("There's no secret named {name}."))?;
        if entry.revision() != rev {
            return Err(notchling_locale::consent::card_vault_changed().to_owned());
        }
        let read = self.store.value(&entry);
        self.store.forget();
        match read {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(notchling_locale::consent::card_vault_unreadable().to_owned()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::vault::entry::{Kind, VaultEntry};
    use notchling_engine::vault::store::KeychainError;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct Counting {
        reads: Arc<AtomicUsize>,
        data: Vec<u8>,
        fail: bool,
    }

    impl VaultKeychain for Counting {
        fn read(&self) -> Result<Option<Vec<u8>>, KeychainError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err(KeychainError::Denied);
            }
            Ok(Some(self.data.clone()))
        }

        fn write(&self, _data: &[u8]) -> Result<(), KeychainError> {
            Ok(())
        }
    }

    const REV: &str = "2026-11-01T09:00:00Z#2";

    fn reader(fail: bool) -> (StoreReader, Arc<AtomicUsize>, tempdir::Dir) {
        let dir = tempdir::Dir::new();
        let index = VaultIndex::in_data(dir.path());
        let mut entry = VaultEntry::new("npm", Kind::Login);
        entry.updated = "2026-11-01T09:00:00Z".into();
        entry.rev = 2;
        index.write(std::slice::from_ref(&entry)).unwrap();
        let mut values = BTreeMap::new();
        values.insert(
            entry.id(),
            VaultValue {
                username: "fake-user".into(),
                secret: "fake-secret".into(),
            },
        );
        let reads = Arc::new(AtomicUsize::new(0));
        let keychain = Counting {
            reads: reads.clone(),
            data: serde_json::to_vec(&values).unwrap(),
            fail,
        };
        (StoreReader::new(index, Box::new(keychain)), reads, dir)
    }

    mod tempdir {
        use std::path::{Path, PathBuf};

        pub struct Dir(PathBuf);

        impl Dir {
            pub fn new() -> Dir {
                let path = std::env::temp_dir().join(format!(
                    "nl-vault-reader-{}-{:?}",
                    std::process::id(),
                    std::thread::current().id()
                ));
                std::fs::create_dir_all(&path).unwrap();
                Dir(path)
            }

            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn a_read_goes_to_the_keychain_every_time_and_keeps_nothing() {
        let (reader, reads, _dir) = reader(false);
        let first = reader.read("npm", REV).unwrap();
        assert_eq!(first.secret, "fake-secret");
        assert_eq!(first.username, "fake-user");
        reader.read("npm", REV).unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_missing_name_is_refused_before_the_keychain_is_read() {
        let (reader, reads, _dir) = reader(false);
        assert!(reader.read("other", REV).is_err());
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_denied_keychain_says_so_without_a_value() {
        let (reader, _reads, _dir) = reader(true);
        let error = reader.read("npm", REV).unwrap_err();
        assert!(error.contains("Keychain access was denied"));
        assert!(!error.contains("fake"));
    }

    #[test]
    fn a_read_for_a_revision_the_entry_on_disk_has_moved_past_is_refused_before_the_keychain_is_read(
    ) {
        let (reader, reads, _dir) = reader(false);
        for stale in ["2026-11-01T09:00:00Z#1", "2026-11-01T09:00:00Z", ""] {
            let error = reader.read("npm", stale).unwrap_err();
            assert!(!error.contains("fake"));
            assert_eq!(error, notchling_locale::consent::card_vault_changed());
        }
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        assert_eq!(reader.read("npm", REV).unwrap().secret, "fake-secret");
    }
}
