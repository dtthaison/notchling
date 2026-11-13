use crate::sessions::iso::parse_iso_ms;
use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const TAIL_BYTES: u64 = 65_536;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn popped_all(tail: &str, text: &str, since_ms: i64) -> bool {
    tail.lines()
        .rev()
        .filter(|line| line.contains("\"popAll\""))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .any(|obj| {
            obj.get("operation").and_then(Value::as_str) == Some("popAll")
                && obj.get("content").and_then(Value::as_str) == Some(text)
                && obj
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(parse_iso_ms)
                    .is_some_and(|stamp| stamp >= since_ms)
        })
}

pub fn popped_all_in_file(path: &Path, text: &str, since_ms: i64) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let size = file.seek(SeekFrom::End(0)).unwrap_or(0);
    if file
        .seek(SeekFrom::Start(size.saturating_sub(TAIL_BYTES)))
        .is_err()
    {
        return false;
    }
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return false;
    }
    popped_all(&String::from_utf8_lossy(&bytes), text, since_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINES: [&str; 4] = [
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-05T09:58:08.490Z","content":"Then say ONE"}"#,
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-05T09:58:11.666Z","content":"And TWO"}"#,
        r#"{"type":"queue-operation","operation":"popAll","timestamp":"2026-11-05T09:58:13.059Z","content":"Then say ONE"}"#,
        r#"{"type":"queue-operation","operation":"popAll","timestamp":"2026-11-05T09:58:13.060Z","content":"And TWO"}"#,
    ];

    #[test]
    fn pop_all_takes_the_queue_back() {
        let tail = LINES.join("\n") + "\n";
        let pressed = parse_iso_ms("2026-11-05T09:58:12Z").unwrap();
        assert!(popped_all(&tail, "And TWO", pressed));
        assert!(
            !popped_all(&tail, "And TWO", pressed + 5_000),
            "an older take-back doesn't count"
        );
        assert!(!popped_all(&tail, "Something else", pressed));
        assert!(!popped_all(&tail, "Then say", pressed), "the whole text");
    }

    #[test]
    fn the_file_is_read_by_its_tail() {
        let dir = std::env::temp_dir().join(format!("nl-popall-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let filler = format!("{{\"type\":\"user\",\"x\":\"{}\"}}\n", "a".repeat(1000));
        let mut text = filler.repeat(100);
        text.push_str(&(LINES.join("\n") + "\n"));
        std::fs::write(&path, text).unwrap();
        let pressed = parse_iso_ms("2026-11-05T09:58:12Z").unwrap();
        assert!(popped_all_in_file(&path, "Then say ONE", pressed));
        assert!(!popped_all_in_file(&dir.join("missing"), "x", 0));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
