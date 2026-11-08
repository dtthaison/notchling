//! One writer of `pages.json` at a time, across sessions, the Swift app and the daemon: an exclusive
//! `flock` on `<pages>/.lock`, the file the Swift app locks too. Another writer may be busy, so
//! wait a little, then give up rather than hold Claude Code.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::time::Duration;

/// How many times the lock is tried, and how long between tries (2 s in all, as the Swift app).
const TRIES: u32 = 40;
const PAUSE: Duration = Duration::from_millis(50);

/// A held lock; dropping it lets go.
#[derive(Debug)]
pub(crate) struct DirLock {
    _file: File,
    #[cfg(not(unix))]
    path: std::path::PathBuf,
}

/// Takes the lock, waiting up to [`TRIES`] × [`PAUSE`]; `Ok(None)` when it stayed busy.
#[cfg(unix)]
pub(crate) fn acquire(path: &Path) -> io::Result<Option<DirLock>> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o644)
        .open(path)?;
    for _ in 0..TRIES {
        // SAFETY: the descriptor stays open for the call.
        let got = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if got == 0 {
            return Ok(Some(DirLock { _file: file }));
        }
        let e = io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::EWOULDBLOCK) {
            return Err(e);
        }
        std::thread::sleep(PAUSE);
    }
    Ok(None)
}

/// Where there is no `flock`: an exclusively created marker file next to the lock path, removed
/// when the lock drops. The pages folder is the user's own, so a stale marker is the only risk;
/// the wait gives up the same way.
#[cfg(not(unix))]
pub(crate) fn acquire(path: &Path) -> io::Result<Option<DirLock>> {
    let marker = path.with_extension("busy");
    for _ in 0..TRIES {
        match OpenOptions::new().write(true).create_new(true).open(&marker) {
            Ok(file) => {
                return Ok(Some(DirLock {
                    _file: file,
                    path: marker,
                }))
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => std::thread::sleep(PAUSE),
            Err(e) => return Err(e),
        }
    }
    Ok(None)
}

#[cfg(not(unix))]
impl Drop for DirLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
