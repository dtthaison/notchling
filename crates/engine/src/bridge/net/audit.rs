//! `AuditLog`: every remote op that changes something, one JSON line each, in
//! `bridge/remote-log.jsonl` (0600), in the Swift app's format (sorted keys, `.iso8601` time). A
//! message's length is logged, never its text.

use super::files::{iso8601, parse_iso8601, write_private, FileLock};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The file's name in the bridge folder.
pub const FILE_NAME: &str = "remote-log.jsonl";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuditEntry {
    /// Milliseconds since the epoch (whole seconds on disk).
    pub time: i64,
    pub device: String,
    pub op: String,
    pub session: Option<String>,
    /// Length of the text that was sent, never the text itself.
    pub length: Option<i64>,
    /// Where the command came from; for the internet route this is the client's own address.
    pub address: Option<String>,
    /// For a takeover that stopped the original session: its pid.
    pub stopped_pid: Option<i64>,
}

impl AuditEntry {
    pub fn new(time: i64, device: &str, op: &str) -> AuditEntry {
        AuditEntry {
            time,
            device: device.to_string(),
            op: op.to_string(),
            ..AuditEntry::default()
        }
    }

    fn to_line(&self) -> String {
        let mut object = Map::new();
        object.insert("time".into(), json!(iso8601(self.time)));
        object.insert("device".into(), json!(self.device));
        object.insert("op".into(), json!(self.op));
        let mut optional = |key: &str, value: Option<Value>| {
            if let Some(v) = value {
                object.insert(key.into(), v);
            }
        };
        optional("session", self.session.as_ref().map(|s| json!(s)));
        optional("length", self.length.map(|n| json!(n)));
        optional("address", self.address.as_ref().map(|s| json!(s)));
        optional("stoppedPid", self.stopped_pid.map(|n| json!(n)));
        super::files::sorted(Value::Object(object)).to_string()
    }

    fn from_line(line: &str) -> Option<AuditEntry> {
        let v: Value = serde_json::from_str(line).ok()?;
        let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        Some(AuditEntry {
            time: parse_iso8601(&text("time")?)?,
            device: text("device")?,
            op: text("op")?,
            session: text("session"),
            length: v.get("length").and_then(Value::as_i64),
            address: text("address"),
            stopped_pid: v.get("stoppedPid").and_then(Value::as_i64),
        })
    }
}

pub struct AuditLog {
    file: PathBuf,
    /// Set by [`AuditLog::halt`]; held through a write, so none outlasts the halt.
    halted: Mutex<bool>,
}

impl AuditLog {
    pub fn new(file: PathBuf) -> AuditLog {
        AuditLog {
            file,
            halted: Mutex::new(false),
        }
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    /// No more lines, from now on: the bridge is going off. Waits for a write in progress.
    pub fn halt(&self) {
        *self.halted.lock().unwrap_or_else(|e| e.into_inner()) = true;
    }

    /// Appends one line. A failure is returned for the caller to report; the op itself goes on.
    pub fn append(&self, entry: &AuditEntry) -> std::io::Result<()> {
        let halted = self.halted.lock().unwrap_or_else(|e| e.into_inner());
        if *halted {
            return Err(std::io::Error::other("the bridge is off"));
        }
        let _lock = FileLock::acquire(&self.file)?;
        let mut data = match std::fs::read(&self.file) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        if !data.is_empty() && !data.ends_with(b"\n") {
            data.push(b'\n');
        }
        data.extend_from_slice(entry.to_line().as_bytes());
        data.push(b'\n');
        write_private(&self.file, &data)
    }

    /// The newest `limit` entries, newest first; a bad line is skipped.
    pub fn recent(&self, limit: usize) -> Vec<AuditEntry> {
        let Ok(text) = std::fs::read_to_string(&self.file) else {
            return Vec::new();
        };
        let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
        let start = lines.len().saturating_sub(limit);
        lines[start..]
            .iter()
            .rev()
            .filter_map(|l| AuditEntry::from_line(l))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::testutil::TempDir;

    const NOW: i64 = 1_800_000_000_000;

    #[test]
    fn a_line_has_no_message_text() {
        let dir = TempDir::new("audit-line");
        let log = AuditLog::new(dir.path("bridge/remote-log.jsonl"));
        let secret = "do not log this sentence";
        let mut entry = AuditEntry::new(NOW, "phone", "message");
        entry.session = Some("s1".into());
        entry.length = Some(secret.len() as i64);
        log.append(&entry).unwrap();
        let line = std::fs::read_to_string(log.file()).unwrap();
        assert!(!line.contains(secret));
        let object: Value = serde_json::from_str(line.trim()).unwrap();
        let keys: Vec<&String> = object.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["device", "length", "op", "session", "time"]);
        assert_eq!(object["length"], json!(secret.len()));
        assert_eq!(object["time"], json!("2027-01-15T08:00:00Z"));
        assert_eq!(log.recent(50)[0].session.as_deref(), Some("s1"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(log.file()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn recent_entries_come_newest_first_and_are_capped() {
        let dir = TempDir::new("audit-recent");
        let log = AuditLog::new(dir.path("bridge/remote-log.jsonl"));
        for index in 0..60 {
            let mut entry = AuditEntry::new(NOW, "d", "message");
            entry.session = Some(format!("s{index}"));
            entry.length = Some(1);
            log.append(&entry).unwrap();
        }
        let recent = log.recent(50);
        assert_eq!(recent.len(), 50);
        assert_eq!(recent[0].session.as_deref(), Some("s59"));
    }

    #[test]
    fn reads_the_swift_apps_lines_and_skips_bad_ones() {
        let dir = TempDir::new("audit-swift");
        let file = dir.write(
            "bridge/remote-log.jsonl",
            "{\"device\":\"mac\",\"op\":\"takeover\",\"stoppedPid\":4242,\"time\":\"2026-11-07T21:14:05Z\"}\nnot json\n",
        );
        let log = AuditLog::new(file);
        let entries = log.recent(10);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].stopped_pid, Some(4242));
        log.append(&AuditEntry::new(NOW, "phone", "pair")).unwrap();
        assert_eq!(log.recent(10)[0].op, "pair");
        assert_eq!(log.recent(10).len(), 2);
    }
}
