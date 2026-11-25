use serde_json::Value;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub type Watcher = Arc<dyn Fn(bool) + Send + Sync>;

pub const ACTING_OFF: &str =
    "acting is off: the Swift app owns sessions until the maintainer turns this on";

pub const SETTINGS_NAME: &str = "notchlingd-settings.json";

pub struct Acting {
    on: AtomicBool,
    watchers: Mutex<Vec<Watcher>>,
}

impl Acting {
    pub fn off() -> Acting {
        Acting::in_memory(false)
    }

    pub fn in_memory(on: bool) -> Acting {
        Acting {
            on: AtomicBool::new(on),
            watchers: Mutex::new(Vec::new()),
        }
    }

    pub fn is_on(&self) -> bool {
        self.on.load(Ordering::SeqCst)
    }

    pub fn set(&self, on: bool) -> bool {
        let changed = self.on.swap(on, Ordering::SeqCst) != on;
        if changed {
            let watchers = self
                .watchers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            for watcher in watchers {
                watcher(on);
            }
        }
        changed
    }

    pub fn watch(&self, watcher: Watcher) {
        self.watchers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(watcher);
    }
}

pub fn file_says_on(file: &Path) -> bool {
    std::fs::read(file)
        .ok()
        .and_then(|data| serde_json::from_slice::<Value>(&data).ok())
        .and_then(|v| v.get("acting").and_then(Value::as_bool))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nl-acting-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn it_starts_off_and_flips_in_memory() {
        let acting = Acting::off();
        assert!(!acting.is_on());
        assert!(acting.set(true));
        assert!(acting.is_on());
        assert!(!acting.set(true), "no change");
        assert!(acting.set(false));
        assert!(!acting.is_on());
    }

    #[test]
    fn the_old_file_is_read_only_to_report_it() {
        let dir = temp("file");
        let file = dir.join(SETTINGS_NAME);
        assert!(!file_says_on(&file));
        for odd in [
            "",
            "[]",
            "{\"acting\": \"yes\"}",
            "not json",
            "{\"acting\": false}",
        ] {
            std::fs::write(&file, odd).unwrap();
            assert!(!file_says_on(&file), "{odd}");
        }
        std::fs::write(&file, "{\"acting\": true}").unwrap();
        assert!(file_says_on(&file));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn watchers_hear_each_change_once() {
        let acting = Acting::off();
        let heard = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let ear = heard.clone();
        acting.watch(std::sync::Arc::new(move |on| ear.lock().unwrap().push(on)));
        acting.set(true);
        acting.set(true);
        acting.set(false);
        assert_eq!(*heard.lock().unwrap(), [true, false]);
    }

    #[test]
    fn the_message_is_the_plans() {
        assert_eq!(
            ACTING_OFF,
            "acting is off: the Swift app owns sessions until the maintainer turns this on"
        );
    }
}
