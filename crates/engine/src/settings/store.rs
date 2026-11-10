//! The settings: one row per key in `notchling.db`, next to the `settings.json` they replace.

use super::keys::{spec, KeySpec};
use crate::store;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// Why a settings call failed.
#[derive(Debug, PartialEq, Eq)]
pub enum SettingsError {
    Io(String),
    /// The old `settings.json` exists but isn't a settings file. It is never written over or removed.
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

/// The settings as they were when opened (a write refreshes them). Reads fall back to the key's
/// default.
#[derive(Debug)]
pub struct Settings {
    /// Where `settings.json` was; its folder is the data folder that holds `notchling.db`.
    path: PathBuf,
    values: Map<String, Value>,
    /// What `settings.json` held when the database had no settings yet: the first write moves it
    /// into rows and removes the file, so opening alone writes nothing.
    old: Option<Map<String, Value>>,
}

impl Settings {
    /// Opens the settings of the data folder `path` sits in. A data folder with no settings is an
    /// empty store (nothing is written until a write), and one with only the old `settings.json`
    /// reads from it; a file that isn't a settings file is [`SettingsError::Corrupt`] and is left
    /// where it is.
    pub fn open(path: &Path) -> Result<Settings, SettingsError> {
        let rows = store::settings(data_dir_of(path)).map_err(SettingsError::Io)?;
        let (values, old) = if rows.is_empty() {
            match read_old(path)? {
                Some(old) => (old.clone(), Some(old)),
                None => (Map::new(), None),
            }
        } else {
            (from_rows(rows), None)
        };
        Ok(Settings {
            path: path.to_path_buf(),
            values,
            old,
        })
    }

    /// The old `settings.json`'s place, which names the data folder.
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

    /// Checks the value against the key and stores it. The store is unchanged on an error.
    pub fn set(&mut self, key: &str, value: Value) -> Result<(), SettingsError> {
        let spec = spec(key).ok_or_else(|| SettingsError::UnknownKey(key.to_string()))?;
        if !spec.accepts(&value) {
            return Err(SettingsError::BadValue(key.to_string()));
        }
        self.commit(vec![(key.to_string(), value)], Vec::new())
    }

    /// Stores a value for a key the engine's list doesn't have (the app's own switches: `actingOn`,
    /// `firstRunDone`...), without the key check. Only that row is written.
    pub fn set_unchecked(&mut self, key: &str, value: Value) -> Result<(), SettingsError> {
        self.commit(vec![(key.to_string(), value)], Vec::new())
    }

    /// Forgets the key (it reads as its default again).
    pub fn remove(&mut self, key: &str) -> Result<(), SettingsError> {
        self.commit(Vec::new(), vec![key.to_string()])
    }

    /// Makes the settings `values` (already checked, as the migration's are). Only the rows that
    /// differ from what was read are written, and only keys that were read and are missing from
    /// `values` are removed, so a key another process set meanwhile stays.
    pub fn replace_all(&mut self, values: Map<String, Value>) -> Result<(), SettingsError> {
        let puts = values
            .iter()
            .filter(|(k, v)| self.values.get(*k) != Some(*v))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let removes = self
            .values
            .keys()
            .filter(|k| !values.contains_key(*k))
            .cloned()
            .collect();
        self.commit(puts, removes)
    }

