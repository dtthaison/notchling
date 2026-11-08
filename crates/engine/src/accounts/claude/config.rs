//! `oauthAccount` in `~/.claude.json`: read to know the main login's account, and replaced (only
//! it) on a switch. The file is Claude Code's, written often; everything else in it stays byte for
//! byte ([`set_top_level`]), a copy of the old file is kept beside it
//! (`.claude.json.notchling-backup`, as the Swift app names it) and both keep the file's mode.

use crate::accounts::json_splice::set_top_level;
use crate::accounts::Login;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// The backup beside `file`.
pub fn backup_path(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".notchling-backup");
    PathBuf::from(name)
}

/// The `oauthAccount` block, when the file has one.
pub fn oauth_account(file: &Path) -> Option<Login> {
    let data = fs::read(file).ok()?;
    match serde_json::from_slice::<Value>(&data).ok()? {
        Value::Object(map) => Login(map).object("oauthAccount"),
        _ => None,
    }
}

/// The file's text with `oauthAccount` set to `account`, checked but not written.
pub fn spliced(file: &Path, account: &Value) -> Result<(String, String), String> {
    let text =
        fs::read_to_string(file).map_err(|_| format!("{} can't be read.", file.display()))?;
    let out = set_top_level(&text, "oauthAccount", account)
        .map_err(|e| format!("{}: {e}", file.display()))?;
    Ok((text, out))
}

/// Sets `oauthAccount`, keeping every other byte, the mode, and a backup of the old text. Claude
/// Code may write the file meanwhile, so the text is read again just before the write and the
/// change made on what is there then.
pub fn set_account(file: &Path, account: &Value) -> Result<(), String> {
    let real = fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let mode = file_mode(&real);
    for _ in 0..3 {
        let (text, out) = spliced(&real, account)?;
        if fs::read_to_string(&real).ok().as_deref() != Some(text.as_str()) {
            continue;
        }
        let backup = backup_path(file);
        notchling_platform::files::write_private(&backup, text.as_bytes())
            .map_err(|e| format!("Couldn't keep a backup of {}: {e}", file.display()))?;
        set_mode(&backup, mode);
        notchling_platform::files::write_private(&real, out.as_bytes())
            .map_err(|e| format!("Couldn't write {}: {e}", file.display()))?;
        set_mode(&real, mode);
        return Ok(());
    }
    Err(format!("{} kept changing; try again.", file.display()))
}

#[cfg(unix)]
fn file_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .ok()
        .map(|m| m.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn file_mode(_path: &Path) -> Option<u32> {
    None
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: Option<u32>) {
    use std::os::unix::fs::PermissionsExt;
    if let Some(mode) = mode {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    }
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: Option<u32>) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::testutil::TempDir;
    use serde_json::json;

    #[test]
    fn only_oauth_account_changes_and_the_old_file_is_kept() {
        let tmp = TempDir::new("claude-json");
        let text = "{\n  \"numStartups\": 3,\n  \"oauthAccount\": {\"accountUuid\": \"a\", \"organizationUuid\": \"o\"},\n  \"tipsHistory\": {\"x\": 1.0}\n}\n";
        let file = tmp.write(".claude.json", text);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        }
        assert_eq!(
            oauth_account(&file).and_then(|a| a.str("accountUuid").map(String::from)),
            Some("a".into())
        );
        set_account(&file, &json!({"accountUuid": "b"})).unwrap();
        let after = fs::read_to_string(&file).unwrap();
        assert_eq!(
            after,
            "{\n  \"numStartups\": 3,\n  \"oauthAccount\": {\n    \"accountUuid\": \"b\"\n  },\n  \"tipsHistory\": {\"x\": 1.0}\n}\n"
        );
        assert_eq!(fs::read_to_string(backup_path(&file)).unwrap(), text);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&file).unwrap().permissions().mode() & 0o777,
                0o644
            );
            assert_eq!(
                fs::metadata(backup_path(&file))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o644
            );
        }
    }

    #[test]
    fn a_missing_or_broken_file_is_left_alone() {
        let tmp = TempDir::new("claude-json-bad");
        let missing = tmp.path(".claude.json");
        assert!(set_account(&missing, &json!({})).is_err());
        assert!(!missing.exists());
        let broken = tmp.write(".claude.json", "{not json");
        assert!(set_account(&broken, &json!({})).is_err());
        assert_eq!(fs::read_to_string(&broken).unwrap(), "{not json");
        assert!(!backup_path(&broken).exists());
    }
}
