//! One daemon per socket: an exclusive `flock` on a lock file beside the socket, held for the
//! daemon's life. The kernel drops it when the process ends, crash included, so a stale lock file
//! never blocks the next start; and once the lock is ours, a socket file left behind is stale.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

/// The lock file for a socket: `notchlingd.sock` → `notchlingd.lock`, in the same folder.
pub fn lock_path_for(socket: &Path) -> PathBuf {
    socket.with_extension("lock")
}

/// A held lock. Dropping it releases the lock (the file stays).
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
    path: PathBuf,
}

impl InstanceLock {
    /// Takes the lock without waiting; `Ok(None)` when another process (or another open file in
    /// this one) holds it.
    #[cfg(unix)]
    pub fn try_acquire(path: &Path) -> io::Result<Option<InstanceLock>> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)?;
        // SAFETY: the descriptor is open for the duration of the call.
        let got = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if got != 0 {
            let e = io::Error::last_os_error();
            return match e.raw_os_error() {
                Some(code) if code == libc::EWOULDBLOCK => Ok(None),
                _ => Err(e),
            };
        }
        Ok(Some(InstanceLock {
            _file: file,
            path: path.to_path_buf(),
        }))
    }

    /// Windows: the named pipe's single-instance flag will do this job; not yet.
    #[cfg(not(unix))]
    pub fn try_acquire(_path: &Path) -> io::Result<Option<InstanceLock>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "the daemon lock on this system",
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn the_lock_sits_beside_the_socket() {
        assert_eq!(
            lock_path_for(Path::new("/d/notchlingd.sock")),
            PathBuf::from("/d/notchlingd.lock")
        );
    }

    #[test]
    fn a_second_holder_is_refused_until_the_first_lets_go() {
        let dir = TempDir::new("lock");
        let path = dir.path().join("notchlingd.lock");
        let first = InstanceLock::try_acquire(&path).unwrap().unwrap();
        assert!(InstanceLock::try_acquire(&path).unwrap().is_none());
        drop(first);
        assert!(InstanceLock::try_acquire(&path).unwrap().is_some());
    }

    #[test]
    fn a_leftover_lock_file_does_not_block() {
        let dir = TempDir::new("lock-stale");
        let path = dir.path().join("notchlingd.lock");
        std::fs::write(&path, b"").unwrap();
        assert!(InstanceLock::try_acquire(&path).unwrap().is_some());
    }

    #[test]
    fn the_lock_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new("lock-mode");
        let path = dir.path().join("notchlingd.lock");
        let _lock = InstanceLock::try_acquire(&path).unwrap().unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
