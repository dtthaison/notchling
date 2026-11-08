//! The vault's values and list together (Swift `VaultStore`).
//!
//! - The values are ONE Keychain item, service `Notchling Secret Vault`, account `values`, JSON
//!   of id → `{username, secret}`, behind [`VaultKeychain`]. On macOS it goes through the Security
//!   framework ([`super::keychain_mac`]), so the item trusts only the build that wrote it; that is
//!   the opposite of the account logins' rule, on purpose. Elsewhere it is [`Unsupported`].
//! - The list is a `vault.json` the store is given (the daemon's own, never the Swift app's).
//! - Values are read once and kept in memory until [`VaultStore::forget`].
//!
//! Every change, and every Keychain call, holds one lock, so a save can't interleave with
//! another. The values kept in memory sit behind a second lock that is never held across a
//! Keychain call: on macOS a read can wait on the user's prompt for as long as it stays up, and
//! [`VaultStore::forget`] (the switch going off) must not wait for that. A read that began before
//! a forget doesn't put what it read back in memory. No error here holds a value: Keychain
//! failures are fixed texts, and the item's JSON is never quoted.

use super::entry::{Kind, VaultEntry, VaultValue};
use super::list::{self, IndexError};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

/// The item's service and account.
pub const SERVICE: &str = "Notchling Secret Vault";
pub const ACCOUNT: &str = "values";

