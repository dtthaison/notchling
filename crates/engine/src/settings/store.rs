//! The settings file: JSON, written atomically, mode 0600.

use super::keys::{spec, KeySpec};
use notchling_platform::files;
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

    /// The stored value, else the key's default; `limitSwitchAt` is held to 80 to 99 as Swift reads it.
    pub fn get(&self, key: &str) -> Option<Value> {
        let spec = spec(key);
        let value = self
            .values
            .get(key)
            .cloned()
            .or_else(|| spec.and_then(KeySpec::default_value))?;
        Some(match spec {
            Some(spec) => spec.normalize(value),
            None => value,
        })
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

    /// Writes the file: a temp file beside it, mode 0600, then a rename (platform's `write_private`).
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

/// Platform's private write (a uniquely named 0600 temp file, flushed, then renamed), refusing a
/// symlink: the file is Notchling's own, so a link planted in its place must not steer the write. A
/// folder that is missing is made 0700; one that is already there is not ours to change.
fn write_atomic(path: &Path, bytes: &[u8]) -> notchling_platform::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        if !dir.is_dir() {
            files::ensure_private_dir(dir)?;
        }
    }
    files::write_private_no_follow(path, bytes)
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

    /// The settings file is Notchling's own: a link planted there must not steer the write to a file
    /// of the user's (`write_private` follows links, for the lanes' shared files).
    #[cfg(unix)]
    #[test]
    fn a_symlinked_settings_file_is_refused_not_written_through() {
        let tmp = TempDir::new("set-symlink");
        let before = r#"{"version":1,"values":{}}"#;
        let victim = tmp.write("victim.json", before);
        std::fs::create_dir_all(tmp.path("data")).expect("dir");
        std::os::unix::fs::symlink(&victim, tmp.path("data/settings.json")).expect("link");
        let mut s = Settings::open(&tmp.path("data/settings.json")).expect("opens");
        assert!(s.set("showPet", json!(false)).is_err());
        assert_eq!(std::fs::read_to_string(&victim).expect("read"), before);
        assert_eq!(
            s.get_bool("showPet"),
            Some(true),
            "the failed save is undone"
        );
    }

    #[test]
    fn saving_from_many_threads_at_once_never_fails() {
        // A temp name made of the pid alone was shared by every thread: one deleted another's file.
        let tmp = TempDir::new("set-threads");
        let path = tmp.path("data/settings.json");
        std::thread::scope(|scope| {
            for t in 0..8 {
                let path = path.clone();
                scope.spawn(move || {
                    let mut s = Settings::open(&path).expect("open");
                    for i in 0..40 {
                        s.set("showPet", json!((t + i) % 2 == 0)).expect("save");
                    }
                });
            }
        });
        assert!(Settings::open(&path).is_ok(), "the file is whole");
        let names: Vec<_> = std::fs::read_dir(tmp.path("data"))
            .expect("dir")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["settings.json"], "no temp file is left");
    }

    #[cfg(unix)]
    #[test]
    fn a_new_data_folder_is_private_and_an_existing_one_is_left_alone() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: std::path::PathBuf| {
            std::fs::metadata(p).expect("meta").permissions().mode() & 0o777
        };
        let tmp = TempDir::new("set-dirmode");
        let mut s = open(&tmp);
        s.set("showPet", json!(false)).expect("set");
        assert_eq!(mode(tmp.path("data")), 0o700);

        let shared = tmp.path("shared");
        std::fs::create_dir(&shared).expect("mkdir");
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let mut s = Settings::open(&shared.join("settings.json")).expect("open");
        s.set("showPet", json!(false)).expect("set");
        assert_eq!(
            mode(shared),
            0o755,
            "a folder that was already there is not ours to change"
        );
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
    fn limit_switch_at_reads_inside_80_to_99() {
        // LimitWatcher.switchAt: min(max(v, 80), 99) on read; the stored value is left as written.
        let tmp = TempDir::new("set-clamp");
        let mut s = open(&tmp);
        for (stored, read) in [(50, 80), (80, 80), (90, 90), (99, 99), (100, 99), (-5, 80)] {
            s.set("limitSwitchAt", json!(stored)).expect("set");
            assert_eq!(s.get_i64("limitSwitchAt"), Some(read), "stored {stored}");
            assert_eq!(s.get("limitSwitchAt"), Some(json!(read)));
        }
        s.set("limitSwitchAt", json!(u64::MAX)).expect("set");
        assert_eq!(s.get_i64("limitSwitchAt"), Some(99));
        s.set("limitSwitchAt", json!(10)).expect("set");
        assert_eq!(s.raw("limitSwitchAt"), Some(&json!(10)));
        // A value another version or the migration wrote reads clamped too.
        tmp.write(
            "data/settings.json",
            r#"{"version":1,"values":{"limitSwitchAt":3}}"#,
        );
        assert_eq!(open(&tmp).get_i64("limitSwitchAt"), Some(80));
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
