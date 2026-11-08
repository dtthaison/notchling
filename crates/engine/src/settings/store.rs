//! The settings file: JSON, written atomically, mode 0600.

use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// Why a settings call failed.
#[derive(Debug, PartialEq, Eq)]
pub enum SettingsError {
    Io(String),
    /// The file exists but isn't a settings file. It is never written over.
    Corrupt(String),
    UnknownKey(String),
    /// The value doesn't fit the key's type or allowed strings.
    BadValue(String),
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsError::Io(m) => write!(f, "settings file: {m}"),
            SettingsError::Corrupt(m) => write!(f, "settings file can't be read: {m}"),
            SettingsError::UnknownKey(k) => write!(f, "unknown setting {k}"),
            SettingsError::BadValue(k) => write!(f, "wrong value for setting {k}"),
        }
    }
}

impl std::error::Error for SettingsError {}

/// The settings, loaded from one file. Reads fall back to the key's default.
#[derive(Debug)]
pub struct Settings {
    path: PathBuf,
    values: Map<String, Value>,
}

impl Settings {
    /// Opens the file; a missing one is an empty store (nothing is written until `set` or `save`).
    pub fn open(_path: &Path) -> Result<Settings, SettingsError> {
        let _ = &Settings { path: PathBuf::new(), values: Map::new() };
        todo!()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The stored value, without the default.
    pub fn raw(&self, _key: &str) -> Option<&Value> {
        todo!()
    }

    /// The stored value, else the key's default.
    pub fn get(&self, _key: &str) -> Option<Value> {
        todo!()
    }

    pub fn get_bool(&self, _key: &str) -> Option<bool> {
        todo!()
    }

    pub fn get_i64(&self, _key: &str) -> Option<i64> {
        todo!()
    }

    pub fn get_f64(&self, _key: &str) -> Option<f64> {
        todo!()
    }

    pub fn get_str(&self, _key: &str) -> Option<String> {
        todo!()
    }

    /// Checks the value against the key, stores it and saves.
    pub fn set(&mut self, _key: &str, _value: Value) -> Result<(), SettingsError> {
        todo!()
    }

    /// Forgets the key (it reads as its default again) and saves.
    pub fn remove(&mut self, _key: &str) -> Result<(), SettingsError> {
        todo!()
    }

    /// Replaces every value with `values` (already checked, as the migration's are) and saves.
    pub fn replace_all(&mut self, _values: Map<String, Value>) -> Result<(), SettingsError> {
        todo!()
    }

    /// Writes the file: a temp file beside it, mode 0600, then a rename.
    pub fn save(&self) -> Result<(), SettingsError> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::sessions::testutil::TempDir;
    use super::*;
    use serde_json::json;

    fn open(tmp: &TempDir) -> Settings {
        Settings::open(&tmp.path("data/settings.json")).expect("open")
    }

    #[test]
    fn missing_file_is_empty_and_reads_defaults() {
        let tmp = TempDir::new("set-empty");
        let s = open(&tmp);
        assert_eq!(s.raw("showPet"), None);
        assert_eq!(s.get_bool("showPet"), Some(true));
        assert_eq!(s.get_i64("limitSwitchAt"), Some(95));
        assert_eq!(s.get_f64("chatPageWidth"), Some(520.0));
        assert_eq!(s.get_str("notchWidth").as_deref(), Some("medium"));
        assert_eq!(s.get_str("chatSelection"), None);
        assert_eq!(s.get("dismissed"), Some(json!({})));
        assert!(!tmp.path("data/settings.json").exists(), "opening writes nothing");
    }

    #[test]
    fn set_saves_and_reopens() {
        let tmp = TempDir::new("set-roundtrip");
        let mut s = open(&tmp);
        s.set("showPet", json!(false)).expect("set");
        s.set("notchWidth", json!("large")).expect("set");
        s.set("limitSwitchAt", json!(80)).expect("set");
        let again = open(&tmp);
        assert_eq!(again.get_bool("showPet"), Some(false));
        assert_eq!(again.get_str("notchWidth").as_deref(), Some("large"));
        assert_eq!(again.get_i64("limitSwitchAt"), Some(80));
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_mode_0600_and_no_temp_file_is_left() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new("set-mode");
        let mut s = open(&tmp);
        s.set("showPet", json!(false)).expect("set");
        s.set("showPet", json!(true)).expect("set again");
        let mode = std::fs::metadata(tmp.path("data/settings.json")).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let names: Vec<_> = std::fs::read_dir(tmp.path("data"))
            .expect("dir")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["settings.json"]);
    }

    #[test]
    fn a_broken_file_is_an_error_and_is_never_written_over() {
        let tmp = TempDir::new("set-corrupt");
        tmp.write("data/settings.json", "{ not json");
        assert!(matches!(Settings::open(&tmp.path("data/settings.json")), Err(SettingsError::Corrupt(_))));
        assert_eq!(std::fs::read_to_string(tmp.path("data/settings.json")).expect("read"), "{ not json");
        tmp.write("data/settings.json", r#"{"version":1,"values":[1]}"#);
        assert!(matches!(Settings::open(&tmp.path("data/settings.json")), Err(SettingsError::Corrupt(_))));
    }

    #[test]
    fn unknown_keys_and_wrong_values_are_refused() {
        let tmp = TempDir::new("set-bad");
        let mut s = open(&tmp);
        assert_eq!(s.set("nope", json!(1)), Err(SettingsError::UnknownKey("nope".into())));
        assert_eq!(s.set("showPet", json!("yes")), Err(SettingsError::BadValue("showPet".into())));
        assert_eq!(s.set("notchWidth", json!("huge")), Err(SettingsError::BadValue("notchWidth".into())));
        assert_eq!(s.set("limitSwitchAt", json!(1.5)), Err(SettingsError::BadValue("limitSwitchAt".into())));
        assert_eq!(s.set("chatProjectFolders", json!([1])), Err(SettingsError::BadValue("chatProjectFolders".into())));
        assert_eq!(s.raw("showPet"), None);
        assert!(!tmp.path("data/settings.json").exists());
    }

    #[test]
    fn remove_goes_back_to_the_default() {
        let tmp = TempDir::new("set-remove");
        let mut s = open(&tmp);
        s.set("showPet", json!(false)).expect("set");
        s.remove("showPet").expect("remove");
        assert_eq!(open(&tmp).get_bool("showPet"), Some(true));
    }

    #[test]
    fn keys_a_newer_version_wrote_survive_a_save() {
        let tmp = TempDir::new("set-forward");
        tmp.write("data/settings.json", r#"{"version":1,"values":{"futureKey":{"a":1},"showPet":false}}"#);
        let mut s = open(&tmp);
        s.set("keepPages", json!(false)).expect("set");
        let text = std::fs::read_to_string(tmp.path("data/settings.json")).expect("read");
        assert!(text.contains("futureKey"), "{text}");
        assert!(text.contains("\"showPet\""), "{text}");
    }
}
