//! How much an Antigravity session's context holds, from its own log: the latest model step
//! (`PLANNER_RESPONSE`) in `brain/<id>/.system_generated/logs/transcript.jsonl` counts the tokens
//! it read (`input_tokens`, plus `cache_read_tokens`, which Antigravity counts apart) and wrote.
//! The ACP stream says this only at the end of a turn, so a session opened from the list showed
//! no Context Meter until its next turn.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// How much of the log's end is read: a model step is one line, and the latest is near the end.
const TAIL_BYTES: u64 = 512 * 1024;

/// The session's log under `home`, for a Chat row id (`agy:<id>`).
pub fn log_path(home: &Path, row_id: &str) -> Option<PathBuf> {
    let id = row_id.strip_prefix("agy:")?;
    // An id is a UUID: never a path of its own.
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return None;
    }
    Some(
        home.join(".gemini/antigravity-cli/brain")
            .join(id)
            .join(".system_generated/logs/transcript.jsonl"),
    )
}

/// The tokens in the context at the latest model step, or `None` without one.
pub fn context_tokens(path: &Path) -> Option<u64> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    let text = String::from_utf8_lossy(&tail);
    text.lines().rev().find_map(step_tokens)
}

fn step_tokens(line: &str) -> Option<u64> {
    if !line.contains("PLANNER_RESPONSE") {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("type").and_then(Value::as_str) != Some("PLANNER_RESPONSE") {
        return None;
    }
    let count = |key: &str| v.get(key).and_then(Value::as_u64).unwrap_or(0);
    let total = count("input_tokens")
        .saturating_add(count("cache_read_tokens"))
        .saturating_add(count("output_tokens"));
    (total > 0).then_some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_latest_model_step_counts_its_read_cached_and_written_tokens() {
        let dir = std::env::temp_dir().join(format!("nl-agy-context-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = log_path(&dir, "agy:1d44f11b-5487").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            concat!(
                r#"{"type":"PLANNER_RESPONSE","input_tokens":10,"cache_read_tokens":5,"output_tokens":1}"#,
                "\n",
                r#"{"type":"PLANNER_RESPONSE","input_tokens":2667,"cache_read_tokens":9025,"output_tokens":23,"content":"Hi"}"#,
                "\n",
                r#"{"type":"USER_INPUT","content":"PLANNER_RESPONSE in words"}"#,
                "\n",
            ),
        )
        .unwrap();
        assert_eq!(context_tokens(&path), Some(11_715));
        assert_eq!(context_tokens(&dir.join("missing.jsonl")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_plain_antigravity_id_names_a_log() {
        let home = Path::new("/h");
        assert_eq!(
            log_path(home, "agy:abc-1").unwrap(),
            Path::new(
                "/h/.gemini/antigravity-cli/brain/abc-1/.system_generated/logs/transcript.jsonl"
            )
        );
        assert_eq!(log_path(home, "agy:../x"), None);
        assert_eq!(log_path(home, "codex:abc"), None);
        assert_eq!(log_path(home, "agy:"), None);
    }
}
