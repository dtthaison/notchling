use super::entry::{Kind, VaultEntry, VaultValue};
use super::list::{self, IndexError, VaultIndex};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard};

pub const SERVICE: &str = "Notchling Secret Vault";
pub const ACCOUNT: &str = "values";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeychainError {
    Denied,
    Unsupported(&'static str),
    Other(i32, String),
}

impl std::fmt::Display for KeychainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeychainError::Denied => f.write_str(
                "Keychain access was denied. Notchling needs it to read the vault; allow it when macOS asks.",
            ),
            KeychainError::Unsupported(why) => f.write_str(why),
            KeychainError::Other(code, text) => write!(f, "Keychain: {text} ({code})"),
        }
    }
}

pub trait VaultKeychain: Send + Sync {
    fn read(&self) -> Result<Option<Vec<u8>>, KeychainError>;
    fn write(&self, data: &[u8]) -> Result<(), KeychainError>;
}

pub struct Unsupported(pub &'static str);

impl VaultKeychain for Unsupported {
    fn read(&self) -> Result<Option<Vec<u8>>, KeychainError> {
        Err(KeychainError::Unsupported(self.0))
    }
    fn write(&self, _data: &[u8]) -> Result<(), KeychainError> {
        Err(KeychainError::Unsupported(self.0))
    }
}

pub const NO_STORE: &str =
    "The vault keeps its values only in the macOS Keychain so far; this system has no store for them yet.";

pub fn system_keychain() -> Box<dyn VaultKeychain> {
    #[cfg(target_os = "macos")]
    {
        Box::new(super::keychain_mac::SecurityItem)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(Unsupported(NO_STORE))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VaultError {
    Problem(String),
    Index(IndexError),
    Keychain(KeychainError),
    ItemUnreadable,
    NoSuchSecret(String),
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VaultError::Problem(why) => f.write_str(why),
            VaultError::Index(e) => write!(f, "{e}"),
            VaultError::Keychain(e) => write!(f, "{e}"),
            VaultError::ItemUnreadable => f.write_str("The vault's Keychain item can't be read."),
            VaultError::NoSuchSecret(name) => write!(f, "There's no secret named {name}."),
        }
    }
}

impl From<IndexError> for VaultError {
    fn from(e: IndexError) -> Self {
        VaultError::Index(e)
    }
}

impl From<KeychainError> for VaultError {
    fn from(e: KeychainError) -> Self {
        VaultError::Keychain(e)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub entry: VaultEntry,
    pub value_changed: bool,
}

#[derive(Default)]
struct Cache {
    values: Option<BTreeMap<String, VaultValue>>,
    generation: u64,
}

pub struct VaultStore {
    index: VaultIndex,
    keychain: Box<dyn VaultKeychain>,
    changing: Mutex<()>,
    cache: Mutex<Cache>,
}

impl VaultStore {
    pub fn new(index: VaultIndex, keychain: Box<dyn VaultKeychain>) -> VaultStore {
        VaultStore {
            index,
            keychain,
            changing: Mutex::new(()),
            cache: Mutex::new(Cache::default()),
        }
    }

    pub fn index(&self) -> &VaultIndex {
        &self.index
    }

    fn lock(&self) -> MutexGuard<'_, ()> {
        self.changing.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn cache(&self) -> MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn generation(&self) -> u64 {
        self.cache().generation
    }

    fn keep(&self, generation: u64, values: BTreeMap<String, VaultValue>) {
        let mut cache = self.cache();
        if cache.generation == generation {
            cache.values = Some(values);
        }
    }

    pub fn entries(&self) -> Result<Vec<VaultEntry>, VaultError> {
        let _held = self.lock();
        Ok(self.index.read()?)
    }

    pub fn entry(&self, name: &str) -> Result<Option<VaultEntry>, VaultError> {
        let id = name.to_lowercase();
        Ok(self.entries()?.into_iter().find(|e| e.id() == id))
    }

    pub fn forget(&self) {
        let mut cache = self.cache();
        cache.values = None;
        cache.generation = cache.generation.wrapping_add(1);
    }

    fn load(&self, generation: u64) -> Result<BTreeMap<String, VaultValue>, VaultError> {
        if let Some(values) = &self.cache().values {
            return Ok(values.clone());
        }
        let values: BTreeMap<String, VaultValue> = match self.keychain.read()? {
            None => BTreeMap::new(),
            Some(data) => serde_json::from_slice(&data).map_err(|_| VaultError::ItemUnreadable)?,
        };
        self.keep(generation, values.clone());
        Ok(values)
    }

    fn store(
        &self,
        generation: u64,
        values: BTreeMap<String, VaultValue>,
    ) -> Result<(), VaultError> {
        let data = serde_json::to_vec(&values).map_err(|_| VaultError::ItemUnreadable)?;
        self.keychain.write(&data)?;
        self.keep(generation, values);
        Ok(())
    }

    pub fn value(&self, entry: &VaultEntry) -> Result<Option<VaultValue>, VaultError> {
        let _held = self.lock();
        let generation = self.generation();
        Ok(self.load(generation)?.remove(&entry.id()))
    }

    pub fn save(
        &self,
        entry: &VaultEntry,
        value: Option<VaultValue>,
        old: Option<&VaultEntry>,
        now: &str,
    ) -> Result<Saved, VaultError> {
        let _held = self.lock();
        let generation = self.generation();
        let index = self.index.read()?;
        if let Some(problem) = list::problem(entry, &index, old) {
            return Err(VaultError::Problem(problem));
        }
        let mut values = self.load(generation)?;
        let id = entry.id();
        let old_id = old.map(VaultEntry::id);
        let stored = match value {
            Some(v) => Some(v),
            None => old_id
                .as_ref()
                .and_then(|o| values.get(o))
                .or_else(|| values.get(&id))
                .cloned(),
        };
        let Some(stored) = stored.filter(|v| !v.secret.is_empty()) else {
            return Err(VaultError::Problem(
                match entry.kind {
                    Kind::Login => "Enter the password.",
                    Kind::Token => "Enter the token.",
                }
                .into(),
            ));
        };
        let before = old_id
            .as_ref()
            .and_then(|o| values.get(o))
            .or_else(|| values.get(&id))
            .cloned();
        if let Some(o) = &old_id {
            values.remove(o);
        }
        values.insert(id.clone(), stored.clone());
        self.store(generation, values)?;
        let mut saved = entry.clone();
        saved.updated = now.to_string();
        self.index.update(|mut list| {
            list.retain(|e| e.id() != id && Some(e.id()) != old_id);
            list.push(saved.clone());
            Ok(list)
        })?;
        Ok(Saved {
            entry: saved,
            value_changed: changed(before.as_ref(), &stored),
        })
    }

    pub fn rename(&self, entry: &VaultEntry, name: &str, now: &str) -> Result<Saved, VaultError> {
        self.save(&entry.renamed(name), None, Some(entry), now)
    }

    pub fn remove(&self, name: &str) -> Result<bool, VaultError> {
        let _held = self.lock();
        let generation = self.generation();
        let index = self.index.read()?;
        let id = name.to_lowercase();
        if !index.iter().any(|e| e.id() == id) {
            return Ok(false);
        }
        let mut values = self.load(generation)?;
        if values.remove(&id).is_some() {
            self.store(generation, values)?;
        }
        self.index
            .update(|list| Ok(list.into_iter().filter(|e| e.id() != id).collect()))?;
        Ok(true)
    }

    pub fn stop_asking(&self, ids: &BTreeSet<String>) -> Result<(), VaultError> {
        let _held = self.lock();
        if !self.index.read()?.iter().any(|e| ids.contains(&e.id())) {
            return Ok(());
        }
        Ok(self.index.update(|list| {
            Ok(list
                .into_iter()
                .map(|mut e| {
                    if ids.contains(&e.id()) {
                        e.ask = Some(false);
                    }
                    e
                })
                .collect())
        })?)
    }

    pub fn set_expires(&self, name: &str, day: Option<&str>) -> Result<bool, VaultError> {
        if let Some(day) = day {
            if super::expiry::parse(day).as_deref() != Some(day) {
                return Err(VaultError::Problem(format!(
                    "{day} is not a day (YYYY-MM-DD)."
                )));
            }
        }
        let _held = self.lock();
        let id = name.to_lowercase();
        let Some(entry) = self.index.read()?.into_iter().find(|e| e.id() == id) else {
            return Err(VaultError::NoSuchSecret(name.to_string()));
        };
        let changed = entry.expires.as_deref() != day;
        self.index.update(|mut list| {
            if let Some(entry) = list.iter_mut().find(|e| e.id() == id) {
                entry.expires = day.map(str::to_string);
            }
            Ok(list)
        })?;
        Ok(changed)
    }
}

pub fn changed(old: Option<&VaultValue>, new: &VaultValue) -> bool {
    old != Some(new)
}