    /// Writes `puts` and `removes` as rows (moving the old file in first when it's still the only
    /// copy), then reads the rows back so the store shows what other writers changed too.
    fn commit(
        &mut self,
        puts: Vec<(String, Value)>,
        removes: Vec<String>,
    ) -> Result<(), SettingsError> {
        if puts.is_empty() && removes.is_empty() && self.old.is_none() {
            return Ok(());
        }
        let dir = data_dir_of(&self.path);
        let seed: Option<Vec<(String, String)>> = self.old.as_ref().map(rows_of);
        let puts: Vec<(String, String)> =
            puts.into_iter().map(|(k, v)| (k, v.to_string())).collect();
        store::update_settings(dir, seed.as_deref(), &puts, &removes).map_err(SettingsError::Io)?;
        if self.old.take().is_some() {
            // The rows are committed, so the file has been read in. One that can't be removed is
            // ignored from now on (rows win), so that failure is not worth refusing the write for.
            let _ = std::fs::remove_file(&self.path);
        }
        let rows = store::settings(dir).map_err(SettingsError::Io)?;
        self.values = from_rows(rows);
        Ok(())
    }
}

/// The data folder `settings.json` sits in.
fn data_dir_of(path: &Path) -> &Path {
    path.parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

/// The old file's values: none when there's no file (or it is a link or a folder, which is not
/// Notchling's own file to read in).
fn read_old(path: &Path) -> Result<Option<Map<String, Value>>, SettingsError> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => return Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(SettingsError::Io(e.to_string())),
    }
    match std::fs::read(path) {
        Ok(data) => parse(&data).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(SettingsError::Io(e.to_string())),
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

fn rows_of(values: &Map<String, Value>) -> Vec<(String, String)> {
    values
        .iter()
        .map(|(k, v)| (k.clone(), v.to_string()))
        .collect()
}

/// The rows as values; a row that isn't JSON (nothing here writes one) reads as unset.
fn from_rows(rows: impl IntoIterator<Item = (String, String)>) -> Map<String, Value> {
    rows.into_iter()
        .filter_map(|(k, v)| Some((k, serde_json::from_str(&v).ok()?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::super::sessions::testutil::TempDir;
    use super::*;
    use serde_json::json;

    fn open(tmp: &TempDir) -> Settings {
        Settings::open(&tmp.path("data/settings.json")).expect("open")
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("dir")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        names
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
        assert!(!tmp.path("data").exists(), "opening writes nothing");
    }

    #[test]
    fn set_saves_and_reopens() {
        let tmp = TempDir::new("set-roundtrip");
        let mut s = open(&tmp);
        s.set("showPet", json!(false)).expect("set");
        s.set("notchWidth", json!("large")).expect("set");
        s.set("limitSwitchAt", json!(80)).expect("set");
        assert_eq!(s.get_bool("showPet"), Some(false), "the store shows it too");
        let again = open(&tmp);
        assert_eq!(again.get_bool("showPet"), Some(false));
        assert_eq!(again.get_str("notchWidth").as_deref(), Some("large"));
        assert_eq!(again.get_i64("limitSwitchAt"), Some(80));
        assert_eq!(
            names(&tmp.path("data")).first().map(String::as_str),
            Some("notchling.db"),
            "the settings live in the database, not in settings.json"
        );
        assert!(!tmp.path("data/settings.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn the_database_is_mode_0600_in_a_private_new_folder() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: PathBuf| std::fs::metadata(p).expect("meta").permissions().mode() & 0o777;
        let tmp = TempDir::new("set-mode");
        let mut s = open(&tmp);
        s.set("showPet", json!(false)).expect("set");
        s.set("showPet", json!(true)).expect("set again");
        assert_eq!(mode(tmp.path("data/notchling.db")), 0o600);
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

    /// A link planted where `settings.json` was is neither read from nor removed.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_old_file_is_not_followed_or_removed() {
        let tmp = TempDir::new("set-symlink");
        let before = r#"{"version":1,"values":{"showPet":false}}"#;
        let victim = tmp.write("victim.json", before);
        std::fs::create_dir_all(tmp.path("data")).expect("dir");
        std::os::unix::fs::symlink(&victim, tmp.path("data/settings.json")).expect("link");
        let mut s = Settings::open(&tmp.path("data/settings.json")).expect("opens");
        assert_eq!(s.raw("showPet"), None, "not read through the link");
        s.set("keepPages", json!(false)).expect("set");
        assert_eq!(std::fs::read_to_string(&victim).expect("read"), before);
        assert!(tmp.path("data/settings.json").is_symlink(), "left in place");
    }

    #[test]
    fn saving_from_many_threads_at_once_never_fails() {
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
        assert!(Settings::open(&path).is_ok(), "the database is whole");
        assert!(
            !names(&tmp.path("data")).iter().any(|n| n.contains("tmp")),
            "no temp file is left"
        );
    }

    #[test]
    fn two_stores_setting_different_keys_both_keep_their_change() {
        let tmp = TempDir::new("set-two");
        let mut app = open(&tmp);
        let mut daemon = open(&tmp);
        app.set("showPet", json!(false)).expect("app");
        daemon.set("codexNetwork", json!(false)).expect("daemon");
        // The store that wrote second shows the first one's key too.
        assert_eq!(daemon.get_bool("showPet"), Some(false));
        let again = open(&tmp);
        assert_eq!(again.get_bool("showPet"), Some(false));
        assert_eq!(again.get_bool("codexNetwork"), Some(false));
    }

    #[test]
    fn two_stores_over_the_old_file_both_keep_their_change_and_it_moves_in_once() {
        let tmp = TempDir::new("set-two-old");
        tmp.write(
            "data/settings.json",
            r#"{"version":1,"values":{"notchWidth":"small"}}"#,
        );
        let mut app = open(&tmp);
        let mut daemon = open(&tmp);
        app.set("showPet", json!(false)).expect("app");
        daemon.set("codexNetwork", json!(false)).expect("daemon");
        let again = open(&tmp);
        assert_eq!(again.get_str("notchWidth").as_deref(), Some("small"));
        assert_eq!(again.get_bool("showPet"), Some(false));
        assert_eq!(again.get_bool("codexNetwork"), Some(false));
        assert!(!tmp.path("data/settings.json").exists());
    }

    #[test]
    fn unchecked_and_replace_all_writes_touch_only_their_rows() {
        let tmp = TempDir::new("set-unchecked");
        let mut a = open(&tmp);
        let mut b = open(&tmp);
        a.set_unchecked("actingOn", json!(true)).expect("unchecked");
        assert_eq!(a.raw("actingOn"), Some(&json!(true)));
        let mut values = b.values().clone();
        values.insert("firstRunDone".into(), json!(true));
        b.replace_all(values).expect("replace");
        let again = open(&tmp);
        assert_eq!(again.raw("actingOn"), Some(&json!(true)), "b never saw it");
        assert_eq!(again.raw("firstRunDone"), Some(&json!(true)));
        // A key that was read and left out is removed; one set meanwhile is not.
        let mut c = open(&tmp);
        a.set_unchecked("later", json!(1)).expect("later");
        let mut values = c.values().clone();
        values.remove("firstRunDone");
        c.replace_all(values).expect("replace");
        let end = open(&tmp);
        assert_eq!(end.raw("firstRunDone"), None);
        assert_eq!(end.raw("later"), Some(&json!(1)));
        assert_eq!(end.raw("actingOn"), Some(&json!(true)));
    }

    #[test]
    fn the_old_file_is_read_at_open_and_moved_in_by_the_first_write() {
        let tmp = TempDir::new("set-import");
        let file = tmp.write(
            "data/settings.json",
            r#"{"version":1,"values":{"futureKey":{"a":1},"showPet":false}}"#,
        );
        let mut s = open(&tmp);
        assert_eq!(s.get_bool("showPet"), Some(false), "read from the file");
        assert!(file.exists(), "opening leaves it, and makes no database");
        assert!(!tmp.path("data/notchling.db").exists());
        s.set("keepPages", json!(false)).expect("set");
        assert!(!file.exists(), "moved in, then removed");
        let again = open(&tmp);
        assert_eq!(again.raw("futureKey"), Some(&json!({"a": 1})));
        assert_eq!(again.get_bool("showPet"), Some(false));
        assert_eq!(again.get_bool("keepPages"), Some(false));
    }

    #[test]
    fn a_file_beside_existing_settings_is_ignored_and_left() {
        let tmp = TempDir::new("set-stale");
        store::put_setting(&tmp.path("data"), "showPet", "true").expect("row");
        let file = tmp.write(
            "data/settings.json",
            r#"{"version":1,"values":{"showPet":false}}"#,
        );
        let mut s = open(&tmp);
        assert_eq!(s.get_bool("showPet"), Some(true));
        s.set("keepPages", json!(false)).expect("set");
        assert!(file.exists());
        assert_eq!(open(&tmp).get_bool("showPet"), Some(true));
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
        assert!(!tmp.path("data/notchling.db").exists());
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
        assert!(!tmp.path("data").exists());
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
        let other = TempDir::new("set-clamp-file");
        other.write(
            "data/settings.json",
            r#"{"version":1,"values":{"limitSwitchAt":3}}"#,
        );
        assert_eq!(open(&other).get_i64("limitSwitchAt"), Some(80));
    }

    #[test]
    fn remove_goes_back_to_the_default() {
        let tmp = TempDir::new("set-remove");
        let mut s = open(&tmp);
        s.set("showPet", json!(false)).expect("set");
        s.remove("showPet").expect("remove");
        assert_eq!(s.raw("showPet"), None);
        assert_eq!(open(&tmp).get_bool("showPet"), Some(true));
    }

    #[test]
    fn removing_from_a_data_folder_with_nothing_makes_nothing() {
        let tmp = TempDir::new("set-remove-none");
        open(&tmp).remove("showPet").expect("remove");
        assert!(!tmp.path("data").exists());
    }
}
