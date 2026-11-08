//! Claude Desktop's files, in `~/Library/Application Support/Claude` (`ClaudeDesktopStore`'s
//! capture, restore and sign-out): the `Cookies` store, its write-ahead files, `config.json` and
//! our one-time backup `Cookies.notchling-backup`. Every function takes the folder, so a test
//! works in a temp one.
//!
//! What decides the desktop app's account is its claude.ai **web session cookies**, not the OAuth
//! token. The cookies are encrypted with a constant machine key, so a saved copy of the whole
//! store stays valid. Nothing here reads a cookie's value: the store is copied as one file.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const BACKUP_NAME: &str = "Cookies.notchling-backup";
const WRITE_AHEAD: [&str; 3] = ["Cookies-wal", "Cookies-journal", "Cookies-shm"];

pub fn cookies(claude_dir: &Path) -> PathBuf {
    claude_dir.join("Cookies")
}

pub fn backup(claude_dir: &Path) -> PathBuf {
    claude_dir.join(BACKUP_NAME)
}

fn config(claude_dir: &Path) -> PathBuf {
    claude_dir.join("config.json")
}

/// An account id that is safe to use as a file name: letters, digits and `-`, up to 64.
pub fn valid_uuid(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The account Desktop is on: `lastKnownAccountUuid` in `config.json`.
pub fn active_account(claude_dir: &Path) -> Option<String> {
    let data = std::fs::read(config(claude_dir)).ok()?;
    let root: Value = serde_json::from_slice(&data).ok()?;
    root.get("lastKnownAccountUuid")?
        .as_str()
        .map(str::to_string)
}

/// Copies the live cookie store to `mine` (0600, in a 0700 folder), unless it already is an exact
/// copy. False when there is nothing to copy or it couldn't be written.
pub fn capture(claude_dir: &Path, mine: &Path) -> bool {
    let Ok(live) = std::fs::read(cookies(claude_dir)) else {
        return false;
    };
    if live.is_empty() {
        return false;
    }
    if std::fs::read(mine).map(|e| e == live).unwrap_or(false) {
        return true;
    }
    let Some(dir) = mine.parent() else {
        return false;
    };
    notchling_platform::files::ensure_private_dir(dir).is_ok()
        && notchling_platform::files::write_private_no_follow(mine, &live).is_ok()
}

/// Backs the live cookie store up once, so there is always the file Notchling found.
fn backup_once(claude_dir: &Path) {
    let live = cookies(claude_dir);
    let copy = backup(claude_dir);
    if live.exists() && !copy.exists() {
        let _ = std::fs::copy(&live, &copy);
    }
}

/// Chromium's write-ahead files; stale ones would override a freshly restored Cookies file.
fn clear_write_ahead(claude_dir: &Path) {
    for name in WRITE_AHEAD {
        let _ = std::fs::remove_file(claude_dir.join(name));
    }
}

/// Restores a saved cookie store and marks the account active in `config.json`. Backs the live
/// cookie store up once and clears any stale write-ahead files so the restore isn't overwritten.
/// The caller has quit Desktop.
pub fn restore(claude_dir: &Path, saved: &Path, uuid: &str) -> Result<(), &'static str> {
    let data = std::fs::read(saved).map_err(|_| "the saved session can't be read")?;
    if data.is_empty() {
        return Err("the saved session is empty");
    }
    backup_once(claude_dir);
    notchling_platform::files::write_private(&cookies(claude_dir), &data)
        .map_err(|_| "the cookies can't be written")?;
    clear_write_ahead(claude_dir);
    set_active(claude_dir, Some(uuid))
}

/// Removes the live cookie session (backing it up once) so Claude Desktop opens at its sign-in
/// screen, and clears the active-account markers. The caller has quit Desktop.
pub fn sign_out(claude_dir: &Path) -> Result<(), &'static str> {
    backup_once(claude_dir);
    let _ = std::fs::remove_file(cookies(claude_dir));
    clear_write_ahead(claude_dir);
    set_active(claude_dir, None)
}

/// Points `config.json` at the account (`lastKnownAccountUuid` and `bootSideSessions`), or clears
/// both; keeps every other key and the file's permissions. A missing or unreadable config is left
/// alone.
fn set_active(claude_dir: &Path, uuid: Option<&str>) -> Result<(), &'static str> {
    let path = config(claude_dir);
    let Ok(data) = std::fs::read(&path) else {
        return Ok(());
    };
    let Ok(Value::Object(mut root)) = serde_json::from_slice::<Value>(&data) else {
        return Ok(());
    };
    match uuid {
        Some(uuid) => {
            root.insert("lastKnownAccountUuid".into(), json!(uuid));
            root.insert(
                "bootSideSessions".into(),
                json!({"accountUuid": uuid, "supported": true}),
            );
        }
        None => {
            root.remove("lastKnownAccountUuid");
            root.remove("bootSideSessions");
        }
    }
    let out =
        serde_json::to_vec(&Value::Object(root)).map_err(|_| "config.json can't be written")?;
    write_keeping_mode(&path, &out).map_err(|_| "config.json can't be written")
}