/// What a Keychain call failed with. Never a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeychainError {
    /// The user (or macOS) refused access.
    Denied,
    /// This system has no store the vault trusts yet.
    Unsupported(&'static str),
    /// Any other failure: the OS's status code and its text.
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

/// The one item holding every value.
pub trait VaultKeychain: Send + Sync {
    /// The item's bytes; `None` when there is no item.
    fn read(&self) -> Result<Option<Vec<u8>>, KeychainError>;
    /// Replaces the item's bytes, making the item when there is none.
    fn write(&self, data: &[u8]) -> Result<(), KeychainError>;
}

/// A store for a system with none the vault trusts (Linux, Windows) or a daemon that must never
/// touch the real one (a fixture root): every call fails, nothing is read.
pub struct Unsupported(pub &'static str);

impl VaultKeychain for Unsupported {
    fn read(&self) -> Result<Option<Vec<u8>>, KeychainError> {
        Err(KeychainError::Unsupported(self.0))
    }
    fn write(&self, _data: &[u8]) -> Result<(), KeychainError> {
        Err(KeychainError::Unsupported(self.0))
    }
}

/// What the system's store answers on this OS.
pub const NO_STORE: &str =
    "The vault keeps its values only in the macOS Keychain so far; this system has no store for them yet.";

/// The real store: the Keychain item on macOS, [`Unsupported`] elsewhere.
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

/// Why a store call failed. Never holds a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VaultError {
    /// The entry can't be saved: the text says why (names and variables only).
    Problem(String),
    Index(IndexError),
    Keychain(KeychainError),
    /// The item exists and isn't the vault's JSON.
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

/// What a save did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub entry: VaultEntry,
    /// A different value is stored now (a new entry counts); a rename that kept it is not.
    pub value_changed: bool,
}

/// The values kept in memory, and how many times they were forgotten.
#[derive(Default)]
struct Cache {
    values: Option<BTreeMap<String, VaultValue>>,
    /// Bumped by each forget: what a read that started before it got is not kept.
    generation: u64,
}

/// The values and the list.
pub struct VaultStore {
    index: PathBuf,
    keychain: Box<dyn VaultKeychain>,
    /// The lock every change and every Keychain call holds.
    changing: Mutex<()>,
    /// The values once read; held only to look or swap, never across a Keychain call.
    cache: Mutex<Cache>,
}

impl VaultStore {
    pub fn new(index: PathBuf, keychain: Box<dyn VaultKeychain>) -> VaultStore {
        VaultStore {
            index,
            keychain,
            changing: Mutex::new(()),
            cache: Mutex::new(Cache::default()),
        }
    }

    pub fn index_path(&self) -> &std::path::Path {
        &self.index
    }

    fn lock(&self) -> MutexGuard<'_, ()> {
        self.changing.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn cache(&self) -> MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// How many forgets so far; taken before a Keychain call, given back to [`Self::keep`].
    fn generation(&self) -> u64 {
        self.cache().generation
    }

    /// Keeps `values` in memory unless a forget came after `generation` was taken.
    fn keep(&self, generation: u64, values: BTreeMap<String, VaultValue>) {
        let mut cache = self.cache();
        if cache.generation == generation {
            cache.values = Some(values);
        }
    }

    /// The list; a file that can't be read is an error, not an empty list.
    pub fn entries(&self) -> Result<Vec<VaultEntry>, VaultError> {
        let _held = self.lock();
        Ok(list::read_index(&self.index)?)
    }

    /// The entry with this name (any case).
    pub fn entry(&self, name: &str) -> Result<Option<VaultEntry>, VaultError> {
        let id = name.to_lowercase();
        Ok(self.entries()?.into_iter().find(|e| e.id() == id))
    }

    /// Drops the values kept in memory; the next use reads the item again. Never waits for a
    /// Keychain call: one under way keeps nothing it read.
    pub fn forget(&self) {
        let mut cache = self.cache();
        cache.values = None;
        cache.generation = cache.generation.wrapping_add(1);
    }

    /// The values, from memory or the item. Called holding [`Self::lock`]; `generation` was taken
    /// after it.
    fn load(&self, generation: u64) -> Result<BTreeMap<String, VaultValue>, VaultError> {
        if let Some(values) = &self.cache().values {
            return Ok(values.clone());
        }
        let values: BTreeMap<String, VaultValue> = match self.keychain.read()? {
            None => BTreeMap::new(),
            // The item's text is never quoted: a parse error could hold a value.
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

    /// The stored value of an entry.
    pub fn value(&self, entry: &VaultEntry) -> Result<Option<VaultValue>, VaultError> {
        let _held = self.lock();
        let generation = self.generation();
        Ok(self.load(generation)?.remove(&entry.id()))
    }

    /// Saves an entry. A `None` value keeps the stored one (editing the note, URL or variables
    /// only); renaming (`old` named otherwise) moves it to the new name. `now` is the ISO time put
    /// in `updated`.
    pub fn save(
        &self,
        entry: &VaultEntry,
        value: Option<VaultValue>,
        old: Option<&VaultEntry>,
        now: &str,
    ) -> Result<Saved, VaultError> {
        let _held = self.lock();
        let generation = self.generation();
        let mut index = list::read_index(&self.index)?;
        if let Some(problem) = list::problem(entry, &index, old) {
            return Err(VaultError::Problem(problem));
        }
        let mut values = self.load(generation)?;
        let id = entry.id();
        let old_id = old.map(VaultEntry::id);
        // A rename whose list write failed left the value under the new id: finding it there makes
        // a retry finish.
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
        index.retain(|e| e.id() != id && Some(e.id()) != old_id);
        let mut saved = entry.clone();
        saved.updated = now.to_string();
        index.push(saved.clone());
        list::write_index(&self.index, &index)?;
        Ok(Saved {
            entry: saved,
            value_changed: changed(before.as_ref(), &stored),
        })
    }

    /// Renames an entry and keeps its value: the value never leaves the store.
    pub fn rename(&self, entry: &VaultEntry, name: &str, now: &str) -> Result<Saved, VaultError> {
        self.save(&entry.renamed(name), None, Some(entry), now)
    }

    /// Removes an entry and its value; false when the list has none by that name. A value under a
    /// name the list doesn't hold is left alone: the item may hold the Swift app's values too.
    pub fn remove(&self, name: &str) -> Result<bool, VaultError> {
        let _held = self.lock();
        let generation = self.generation();
        let index = list::read_index(&self.index)?;
        let id = name.to_lowercase();
        let left: Vec<VaultEntry> = index.iter().filter(|e| e.id() != id).cloned().collect();
        if left.len() == index.len() {
            return Ok(false);
        }
        let mut values = self.load(generation)?;
        if values.remove(&id).is_some() {
            self.store(generation, values)?;
        }
        list::write_index(&self.index, &left)?;
        Ok(true)
    }

    /// Turns "ask before each use" off for these ids (the approval's Always Allow).
    pub fn stop_asking(&self, ids: &BTreeSet<String>) -> Result<(), VaultError> {
        let _held = self.lock();
        let index = list::read_index(&self.index)?;
        let list: Vec<VaultEntry> = index
            .into_iter()
            .map(|mut e| {
                if ids.contains(&e.id()) {
                    e.ask = Some(false);
                }
                e
            })
            .collect();
        Ok(list::write_index(&self.index, &list)?)
    }

    /// Sets or clears the expiry day of a saved secret; only that field changes. A bad day or a
    /// missing name changes nothing. True when the day changed.
    pub fn set_expires(&self, name: &str, day: Option<&str>) -> Result<bool, VaultError> {
        if let Some(day) = day {
            if super::expiry::parse(day).as_deref() != Some(day) {
                return Err(VaultError::Problem(format!(
                    "{day} is not a day (YYYY-MM-DD)."
                )));
            }
        }
        let _held = self.lock();
        let mut index = list::read_index(&self.index)?;
        let id = name.to_lowercase();
        let Some(entry) = index.iter_mut().find(|e| e.id() == id) else {
            return Err(VaultError::NoSuchSecret(name.to_string()));
        };
        let changed = entry.expires.as_deref() != day;
        entry.expires = day.map(str::to_string);
        list::write_index(&self.index, &index)?;
        Ok(changed)
    }
}

/// Whether a save puts a different value in place of the stored one (a new entry counts).
pub fn changed(old: Option<&VaultValue>, new: &VaultValue) -> bool {
    old != Some(new)
}
