use notchling_engine::sessions::iso::parse_iso_ms;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostRecord {
    pub id: String,
    pub cwd: String,
    pub model: Option<String>,
    pub claude_session_id: Option<String>,
    pub pid: i32,
    pub started_at_ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostSettings {
    pub resume_on_start: bool,
}

impl Default for HostSettings {
    fn default() -> Self {
        HostSettings {
            resume_on_start: true,
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct FileRecord {
    #[serde(
        rename = "claudeSessionId",
        skip_serializing_if = "Option::is_none",
        default
    )]
    claude_session_id: Option<String>,
    cwd: String,
    id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    model: Option<String>,
    pid: i32,
    #[serde(rename = "startedAt")]
    started_at: String,
}

#[derive(Serialize, Deserialize)]
struct FileSettings {
    #[serde(rename = "resumeOnStart", default = "yes")]
    resume_on_start: bool,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize)]
struct StateFile {
    #[serde(default)]
    records: Vec<FileRecord>,
    #[serde(default = "default_settings")]
    settings: FileSettings,
}

fn default_settings() -> FileSettings {
    FileSettings {
        resume_on_start: true,
    }
}

#[derive(Debug)]
pub struct StateStore {
    path: PathBuf,
    inner: Mutex<(Vec<HostRecord>, HostSettings)>,
}

impl StateStore {
    pub fn new(path: impl AsRef<Path>) -> StateStore {
        let path = path.as_ref().to_path_buf();
        let parsed = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<StateFile>(&bytes).ok());
        let (records, settings) = match parsed {
            Some(file) => (
                file.records.into_iter().filter_map(from_file).collect(),
                HostSettings {
                    resume_on_start: file.settings.resume_on_start,
                },
            ),
            None => (Vec::new(), HostSettings::default()),
        };
        StateStore {
            path,
            inner: Mutex::new((records, settings)),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, (Vec<HostRecord>, HostSettings)> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn records(&self) -> Vec<HostRecord> {
        self.lock().0.clone()
    }

    pub fn settings(&self) -> HostSettings {
        self.lock().1
    }

    pub fn upsert(&self, record: HostRecord) {
        self.mutate(
            |records, _| match records.iter_mut().find(|r| r.id == record.id) {
                Some(existing) => *existing = record,
                None => records.push(record),
            },
        );
    }

    pub fn remove(&self, id: &str) {
        self.mutate(|records, _| records.retain(|r| r.id != id));
    }

    pub fn set_settings(&self, settings: HostSettings) {
        self.mutate(|_, current| *current = settings);
    }

    fn mutate(&self, change: impl FnOnce(&mut Vec<HostRecord>, &mut HostSettings)) {
        let mut guard = self.lock();
        let (records, settings) = &mut *guard;
        change(records, settings);
        let file = StateFile {
            records: records.iter().map(to_file).collect(),
            settings: FileSettings {
                resume_on_start: settings.resume_on_start,
            },
        };
        if let Err(error) = write_private(&self.path, &file) {
            eprintln!("ptyhost: {}: {error}", self.path.display());
        }
    }
}

fn from_file(record: FileRecord) -> Option<HostRecord> {
    Some(HostRecord {
        started_at_ms: parse_iso_ms(&record.started_at)?,
        id: record.id,
        cwd: record.cwd,
        model: record.model,
        claude_session_id: record.claude_session_id,
        pid: record.pid,
    })
}

fn to_file(record: &HostRecord) -> FileRecord {
    FileRecord {
        claude_session_id: record.claude_session_id.clone(),
        cwd: record.cwd.clone(),
        id: record.id.clone(),
        model: record.model.clone(),
        pid: record.pid,
        started_at: iso_seconds(record.started_at_ms),
    }
}

fn write_private(path: &Path, file: &StateFile) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(file).map_err(std::io::Error::other)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temporary = dir.join(format!(".{name}.tmp"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        let _ = std::fs::remove_file(&temporary);
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        std::io::Write::write_all(&mut out, &bytes)?;
    }
    #[cfg(not(unix))]
    std::fs::write(&temporary, &bytes)?;
    std::fs::rename(&temporary, path)
}

pub fn iso_seconds(ms: i64) -> String {
    let seconds = ms.div_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateAction {
    Nothing,
    RestartNow,
    DeferUntilIdle,
}

pub fn decide(binary_changed: bool, running_sessions: usize) -> UpdateAction {
    if !binary_changed {
        UpdateAction::Nothing
    } else if running_sessions == 0 {
        UpdateAction::RestartNow
    } else {
        UpdateAction::DeferUntilIdle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip() {
        for text in [
            "1970-01-01T00:00:00Z",
            "2026-11-08T10:00:00Z",
            "2000-02-29T23:59:59Z",
            "2100-03-01T00:00:01Z",
        ] {
            let ms = parse_iso_ms(text).expect("parses");
            assert_eq!(iso_seconds(ms), text);
        }
        assert_eq!(iso_seconds(1_794_132_000_999), "2026-11-08T10:00:00Z");
    }
}
