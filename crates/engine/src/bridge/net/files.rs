//! What the bridge's files share with the Swift app's `BridgeFiles`: the clock, Foundation's
//! `.iso8601` dates, and private writes (0600 files in a 0700 folder) under an advisory lock, so
//! two writers of the same file take turns.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

/// Milliseconds since the epoch, now.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// `2026-11-08T10:20:30Z`: Foundation's `.iso8601` (`ISO8601DateFormatter` with
/// `.withInternetDateTime`), whole seconds in UTC. Its decoder reads nothing else, so the bridge
/// writes exactly this.
pub fn iso8601(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem / 60) % 60,
        rem % 60
    )
}

/// Milliseconds for an ISO 8601 date (what [`iso8601`] writes, and fractions or offsets too).
pub fn parse_iso8601(text: &str) -> Option<i64> {
    crate::sessions::iso::parse_iso_ms(text)
}

/// Year, month and day of a day count since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// The lock file beside `file`: `.<name>.lock`.
pub fn lock_path(file: &Path) -> PathBuf {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    file.with_file_name(format!(".{name}.lock"))
}

/// Holds the exclusive lock on `file`'s lock file until dropped. Making it creates the folder
/// (0700) and the lock file, so it's taken only on the way to a write.
pub struct FileLock(File);

impl FileLock {
    pub fn acquire(file: &Path) -> std::io::Result<FileLock> {
        if let Some(dir) = file.parent() {
            notchling_platform::files::ensure_private_dir(dir).map_err(std::io::Error::other)?;
        }
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(lock_path(file))?;
        lock.lock()?;
        Ok(FileLock(lock))
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// Writes `data` to `file` as a 0600 file inside a 0700 folder, atomically, refusing a symlink.
pub fn write_private(file: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        notchling_platform::files::ensure_private_dir(dir).map_err(std::io::Error::other)?;
    }
    notchling_platform::files::write_private_no_follow(file, data).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_foundations_iso8601() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(1_793_607_666_159), "2026-11-02T08:21:06Z");
        assert_eq!(iso8601(951_782_400_000), "2000-02-29T00:00:00Z");
        for ms in [0, 951_782_400_000, 1_793_607_666_000, 4_102_444_800_000] {
            assert_eq!(parse_iso8601(&iso8601(ms)), Some(ms));
        }
    }

    #[test]
    fn the_lock_file_sits_beside() {
        assert_eq!(
            lock_path(Path::new("/x/bridge/devices.json")),
            PathBuf::from("/x/bridge/.devices.json.lock")
        );
    }
}
