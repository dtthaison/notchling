use std::collections::HashMap;
use std::path::Path;

use notchling_engine::store;

const OLD_FILE: &str = "chat-drafts.json";

pub fn load(data_dir: &Path) -> HashMap<String, String> {
    import_old(data_dir);
    store::drafts(data_dir).unwrap_or_default()
}

pub fn put(data_dir: &Path, row_id: &str, text: &str) -> Result<(), String> {
    store::put_draft(data_dir, row_id, text, now_ms())
}

fn import_old(data_dir: &Path) {
    let old = data_dir.join(OLD_FILE);
    let Ok(bytes) = std::fs::read(&old) else {
        return;
    };
    let Ok(drafts) = serde_json::from_slice::<HashMap<String, String>>(&bytes) else {
        return;
    };
    let moved = drafts
        .iter()
        .filter(|(_, text)| !text.is_empty())
        .all(|(id, text)| put(data_dir, id, text).is_ok());
    if moved {
        let _ = std::fs::remove_file(old);
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drafts_come_back_after_a_restart() {
        let dir = crate::testutil::Scratch::new("drafts");
        put(dir.path(), "job-1", "half a reply\nsecond line").unwrap();
        put(dir.path(), "codex:s1", "fix the build").unwrap();
        assert_eq!(
            load(dir.path()),
            HashMap::from([
                ("job-1".to_owned(), "half a reply\nsecond line".to_owned()),
                ("codex:s1".to_owned(), "fix the build".to_owned()),
            ])
        );
        put(dir.path(), "job-1", "").unwrap();
        assert_eq!(load(dir.path()).len(), 1);
    }

    #[test]
    fn the_old_json_file_is_moved_in_once() {
        let dir = crate::testutil::Scratch::new("drafts-old");
        dir.write(OLD_FILE, r#"{"a":"","b":"kept"}"#);
        assert_eq!(
            load(dir.path()),
            HashMap::from([("b".into(), "kept".into())])
        );
        assert!(!dir.path().join(OLD_FILE).exists());
        dir.write(OLD_FILE, "{not json");
        assert_eq!(load(dir.path()).len(), 1);
        assert!(
            dir.path().join(OLD_FILE).exists(),
            "a broken file is left alone"
        );
    }
}
