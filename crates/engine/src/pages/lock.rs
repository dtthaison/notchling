use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::time::Duration;

const TRIES: u32 = 40;
const PAUSE: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub(crate) struct DirLock {
    _file: File,
    #[cfg(not(unix))]
    path: std::path::PathBuf,
}

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

#[cfg(not(unix))]
pub(crate) fn acquire(path: &Path) -> io::Result<Option<DirLock>> {
    let marker = path.with_extension("busy");
    for _ in 0..TRIES {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)
        {
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
