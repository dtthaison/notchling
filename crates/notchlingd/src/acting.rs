//! The `acting` switch: whether the daemon may act on sessions (type into them, answer their
//! prompts, start and stop them). Off by default, and off until the maintainer hands the sessions
//! over from the Swift app (decision 0011): while it's off every acting op is refused with
//! [`ACTING_OFF`] and the hook passes.
//!
//! It lives in the daemon's own settings file, `<data dir>/notchlingd-settings.json`
//! (`{"acting": true}`), read once at start; `settings.set_acting` (app tier) changes it. A file
//! that can't be read leaves it off.

use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// What every acting op answers while the switch is off (code `forbidden`).
pub const ACTING_OFF: &str =
    "acting is off: the Swift app owns sessions until the maintainer turns this on";

/// The daemon's settings file in the data folder.
pub const SETTINGS_NAME: &str = "notchlingd-settings.json";

/// The switch, and the file it's kept in.
pub struct Acting {
    on: AtomicBool,
    /// `None` keeps it in memory only (tests).
    file: Option<PathBuf>,
    write: Mutex<()>,
}

impl Acting {
    /// Reads the switch from `file`; missing, unreadable or not a bool is off.
    pub fn load(file: &Path) -> Acting {
        let on = read(file)
            .and_then(|m| m.get("acting").and_then(Value::as_bool))
            .unwrap_or(false);
        Acting {
            on: AtomicBool::new(on),
            file: Some(file.to_path_buf()),
            write: Mutex::new(()),
        }
    }

    /// A switch with no file behind it.
    pub fn in_memory(on: bool) -> Acting {
        Acting {
            on: AtomicBool::new(on),
            file: None,
            write: Mutex::new(()),
        }
    }

    pub fn is_on(&self) -> bool {
        self.on.load(Ordering::SeqCst)
    }

    /// Saves, then flips. Other keys in the file are kept. A failed save leaves it as it was.
    pub fn set(&self, on: bool) -> Result<(), String> {
        let _guard = self.write.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(file) = &self.file {
            let mut values = read(file).unwrap_or_default();
            values.insert("acting".into(), Value::Bool(on));
            let text =
                serde_json::to_vec_pretty(&Value::Object(values)).map_err(|e| e.to_string())?;
            if let Some(parent) = file.parent() {
                notchling_platform::files::ensure_private_dir(parent).map_err(|e| e.to_string())?;
            }
            notchling_platform::files::write_private(file, &text).map_err(|e| e.to_string())?;
        }
        self.on.store(on, Ordering::SeqCst);
        Ok(())
    }
}

fn read(file: &Path) -> Option<Map<String, Value>> {
    let data = std::fs::read(file).ok()?;
    match serde_json::from_slice(&data).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nl-acting-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn it_is_off_by_default_and_when_the_file_is_odd() {
        let dir = temp("default");
        assert!(!Acting::load(&dir.join(SETTINGS_NAME)).is_on());
        for odd in ["", "[]", "{\"acting\": \"yes\"}", "not json"] {
            std::fs::write(dir.join(SETTINGS_NAME), odd).unwrap();
            assert!(!Acting::load(&dir.join(SETTINGS_NAME)).is_on(), "{odd}");
        }
        std::fs::write(dir.join(SETTINGS_NAME), "{\"acting\": true}").unwrap();
        assert!(Acting::load(&dir.join(SETTINGS_NAME)).is_on());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn setting_it_saves_and_keeps_other_keys() {
        let dir = temp("set");
        let file = dir.join("data").join(SETTINGS_NAME);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "{\"later\": 1}").unwrap();
        let acting = Acting::load(&file);
        acting.set(true).unwrap();
        assert!(acting.is_on());
        let saved: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(saved, serde_json::json!({"acting": true, "later": 1}));
        assert!(Acting::load(&file).is_on(), "read again at the next start");
        acting.set(false).unwrap();
        assert!(!Acting::load(&file).is_on());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_message_is_the_plans() {
        assert_eq!(
            ACTING_OFF,
            "acting is off: the Swift app owns sessions until the maintainer turns this on"
        );
    }
}
