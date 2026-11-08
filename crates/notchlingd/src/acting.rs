//! The `acting` switch: whether the daemon may act on sessions (type into them, answer their
//! prompts, start and stop them). Off until the maintainer hands the sessions over from the Swift
//! app (decision 0011): while it's off every acting op is refused with [`ACTING_OFF`] and the hook
//! passes.
//!
//! Every daemon run starts with it off, whatever any file says, and only an app-tier
//! `settings.set_acting` during that run turns it on. A file at rest is not a tier boundary: an
//! agent that can run a shell can write any file this user can, then restart the daemon (decision
//! 0007's note of 2026-11-08). So the switch lives in memory only, and the app, which keeps the
//! user's choice in its own settings, asserts it again after each hello.
//!
//! Milestone 3 first kept it in `<data dir>/notchlingd-settings.json`. The daemon no longer writes
//! that file; one that still says on is ignored, and the start is audit-logged
//! (`file_said_on_ignored`), since it means someone tried, or an older daemon ran.

use serde_json::Value;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// What every acting op answers while the switch is off (code `forbidden`).
pub const ACTING_OFF: &str =
    "acting is off: the Swift app owns sessions until the maintainer turns this on";

/// The settings file Milestone 3's first daemon kept the switch in, now only read to report it.
pub const SETTINGS_NAME: &str = "notchlingd-settings.json";

/// What the audit log says when that file says on at start.
pub const FILE_SAID_ON: &str = "file_said_on_ignored";

/// The switch, for this run only.
pub struct Acting {
    on: AtomicBool,
}

impl Acting {
    /// Off: how every daemon run starts.
    pub fn off() -> Acting {
        Acting::in_memory(false)
    }

    /// A switch set as given (tests).
    pub fn in_memory(on: bool) -> Acting {
        Acting {
            on: AtomicBool::new(on),
        }
    }

    pub fn is_on(&self) -> bool {
        self.on.load(Ordering::SeqCst)
    }

    /// Flips it; true when that changed it.
    pub fn set(&self, on: bool) -> bool {
        self.on.swap(on, Ordering::SeqCst) != on
    }
}

/// Whether `file` (the old settings file) says `"acting": true`. Read only to audit it.
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
    fn the_message_is_the_plans() {
        assert_eq!(
            ACTING_OFF,
            "acting is off: the Swift app owns sessions until the maintainer turns this on"
        );
    }
}
