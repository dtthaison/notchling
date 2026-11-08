//! The live login: `<CODEX_HOME>/auth.json`, shared by the Codex CLI, its managed app server and
//! the ChatGPT app. Written while holding Codex's own `auth.lock`, so a write can't land in the
//! middle of Codex renewing the file.

use crate::providers::AccountError;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub fn auth_path(home: &Path) -> PathBuf {
    home.join("auth.json")
}

/// How long a write waits for Codex to let go of `auth.lock`.
pub const LOCK_WAIT: Duration = Duration::from_secs(10);

/// Writes `auth.json` (0600, the same atomic rename the Swift app's `PrivateFile` makes; a link at
/// `auth.json` is followed, so the user's dotfile setup keeps working) while holding an exclusive
/// lock on `auth.lock`. The folder is made if it is missing. If the lock isn't free within `wait`
/// nothing is written.
pub fn write_live(home: &Path, login: &[u8], wait: Duration) -> Result<(), AccountError> {
    let failed = || AccountError::new("Couldn't write the Codex login.");
    std::fs::create_dir_all(home).map_err(|_| failed())?;
    let _lock = Lock::take(&home.join("auth.lock"), wait)
        .ok_or_else(|| AccountError::new("Codex is using its login right now. Try again."))?;
    notchling_platform::files::write_private(&auth_path(home), login).map_err(|_| failed())
}

/// An exclusive `flock` on a file, released when dropped. On systems without `flock` it holds
/// nothing and always succeeds.
struct Lock {
    #[cfg(unix)]
    file: std::fs::File,
}

impl Lock {
    #[cfg(unix)]
    fn take(path: &Path, wait: Duration) -> Option<Lock> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
            .ok()?;
        let deadline = Instant::now() + wait;
        loop {
            // SAFETY: `file` owns a valid descriptor for the whole call.
            let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if rc == 0 {
                return Some(Lock { file });
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[cfg(not(unix))]
    fn take(_path: &Path, _wait: Duration) -> Option<Lock> {
        Some(Lock {})
    }
}

#[cfg(unix)]
impl Drop for Lock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        // SAFETY: the descriptor is still open; it closes when `file` drops right after.
        unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nl-codex-live-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn writes_the_login_privately_and_makes_the_folder() {
        let home = temp("write");
        write_live(&home, b"one", Duration::from_secs(1)).unwrap();
        write_live(&home, b"two", Duration::from_secs(1)).unwrap();
        assert_eq!(std::fs::read(auth_path(&home)).unwrap(), b"two");
        assert!(home.join("auth.lock").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(auth_path(&home)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&home);
    }

    #[cfg(unix)]
    #[test]
    fn it_waits_for_codexs_lock_and_writes_nothing_without_it() {
        use std::os::fd::AsRawFd;
        let home = temp("lock");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(auth_path(&home), b"old").unwrap();
        let held = std::fs::File::create(home.join("auth.lock")).unwrap();
        // SAFETY: `held` is a valid open descriptor.
        assert_eq!(unsafe { libc::flock(held.as_raw_fd(), libc::LOCK_EX) }, 0);
        let err = write_live(&home, b"new", Duration::from_millis(150)).unwrap_err();
        assert!(err.message().contains("using its login"));
        assert_eq!(std::fs::read(auth_path(&home)).unwrap(), b"old");
        // Released a moment later: the write goes through.
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            drop(held);
        });
        write_live(&home, b"new", Duration::from_secs(5)).unwrap();
        release.join().unwrap();
        assert_eq!(std::fs::read(auth_path(&home)).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(&home);
    }
}
