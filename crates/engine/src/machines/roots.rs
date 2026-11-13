use super::registry::RegistryFiles;
use super::step::SecretSource;
use crate::accounts::Secret;
use crate::vault::{VaultEntry, VaultStore};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineRoots {
    pub data_dir: PathBuf,
    pub swift_file: PathBuf,
    pub own_dir: PathBuf,
}

impl MachineRoots {
    pub fn in_data(data: &Path) -> MachineRoots {
        MachineRoots {
            data_dir: data.to_path_buf(),
            swift_file: data.join("machines.json"),
            own_dir: data.join("daemon").join("machines"),
        }
    }

    pub fn registry(&self) -> RegistryFiles {
        RegistryFiles {
            data_dir: self.data_dir.clone(),
            old_own: self.own_dir.join("machines.json"),
            swift: Some(self.swift_file.clone()),
            read_only: false,
        }
    }

    pub fn audit_file(&self) -> PathBuf {
        self.own_dir.join("audit.jsonl")
    }

    pub fn merge_dir(&self) -> PathBuf {
        self.own_dir.join("merge")
    }

    pub fn scratch(&self) -> PathBuf {
        self.own_dir.join("scratch")
    }
}

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
        assert_eq!(reg.data_dir, Path::new("/tmp/nl-root/notchling"));
        assert_eq!(
            reg.old_own,
            Path::new("/tmp/nl-root/notchling/daemon/machines/machines.json")
        );
        assert_eq!(
            reg.swift.as_deref(),
            Some(Path::new("/tmp/nl-root/notchling/machines.json"))
        );
        assert!(!reg.read_only);
    }
}
