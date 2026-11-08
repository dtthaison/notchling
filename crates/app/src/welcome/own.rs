//! Yes/no facts the app keeps in its own `settings.json`, past the engine's key list the way
//! `actingOn` and `bridgeOn` are: the first-run flag and the user's choice for each M6 switch. The
//! Swift app's preferences are never touched.

use std::path::PathBuf;

use notchling_engine::settings::Settings;
use serde_json::Value;

/// Set once the first-run view has been shown and dismissed.
pub const FIRST_RUN_KEY: &str = "firstRunDone";

/// The key holding the user's choice for a daemon switch (`accounts` -> `accountsOn`).
pub fn choice_key(switch: &str) -> String {
    format!("{switch}On")
}

/// Booleans in one settings file.
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

    /// Stores `on`, keeping every other value; a file that can't be read is left as it is.
    pub fn set(&self, key: &str, on: bool) -> Result<(), String> {
        let mut store = Settings::open(&self.file).map_err(|e| e.to_string())?;
        let mut values = store.values().clone();
        values.insert(key.to_owned(), Value::Bool(on));
        store.replace_all(values).map_err(|e| e.to_string())
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
    fn choice_keys_follow_the_acting_and_bridge_pattern() {
        assert_eq!(choice_key("accounts"), "accountsOn");
        assert_eq!(choice_key("install"), "installOn");
    }
}
