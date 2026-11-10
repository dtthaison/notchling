//! Yes/no facts the app keeps in its own settings (`notchling.db`, opened by the path of its old
//! `settings.json`), past the engine's key list the way
//! `actingOn` and `bridgeOn` are: the first-run flag and the user's choice for each M6 switch. The
//! Swift app's preferences are never touched.

use std::path::PathBuf;

use notchling_engine::settings::{Settings, SettingsError};
use serde_json::Value;

/// Set once the first-run view has been shown and dismissed.
pub const FIRST_RUN_KEY: &str = "firstRunDone";

/// The key holding the user's choice for a daemon switch (`accounts` -> `accountsOn`).
pub fn choice_key(switch: &str) -> String {
    format!("{switch}On")
}

/// Booleans in one data folder's settings; `file` names its old `settings.json`.
#[derive(Clone, Debug)]
pub struct Flags {
    pub file: PathBuf,
}

impl Flags {
    /// The stored value, false when unset or the file can't be read.
    pub fn get(&self, key: &str) -> bool {
        Settings::open(&self.file)
            .ok()
            .and_then(|s| s.raw(key).and_then(Value::as_bool))
            .unwrap_or(false)
    }

    /// Stores `on` and nothing else (one row); a file that can't be read is left as it is.
    pub fn set(&self, key: &str, on: bool) -> Result<(), String> {
        let mut store = Settings::open(&self.file).map_err(|e| e.to_string())?;
        store
            .set_unchecked(key, Value::Bool(on))
            .map_err(|e| e.to_string())
    }

    /// Stores the user's choice for a switch. Unlike [`Flags::set`], an old `settings.json` that
    /// can't be read doesn't stop it: that file is moved aside (`settings.json.unreadable-<secs>`,
    /// nothing in it lost) and the choice is written, so "off" is always kept.
    pub fn set_choice(&self, key: &str, on: bool) -> Result<(), String> {
        let mut store = match Settings::open(&self.file) {
            Ok(store) => store,
            Err(SettingsError::Corrupt(_)) => {
                let secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let name = self
                    .file
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let aside = self
                    .file
                    .with_file_name(format!("{name}.unreadable-{secs}"));
                std::fs::rename(&self.file, &aside).map_err(|e| e.to_string())?;
                Settings::open(&self.file).map_err(|e| e.to_string())?
            }
            Err(e) => return Err(e.to_string()),
        };
        store
            .set_unchecked(key, Value::Bool(on))
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nl-own-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("settings.json")
    }

    #[test]
    fn a_flag_round_trips_and_keeps_the_rest() {
        let path = file("round");
        let flags = Flags { file: path.clone() };
        assert!(!flags.get(FIRST_RUN_KEY));
        let mut store = Settings::open(&path).unwrap();
        store.set("mode", json!("on")).unwrap();
        flags.set(FIRST_RUN_KEY, true).unwrap();
        flags.set(&choice_key("vault"), true).unwrap();
        assert!(flags.get(FIRST_RUN_KEY) && flags.get("vaultOn"));
        assert!(!flags.get("accountsOn"));
        assert_eq!(
            Settings::open(&path).unwrap().get_str("mode").as_deref(),
            Some("on")
        );
        flags.set("vaultOn", false).unwrap();
        assert!(!flags.get("vaultOn"));
    }

    #[test]
    fn an_unreadable_file_is_never_written_over() {
        let path = file("bad");
        std::fs::write(&path, "[1,").unwrap();
        let flags = Flags { file: path.clone() };
        assert!(flags.set(FIRST_RUN_KEY, true).is_err());
        assert!(!flags.get(FIRST_RUN_KEY));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[1,");
    }

    #[test]
    fn a_choice_is_kept_even_over_an_unreadable_file_which_is_set_aside() {
        let path = file("choice-bad");
        std::fs::write(&path, "[1,").unwrap();
        let flags = Flags { file: path.clone() };
        flags.set_choice("vaultOn", false).unwrap();
        assert_eq!(
            Settings::open(&path).unwrap().raw("vaultOn"),
            Some(&json!(false))
        );
        let dir = path.parent().unwrap();
        let aside: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("settings.json.unreadable-"))
            .collect();
        assert_eq!(aside.len(), 1, "{aside:?}");
        assert_eq!(std::fs::read_to_string(dir.join(&aside[0])).unwrap(), "[1,");
    }

    #[test]
    fn writes_from_many_threads_at_once_keep_every_key() {
        let path = file("many");
        let threads: Vec<_> = (0..16)
            .map(|i| {
                let flags = Flags { file: path.clone() };
                std::thread::spawn(move || flags.set_choice(&format!("k{i}On"), true).unwrap())
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let store = Settings::open(&path).unwrap();
        for i in 0..16 {
            assert_eq!(store.raw(&format!("k{i}On")), Some(&json!(true)), "k{i}");
        }
    }

    #[test]
    fn choice_keys_follow_the_acting_and_bridge_pattern() {
        assert_eq!(choice_key("accounts"), "accountsOn");
        assert_eq!(choice_key("install"), "installOn");
    }
}
