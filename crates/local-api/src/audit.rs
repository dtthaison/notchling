use crate::wire::Tier;
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const ROTATE_AT: u64 = 5 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AuditEntry {
    pub time_ms: u64,
    pub op: String,
    pub tier: Option<Tier>,
    pub peer: Option<String>,
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl AuditEntry {
    pub fn now(op: &str, tier: Option<Tier>, peer: Option<&Path>, result: &str) -> AuditEntry {
        let time_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        AuditEntry {
            time_ms,
            op: op.to_string(),
            tier,
            peer: peer.map(|p| p.to_string_lossy().into_owned()),
            result: result.to_string(),
            reason: None,
        }
    }

    pub fn with_reason(mut self, reason: Option<String>) -> AuditEntry {
        self.reason = reason;
        self
    }
}

const REASON_OPS: [&str; 6] = [
    "session.permission",
    "session.answer",
    "session.reply",
    "session.interrupt",
    "session.take_back",
    "session.question",
];

pub const REASON_CHARS: usize = 200;

pub fn audit_reason(op: &str, message: &str) -> Option<String> {
    if !REASON_OPS.contains(&op) {
        return None;
    }
    let one_line = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.is_empty() {
        return None;
    }
    if one_line.chars().count() <= REASON_CHARS {
        return Some(one_line);
    }
    let cut: String = one_line.chars().take(REASON_CHARS).collect();
    Some(format!("{cut}…"))
}

pub struct AuditLog {
    path: PathBuf,
    rotate_at: u64,
    lock: Mutex<()>,
}

impl AuditLog {
    pub fn new(path: PathBuf) -> AuditLog {
        AuditLog::with_rotation(path, ROTATE_AT)
    }

    pub fn with_rotation(path: PathBuf, rotate_at: u64) -> AuditLog {
        AuditLog {
            path,
            rotate_at,
            lock: Mutex::new(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn rotated_path(&self) -> PathBuf {
        let mut name = self.path.as_os_str().to_owned();
        name.push(".1");
        PathBuf::from(name)
    }

    pub fn append(&self, entry: &AuditEntry) -> io::Result<()> {
        let mut line = serde_json::to_vec(entry).map_err(io::Error::other)?;
        line.push(b'\n');
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let size = fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        if size > 0 && size + line.len() as u64 > self.rotate_at {
            fs::rename(&self.path, self.rotated_path())?;
        }
        let mut file = open_private_append(&self.path)?;
        file.write_all(&line)
    }
}

fn open_private_append(path: &Path) -> io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600);
        let file = options.open(path)?;
        if file.metadata()?.permissions().mode() & 0o077 != 0 {
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        Ok(file)
    }
    #[cfg(not(unix))]
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    fn entry(n: u32) -> AuditEntry {
        AuditEntry {
            time_ms: 1,
            op: format!("op{n}"),
            tier: Some(Tier::Agent),
            peer: Some("/bin/x".into()),
            result: "forbidden".into(),
            reason: None,
        }
    }

    #[test]
    fn a_line_has_time_op_tier_peer_and_result() {
        let dir = TempDir::new("audit");
        let log = AuditLog::new(dir.path().join("audit.jsonl"));
        log.append(&AuditEntry::now(
            "settings.set",
            Some(Tier::Agent),
            Some(Path::new("/bin/sh")),
            "forbidden",
        ))
        .unwrap();
        let text = fs::read_to_string(log.path()).unwrap();
        let v: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert!(v["time_ms"].as_u64().unwrap() > 0);
        assert_eq!(v["op"], "settings.set");
        assert_eq!(v["tier"], "agent");
        assert_eq!(v["peer"], "/bin/sh");
        assert_eq!(v["result"], "forbidden");
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_0600_even_if_it_was_looser() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new("audit-mode");
        let path = dir.path().join("audit.jsonl");
        let log = AuditLog::new(path.clone());
        log.append(&entry(1)).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        log.append(&entry(2)).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn it_rotates_keeping_one_old_file() {
        let dir = TempDir::new("audit-rotate");
        let path = dir.path().join("audit.jsonl");
        let one = serde_json::to_vec(&entry(1)).unwrap().len() as u64 + 1;
        let log = AuditLog::with_rotation(path.clone(), one * 2 + 1);
        for n in 1..=5 {
            log.append(&entry(n)).unwrap();
        }
        let now = fs::read_to_string(&path).unwrap();
        let old = fs::read_to_string(log.rotated_path()).unwrap();
        assert_eq!(now.lines().count(), 1);
        assert!(now.contains("op5"));
        assert_eq!(old.lines().count(), 2);
        assert!(old.contains("op3") && old.contains("op4"));
        let names = fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(names, 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(log.rotated_path())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn the_default_rotation_is_five_megabytes() {
        assert_eq!(ROTATE_AT, 5 * 1024 * 1024);
    }
}