/// Writes through a temp file in the same folder and renames, keeping the old file's mode.
fn write_keeping_mode(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let temp = path.with_file_name(format!(".config.json.{}.tmp", std::process::id()));
    let result = (|| {
        std::fs::write(&temp, data)?;
        if let Ok(meta) = std::fs::metadata(path) {
            std::fs::set_permissions(&temp, meta.permissions())?;
        }
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nl-desktop-files-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_account_is_what_config_json_says() {
        let dir = temp("active");
        assert_eq!(active_account(&dir), None);
        std::fs::write(config(&dir), r#"{"lastKnownAccountUuid":"u-1"}"#).unwrap();
        assert_eq!(active_account(&dir).as_deref(), Some("u-1"));
        std::fs::write(config(&dir), "not json").unwrap();
        assert_eq!(active_account(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_capture_is_an_exact_private_copy_made_once() {
        let dir = temp("capture");
        let mine = dir.join("mine").join("u-1.cookies");
        assert!(!capture(&dir, &mine), "nothing to copy");
        std::fs::write(cookies(&dir), b"").unwrap();
        assert!(!capture(&dir, &mine), "an empty store is not a session");
        std::fs::write(cookies(&dir), b"cookie-store-1").unwrap();
        assert!(capture(&dir, &mine));
        assert_eq!(std::fs::read(&mine).unwrap(), b"cookie-store-1");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&mine).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::write(cookies(&dir), b"cookie-store-2").unwrap();
        assert!(capture(&dir, &mine));
        assert_eq!(std::fs::read(&mine).unwrap(), b"cookie-store-2");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_restore_swaps_the_store_backs_up_once_clears_write_ahead_and_marks_the_account() {
        let dir = temp("restore");
        std::fs::write(cookies(&dir), b"live-store").unwrap();
        for name in WRITE_AHEAD {
            std::fs::write(dir.join(name), b"stale").unwrap();
        }
        std::fs::write(
            config(&dir),
            r#"{"theme":"dark","lastKnownAccountUuid":"u-old","bootSideSessions":{"accountUuid":"u-old"}}"#,
        )
        .unwrap();
        let saved = dir.join("saved.cookies");
        std::fs::write(&saved, b"saved-store").unwrap();
        restore(&dir, &saved, "u-2").unwrap();
        assert_eq!(std::fs::read(cookies(&dir)).unwrap(), b"saved-store");
        assert_eq!(std::fs::read(backup(&dir)).unwrap(), b"live-store");
        for name in WRITE_AHEAD {
            assert!(!dir.join(name).exists(), "{name}");
        }
        let config: Value = serde_json::from_slice(&std::fs::read(config(&dir)).unwrap()).unwrap();
        assert_eq!(config["lastKnownAccountUuid"], "u-2");
        assert_eq!(
            config["bootSideSessions"],
            json!({"accountUuid": "u-2", "supported": true})
        );
        assert_eq!(config["theme"], "dark", "other keys stay");
        // A second restore keeps the first backup: it is the file Notchling found.
        restore(&dir, &saved, "u-2").unwrap();
        assert_eq!(std::fs::read(backup(&dir)).unwrap(), b"live-store");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_restore_from_a_missing_or_empty_session_changes_nothing() {
        let dir = temp("restore-bad");
        std::fs::write(cookies(&dir), b"live-store").unwrap();
        assert!(restore(&dir, &dir.join("nope"), "u").is_err());
        std::fs::write(dir.join("empty"), b"").unwrap();
        assert!(restore(&dir, &dir.join("empty"), "u").is_err());
        assert_eq!(std::fs::read(cookies(&dir)).unwrap(), b"live-store");
        assert!(!backup(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sign_out_removes_the_session_and_the_markers() {
        let dir = temp("signout");
        std::fs::write(cookies(&dir), b"live-store").unwrap();
        std::fs::write(dir.join("Cookies-wal"), b"x").unwrap();
        std::fs::write(
            config(&dir),
            r#"{"keep":1,"lastKnownAccountUuid":"u","bootSideSessions":{}}"#,
        )
        .unwrap();
        sign_out(&dir).unwrap();
        assert!(!cookies(&dir).exists() && !dir.join("Cookies-wal").exists());
        assert_eq!(std::fs::read(backup(&dir)).unwrap(), b"live-store");
        let config: Value = serde_json::from_slice(&std::fs::read(config(&dir)).unwrap()).unwrap();
        assert_eq!(config, json!({"keep": 1}));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_json_keeps_its_mode() {
        let dir = temp("mode");
        std::fs::write(config(&dir), r#"{"a":1}"#).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(config(&dir), std::fs::Permissions::from_mode(0o640)).unwrap();
            set_active(&dir, Some("u")).unwrap();
            assert_eq!(
                std::fs::metadata(config(&dir))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o640
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_plain_ids_are_file_names() {
        assert!(valid_uuid("3f2a-9c"));
        for bad in ["", "../x", "a/b", "a b", "a.cookies", &"x".repeat(65)] {
            assert!(!valid_uuid(bad), "{bad}");
        }
    }
}
