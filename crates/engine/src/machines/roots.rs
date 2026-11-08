//! Where Machines reads and writes, as parameters (decision: the Swift app owns the machines until
//! handover). The Swift app's `machines.json` sits in the data folder and is only read; this tool
//! writes its own copy, the pending offers and its scratch files under `<data>/daemon/machines/`.

use super::registry::RegistryFiles;
use super::step::SecretSource;
use crate::accounts::Secret;
use crate::vault::{VaultEntry, VaultStore};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineRoots {
    /// The Swift app's file, read only.
    pub swift_file: PathBuf,
    /// This tool's folder.
    pub own_dir: PathBuf,
}

impl MachineRoots {
    /// From Notchling's data folder (`~/Library/Application Support/Notchling`, or a fixture's).
    pub fn in_data(data: &Path) -> MachineRoots {
        MachineRoots {
            swift_file: data.join("machines.json"),
            own_dir: data.join("daemon").join("machines"),
        }
    }

    /// The registry: this tool's own file once it saved one, until then the Swift app's.
    pub fn registry(&self) -> RegistryFiles {
        RegistryFiles {
            own: self.own_dir.join("machines.json"),
            swift: Some(self.swift_file.clone()),
        }
    }

    pub fn offers_file(&self) -> PathBuf {
        self.own_dir.join("pending-updates.json")
    }

    /// A private folder for files a Fix hands `gh` (a public key).
    pub fn scratch(&self) -> PathBuf {
        self.own_dir.join("scratch")
    }
}

/// The Secret Vault as Machines reads it. Made only when the vault is switched on: until then the
/// tool takes [`super::step::NoSecrets`] and never opens the Keychain item.
pub struct VaultSecrets {
    store: VaultStore,
}

impl VaultSecrets {
    pub fn new(store: VaultStore) -> VaultSecrets {
        VaultSecrets { store }
    }
}

impl SecretSource for VaultSecrets {
    fn value(&self, name: &str) -> Option<Secret> {
        let entry = self.store.entry(name).ok()??;
        let value = self.store.value(&entry).ok()??;
        (!value.secret.is_empty()).then(|| Secret::new(value.secret))
    }

    fn entry(&self, name: &str) -> Option<VaultEntry> {
        self.store.entry(name).ok()?
    }

    fn rename(&self, old: &str, new: &str) -> Result<(), String> {
        let entry = self
            .store
            .entry(old)
            .map_err(|e| e.to_string())?
            .ok_or(format!("the vault has no secret named {old}"))?;
        if self.store.entry(new).map_err(|e| e.to_string())?.is_some() {
            return Err(format!("the vault already has a secret named {new}"));
        }
        self.store
            .rename(&entry, new, &crate::vault::now_iso())
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn set_expires(&self, name: &str, day: Option<&str>) -> Result<(), String> {
        self.store
            .set_expires(name, day)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_swift_file_is_read_and_this_tool_writes_beside_the_daemon() {
        let r = MachineRoots::in_data(Path::new("/tmp/nl-root/notchling"));
        let reg = r.registry();
        assert_eq!(
            reg.own,
            Path::new("/tmp/nl-root/notchling/daemon/machines/machines.json")
        );
        assert_eq!(
            reg.swift.as_deref(),
            Some(Path::new("/tmp/nl-root/notchling/machines.json"))
        );
        assert_eq!(
            r.offers_file(),
            Path::new("/tmp/nl-root/notchling/daemon/machines/pending-updates.json")
        );
        assert_eq!(reg.offers_file(), r.offers_file());
    }
}
