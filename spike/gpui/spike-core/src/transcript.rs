//! What S5 needs from `ReplySender`: the clear-input keys, bracketed paste, and a
//! check that a typed message reached the transcript.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// `ReplySender.clearInput`: Ctrl+E, Ctrl+U, Backspace twenty times, then Ctrl+E, Ctrl+U.
pub const CLEAR_INPUT: &str = concat!(
    "\u{05}\u{15}\u{7F}\u{05}\u{15}\u{7F}\u{05}\u{15}\u{7F}\u{05}\u{15}\u{7F}\u{05}\u{15}\u{7F}",
    "\u{05}\u{15}\u{7F}\u{05}\u{15}\u{7F}\u{05}\u{15}\u{7F}\u{05}\u{15}\u{7F}\u{05}\u{15}\u{7F}",
    "\u{05}\u{15}\u{7F}\u{05}\u{15}\u{7F}\u{05}\u{15}\u{7F}\u{05}\u{15}\u{7F}\u{05}\u{15}\u{7F}",
    "\u{05}\u{15}\u{7F}\u{05}\u{15}\u{7F}\u{05}\u{15}\u{7F}\u{05}\u{15}\u{7F}\u{05}\u{15}\u{7F}",
    "\u{05}\u{15}",
);

pub fn bracketed(text: &str) -> String {
    format!("\u{1b}[200~{text}\u{1b}[201~")
}

/// True when a complete `"type":"user"` line in the last `max_bytes` has `needle`
/// in its message content (a string, or `{type:"text", text}` blocks).
pub fn tail_has_user_text(path: &Path, needle: &str, max_bytes: u64) -> io::Result<bool> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let start = len.saturating_sub(max_bytes);
    // Read one byte before the tail too: the first line is cut only when that byte
    // isn't a newline.
    let from = start.saturating_sub(1);
    f.seek(SeekFrom::Start(from))?;
    let mut buf = Vec::new();
    f.take(max_bytes + (start - from)).read_to_end(&mut buf)?;
    let starts_clean = start == 0 || buf.first() == Some(&b'\n');
    let text = String::from_utf8_lossy(if start > 0 { &buf[1..] } else { &buf[..] });
    let mut lines: Vec<&str> = text.split_inclusive('\n').collect();
    if !starts_clean && !lines.is_empty() {
        lines.remove(0); // starts mid-line
    }
    Ok(lines
        .iter()
        .filter(|l| l.ends_with('\n'))
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["type"] == "user")
        .any(|v| content_text(&v["message"]["content"]).contains(needle)))
}

fn content_text(c: &serde_json::Value) -> String {
    match c {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_with(text: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(text.as_bytes()).unwrap();
        f
    }

    #[test]
    fn matcher_finds_user_line() {
        let f = tmp_with("{\"type\":\"user\",\"message\":{\"content\":\"spike ping abc123\"}}\n");
        assert!(tail_has_user_text(f.path(), "abc123", 2_000_000).unwrap());
    }

    #[test]
    fn matcher_ignores_partial_line() {
        let f = tmp_with("{\"type\":\"user\",\"message\":{\"content\":\"spike ping abc123\"");
        assert!(!tail_has_user_text(f.path(), "abc123", 2_000_000).unwrap());
    }

    #[test]
    fn matcher_ignores_assistant_echo() {
        let f = tmp_with("{\"type\":\"assistant\",\"message\":{\"content\":\"spike ping abc123\"}}\n");
        assert!(!tail_has_user_text(f.path(), "abc123", 2_000_000).unwrap());
    }

    #[test]
    fn matcher_reads_text_blocks() {
        let f = tmp_with(
            "{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"spike ping abc123\"}]}}\n",
        );
        assert!(tail_has_user_text(f.path(), "abc123", 2_000_000).unwrap());
    }

    #[test]
    fn matcher_skips_cut_first_line_of_tail() {
        // The tail starts mid-line: the cut line must not be parsed or matched.
        let line = "{\"type\":\"user\",\"message\":{\"content\":\"spike ping abc123\"}}\n";
        let f = tmp_with(line);
        assert!(!tail_has_user_text(f.path(), "abc123", (line.len() - 5) as u64).unwrap());
    }

    #[test]
    fn matcher_keeps_whole_first_line_of_tail() {
        // The tail starts exactly after a newline: its first line is complete.
        let first = "{\"type\":\"user\",\"message\":{\"content\":\"older\"}}\n";
        let second = "{\"type\":\"user\",\"message\":{\"content\":\"spike ping abc123\"}}\n";
        let f = tmp_with(&format!("{first}{second}"));
        assert!(tail_has_user_text(f.path(), "abc123", second.len() as u64).unwrap());
    }

    #[test]
    fn clear_input_matches_swift() {
        assert_eq!(CLEAR_INPUT.len(), 20 * 3 + 2);
        assert!(CLEAR_INPUT.starts_with("\u{05}\u{15}\u{7F}") && CLEAR_INPUT.ends_with("\u{05}\u{15}"));
    }

    #[test]
    fn bracketed_wraps() {
        assert_eq!(bracketed("hi"), "\u{1b}[200~hi\u{1b}[201~");
    }
}
