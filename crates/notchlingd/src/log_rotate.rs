use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const ROTATE_AT: u64 = 5 * 1024 * 1024;

pub const CHECK_EVERY: Duration = Duration::from_secs(60);

pub fn old_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".1");
    PathBuf::from(name)
}

#[derive(Clone, Debug)]
pub struct StderrLog {
    path: PathBuf,
    cap: u64,
    fds: Vec<i32>,
}

impl StderrLog {
    pub fn adopt(path: &Path) -> Option<StderrLog> {
        StderrLog::adopt_fds(path, ROTATE_AT, &[1, 2], 2)
    }

    pub fn adopt_fds(path: &Path, cap: u64, fds: &[i32], probe: i32) -> Option<StderrLog> {
        sys::same_file(probe, path).then(|| StderrLog {
            path: path.to_path_buf(),
            cap,
            fds: fds.to_vec(),
        })
    }

    pub fn check(&self) -> io::Result<bool> {
        let Some(&first) = self.fds.first() else {
            return Ok(false);
        };
        match sys::size(first) {
            Some(size) if size > self.cap => {}
            _ => return Ok(false),
        }
        if sys::same_file(first, &self.path) {
            std::fs::rename(&self.path, old_path(&self.path))?;
        }
        sys::reopen(&self.path, &self.fds)?;
        Ok(true)
    }
}

#[cfg(unix)]
mod sys {
    use std::fs::OpenOptions;
    use std::io;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::path::Path;

    fn stat(fd: i32) -> Option<libc::stat> {
        let mut info: libc::stat = unsafe { std::mem::zeroed() };
        (unsafe { libc::fstat(fd, &mut info) } == 0).then_some(info)
    }

    pub fn size(fd: i32) -> Option<u64> {
        let info = stat(fd)?;
        (info.st_mode & libc::S_IFMT == libc::S_IFREG).then_some(info.st_size.max(0) as u64)
    }

    #[allow(clippy::unnecessary_cast)]
    pub fn same_file(fd: i32, path: &Path) -> bool {
        let (Some(info), Ok(meta)) = (stat(fd), std::fs::metadata(path)) else {
            return false;
        };
        info.st_mode & libc::S_IFMT == libc::S_IFREG
            && info.st_ino as u64 == meta.ino()
            && info.st_dev as u64 == meta.dev()
    }

    pub fn reopen(path: &Path, fds: &[i32]) -> io::Result<()> {
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(path)?;
        for &fd in fds {
            if unsafe { libc::dup2(file.as_raw_fd(), fd) } < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

#[cfg(not(unix))]
mod sys {
    use std::io;
    use std::path::Path;

    pub fn size(_fd: i32) -> Option<u64> {
        None
    }

    pub fn same_file(_fd: i32, _path: &Path) -> bool {
        false
    }

    pub fn reopen(_path: &Path, _fds: &[i32]) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(all(test, unix))]
#[path = "log_rotate_tests.rs"]
mod tests;
