//! The settings file: JSON, written atomically, mode 0600.

use super::keys::{spec, KeySpec};
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

/// The file format's version; a newer file's extra keys are kept as they are.
const VERSION: u64 = 1;

impl Settings {
    /// Opens the file; a missing one is an empty store (nothing is written until `set` or `save`).
    pub fn open(path: &Path) -> Result<Settings, SettingsError> {
        let values = match std::fs::read(path) {
            Ok(data) => parse(&data)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Map::new(),
            Err(e) => return Err(SettingsError::Io(e.to_string())),
        };
        Ok(Settings {
            path: path.to_path_buf(),
            values,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every stored value (no defaults), including keys a newer version wrote.
    pub fn values(&self) -> &Map<String, Value> {
        &self.values
    }

    /// The stored value, without the default.
    pub fn raw(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    /// The stored value, else the key's default.
    pub fn get(&self, key: &str) -> Option<Value> {
        self.values
            .get(key)
            .cloned()
            .or_else(|| spec(key).and_then(KeySpec::default_value))
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.get(key)?.as_bool()
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get(key)?.as_i64()
    }

    pub fn get_f64(&self, key: &str) -> Option<f64> {
        self.get(key)?.as_f64()
    }

    pub fn get_str(&self, key: &str) -> Option<String> {
        self.get(key)?.as_str().map(String::from)
    }

    /// Checks the value against the key, stores it and saves. The store is unchanged on an error.
    pub fn set(&mut self, key: &str, value: Value) -> Result<(), SettingsError> {
        let spec = spec(key).ok_or_else(|| SettingsError::UnknownKey(key.to_string()))?;
        if !spec.accepts(&value) {
            return Err(SettingsError::BadValue(key.to_string()));
        }
        let before = self.values.insert(key.to_string(), value);
        self.save_or_restore(key, before)
    }

    /// Forgets the key (it reads as its default again) and saves.
    pub fn remove(&mut self, key: &str) -> Result<(), SettingsError> {
        let before = self.values.remove(key);
        self.save_or_restore(key, before)
    }

    /// Replaces every value with `values` (already checked, as the migration's are) and saves.
    pub fn replace_all(&mut self, values: Map<String, Value>) -> Result<(), SettingsError> {
        let before = std::mem::replace(&mut self.values, values);
        let saved = self.save();
        if saved.is_err() {
            self.values = before;
        }
        saved
    }

    /// Writes the file: a temp file beside it, mode 0600, then a rename.
    pub fn save(&self) -> Result<(), SettingsError> {
        let mut root = Map::new();
        root.insert("version".to_string(), Value::from(VERSION));
        root.insert("values".to_string(), Value::Object(self.values.clone()));
        let mut text = serde_json::to_string_pretty(&Value::Object(root))
            .map_err(|e| SettingsError::Io(e.to_string()))?;
        text.push('\n');
        write_atomic(&self.path, text.as_bytes()).map_err(|e| SettingsError::Io(e.to_string()))
    }

    /// Saves, and puts `key` back as it was when that fails.
    fn save_or_restore(&mut self, key: &str, before: Option<Value>) -> Result<(), SettingsError> {
        let saved = self.save();
        if saved.is_err() {
            match before {
                Some(v) => self.values.insert(key.to_string(), v),
                None => self.values.remove(key),
            };
        }
        saved
    }
}

fn parse(data: &[u8]) -> Result<Map<String, Value>, SettingsError> {
    let root: Value =
        serde_json::from_slice(data).map_err(|e| SettingsError::Corrupt(e.to_string()))?;
    match root.get("values") {
        Some(Value::Object(values)) => Ok(values.clone()),
        _ => Err(SettingsError::Corrupt("no \"values\" object".to_string())),
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("settings.json");
    let temp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&temp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
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
        assert!(
            !tmp.path("data/settings.json").exists(),
            "opening writes nothing"
        );
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
        let mode = std::fs::metadata(tmp.path("data/settings.json"))
            .expect("meta")
            .permissions()
            .mode();
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
        assert!(matches!(
            Settings::open(&tmp.path("data/settings.json")),
            Err(SettingsError::Corrupt(_))
        ));
        assert_eq!(
            std::fs::read_to_string(tmp.path("data/settings.json")).expect("read"),
            "{ not json"
        );
        tmp.write("data/settings.json", r#"{"version":1,"values":[1]}"#);
        assert!(matches!(
            Settings::open(&tmp.path("data/settings.json")),
            Err(SettingsError::Corrupt(_))
        ));
    }

    #[test]
    fn unknown_keys_and_wrong_values_are_refused() {
        let tmp = TempDir::new("set-bad");
        let mut s = open(&tmp);
        assert_eq!(
            s.set("nope", json!(1)),
            Err(SettingsError::UnknownKey("nope".into()))
        );
        assert_eq!(
            s.set("showPet", json!("yes")),
            Err(SettingsError::BadValue("showPet".into()))
        );
        assert_eq!(
            s.set("notchWidth", json!("huge")),
            Err(SettingsError::BadValue("notchWidth".into()))
        );
        assert_eq!(
            s.set("limitSwitchAt", json!(1.5)),
            Err(SettingsError::BadValue("limitSwitchAt".into()))
        );
        assert_eq!(
            s.set("chatProjectFolders", json!([1])),
            Err(SettingsError::BadValue("chatProjectFolders".into()))
        );
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
        tmp.write(
            "data/settings.json",
            r#"{"version":1,"values":{"futureKey":{"a":1},"showPet":false}}"#,
        );
        let mut s = open(&tmp);
        s.set("keepPages", json!(false)).expect("set");
        let text = std::fs::read_to_string(tmp.path("data/settings.json")).expect("read");
        assert!(text.contains("futureKey"), "{text}");
        assert!(text.contains("\"showPet\""), "{text}");
    }
}
