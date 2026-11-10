//! Every row's unsent reply text on disk (`chat-drafts.json` in Notchling's data folder, 0600), so
//! a quit, a crash or an install in the middle of typing finds it again. Written whole, through a
//! temp file and a rename, on each change; a file that can't be read is an empty set.

use std::collections::HashMap;
use std::path::Path;

/// The file's name in the data folder.
pub const FILE: &str = "chat-drafts.json";

/// The drafts saved at `path`, by row id; none when the file is missing or unreadable.
pub fn load(path: &Path) -> HashMap<String, String> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<HashMap<String, String>>(&bytes).ok())
        .map(|mut drafts| {
            drafts.retain(|_, text| !text.is_empty());
            drafts
        })
        .unwrap_or_default()
}

/// Writes `drafts` to `path`, replacing what was there; never through a link planted there.
pub fn save(path: &Path, drafts: &HashMap<String, String>) -> Result<(), String> {
    let bytes = serde_json::to_vec(drafts).map_err(|e| e.to_string())?;
    notchling_platform::files::write_private_no_follow(path, &bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drafts_come_back_after_a_restart() {
        let dir = crate::testutil::Scratch::new("drafts");
        let path = dir.path().join(FILE);
        let drafts = HashMap::from([
            ("job-1".to_owned(), "half a reply\nsecond line".to_owned()),
            ("codex:s1".to_owned(), "fix the build".to_owned()),
        ]);
        save(&path, &drafts).unwrap();
        assert_eq!(load(&path), drafts);
        save(&path, &HashMap::new()).unwrap();
        assert!(load(&path).is_empty());
    }

    #[test]
    fn a_missing_or_broken_file_is_no_drafts() {
        let dir = crate::testutil::Scratch::new("drafts");
        let path = dir.path().join(FILE);
        assert!(load(&path).is_empty());
        std::fs::write(&path, b"{not json").unwrap();
        assert!(load(&path).is_empty());
        std::fs::write(&path, br#"{"a":"","b":"kept"}"#).unwrap();
        assert_eq!(load(&path), HashMap::from([("b".into(), "kept".into())]));
    }

    #[cfg(unix)]
    #[test]
    fn only_you_can_read_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::testutil::Scratch::new("drafts");
        let path = dir.path().join(FILE);
        save(&path, &HashMap::from([("a".into(), "b".into())])).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
