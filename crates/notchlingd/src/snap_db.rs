use crate::snap_copy::{create_private_file, remove_quietly};
use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, OpenFlags};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

const PAGES_PER_STEP: i32 = 256;
const STEP_PAUSE: Duration = Duration::from_millis(5);
const FINAL_PAUSE: Duration = Duration::from_millis(50);
const MAX_RESTARTS: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupPath {
    Stepped,
    Whole,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct BackupLimits {
    pub stepped_for: Duration,
    pub total: Duration,
}

impl Default for BackupLimits {
    fn default() -> Self {
        BackupLimits {
            stepped_for: Duration::from_secs(10),
            total: Duration::from_secs(60),
        }
    }
}

pub fn backup_db(src: &Path, dst: &Path) -> Result<(), String> {
    backup_db_report(src, dst).map(|_| ())
}

pub fn backup_db_report(src: &Path, dst: &Path) -> Result<BackupPath, String> {
    backup_db_limited(src, dst, &BackupLimits::default())
}

pub(crate) fn backup_db_limited(
    src: &Path,
    dst: &Path,
    limits: &BackupLimits,
) -> Result<BackupPath, String> {
    match fs::symlink_metadata(src) {
        Ok(found) if found.is_file() => {}
        Ok(_) => return Err(format!("{} is not a regular file", src.display())),
        Err(e) => return Err(format!("{}: {e}", src.display())),
    }
    let source = Connection::open_with_flags(
        src,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("{}: {e}", src.display()))?;
    source
        .busy_timeout(Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    let result = copy_db(&source, dst, limits);
    if result.is_err() {
        remove_quietly(dst);
    }
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut name = dst.as_os_str().to_owned();
        name.push(suffix);
        remove_quietly(Path::new(&name));
    }
    result
}

fn copy_db(source: &Connection, dst: &Path, limits: &BackupLimits) -> Result<BackupPath, String> {
    create_private_file(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    let mut target = Connection::open(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    let path = {
        let backup = Backup::new(source, &mut target).map_err(|e| e.to_string())?;
        run_steps(&backup, limits)?
    };
    let mode: String = target
        .pragma_update_and_check(None, "journal_mode", "DELETE", |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if !mode.eq_ignore_ascii_case("delete") {
        return Err(format!("the copy stayed in journal mode {mode}"));
    }
    target.close().map_err(|(_, e)| e.to_string())?;
    Ok(path)
}

fn run_steps(backup: &Backup<'_, '_>, limits: &BackupLimits) -> Result<BackupPath, String> {
    let started = Instant::now();
    let mut restarts = 0u32;
    let mut last_remaining = i32::MAX;
    while started.elapsed() < limits.stepped_for && restarts < MAX_RESTARTS {
        if let StepResult::Done = backup.step(PAGES_PER_STEP).map_err(|e| e.to_string())? {
            return Ok(BackupPath::Stepped);
        }
        let remaining = backup.progress().remaining;
        if remaining >= last_remaining {
            restarts += 1;
        }
        last_remaining = remaining;
        std::thread::sleep(STEP_PAUSE);
    }
    while started.elapsed() < limits.total {
        if let StepResult::Done = backup.step(-1).map_err(|e| e.to_string())? {
            return Ok(BackupPath::Whole);
        }
        std::thread::sleep(FINAL_PAUSE);
    }
    Err("the database stayed busy".to_owned())
}
