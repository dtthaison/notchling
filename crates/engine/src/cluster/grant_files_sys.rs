pub const NO_UNIX: &str = "grant files need a unix system";
pub const MAX_TREE_DEPTH: usize = 8;

#[cfg(unix)]
mod imp {
    use std::ffi::{CStr, CString, OsStr, OsString};
    use std::fs::{DirBuilder, File, OpenOptions};
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::path::{Path, PathBuf};

    use super::MAX_TREE_DEPTH;

    pub struct Dir {
        fd: OwnedFd,
        path: PathBuf,
    }

    fn cname(name: &OsStr) -> io::Result<CString> {
        CString::new(name.as_bytes()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
    }

    fn openat(dir: RawFd, name: &OsStr, flags: libc::c_int, mode: u32) -> io::Result<OwnedFd> {
        let name = cname(name)?;
        let raw = unsafe {
            libc::openat(
                dir,
                name.as_ptr(),
                flags | libc::O_CLOEXEC,
                libc::c_uint::from(mode),
            )
        };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { OwnedFd::from_raw_fd(raw) })
    }

    fn unlinkat(dir: RawFd, name: &OsStr, flags: libc::c_int) -> io::Result<()> {
        let name = cname(name)?;
        if unsafe { libc::unlinkat(dir, name.as_ptr(), flags) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn fchmodat_private(dir: RawFd, name: &OsStr) -> io::Result<()> {
        let name = cname(name)?;
        if unsafe { libc::fchmodat(dir, name.as_ptr(), 0o700, libc::AT_SYMLINK_NOFOLLOW) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn mkdirat(dir: RawFd, name: &OsStr) -> io::Result<()> {
        let name = cname(name)?;
        if unsafe { libc::mkdirat(dir, name.as_ptr(), 0o700) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn is_link_error(e: &io::Error) -> bool {
        matches!(e.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR))
    }

    fn link_refused() -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "refusing a link or a file where a folder belongs",
        )
    }

    pub fn is_not_a_folder(e: &io::Error) -> bool {
        e.kind() == io::ErrorKind::InvalidData
    }

    fn clear_errno() -> bool {
        #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
        unsafe {
            *libc::__error() = 0;
        }
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "emscripten"))]
        unsafe {
            *libc::__errno_location() = 0;
        }
        #[cfg(any(target_os = "netbsd", target_os = "openbsd"))]
        unsafe {
            *libc::__errno() = 0;
        }
        cfg!(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "linux",
            target_os = "android",
            target_os = "emscripten",
            target_os = "netbsd",
            target_os = "openbsd"
        ))
    }

    fn euid() -> u32 {
        unsafe { libc::geteuid() }
    }

    pub fn verify_file_private(file: &File) -> io::Result<()> {
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(io::Error::other("not a regular file"));
        }
        if meta.nlink() != 1 {
            return Err(io::Error::other("a file with more than one hard link"));
        }
        if meta.uid() != euid() {
            return Err(io::Error::other("a file owned by someone else"));
        }
        if meta.mode() & 0o077 != 0 {
            return Err(io::Error::other("a file open to group or others"));
        }
        Ok(())
    }

    impl Dir {
        pub fn open_root(path: &Path) -> io::Result<Dir> {
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
                .open(path)
                .map_err(|e| if is_link_error(&e) { link_refused() } else { e })?;
            let fd = OwnedFd::from(file);
            Ok(Dir {
                fd,
                path: path.to_path_buf(),
            })
        }

        pub fn create_root(path: &Path) -> io::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            match DirBuilder::new().mode(0o700).create(path) {
                Err(e) if e.kind() != io::ErrorKind::AlreadyExists => Err(e),
                _ => Ok(()),
            }
        }

        pub fn path(&self) -> &Path {
            &self.path
        }

        fn meta(&self) -> io::Result<std::fs::Metadata> {
            File::from(self.fd.try_clone()?).metadata()
        }

        pub fn verify_private(&self) -> io::Result<()> {
            let meta = self.meta()?;
            if !meta.is_dir() {
                return Err(io::Error::other("not a folder"));
            }
            if meta.uid() != euid() {
                return Err(io::Error::other("a folder owned by someone else"));
            }
            if meta.mode() & 0o077 != 0 {
                return Err(io::Error::other("a folder open to group or others"));
            }
            Ok(())
        }

        pub fn owned_by_us(&self) -> io::Result<bool> {
            Ok(self.meta()?.uid() == euid())
        }

        fn open_dir(&self, name: &OsStr) -> io::Result<OwnedFd> {
            openat(
                self.fd.as_raw_fd(),
                name,
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
                0,
            )
            .map_err(|e| if is_link_error(&e) { link_refused() } else { e })
        }

        pub fn child(&self, name: &OsStr, create: bool) -> io::Result<Option<Dir>> {
            let mut made = false;
            let fd = loop {
                match self.open_dir(name) {
                    Ok(fd) => break fd,
                    Err(e) if e.kind() == io::ErrorKind::NotFound && create && !made => {
                        made = true;
                        match mkdirat(self.fd.as_raw_fd(), name) {
                            Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
                            _ => {}
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                    Err(e) => return Err(e),
                }
            };
            Ok(Some(Dir {
                fd,
                path: self.path.join(name),
            }))
        }

        pub fn create_file(&self, name: &OsStr) -> io::Result<File> {
            openat(
                self.fd.as_raw_fd(),
                name,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
                0o600,
            )
            .map(File::from)
        }

        pub fn open_file(&self, name: &OsStr) -> io::Result<Option<File>> {
            let fd = match openat(
                self.fd.as_raw_fd(),
                name,
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY,
                0,
            ) {
                Ok(fd) => fd,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(e) if e.raw_os_error() == Some(libc::ELOOP) => {
                    return Err(io::Error::other("refusing a link"))
                }
                Err(e) => return Err(e),
            };
            let file = File::from(fd);
            if !file.metadata()?.is_file() {
                return Err(io::Error::other("not a regular file"));
            }
            Ok(Some(file))
        }

        pub fn unlink(&self, name: &OsStr) -> io::Result<()> {
            match unlinkat(self.fd.as_raw_fd(), name, 0) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                other => other,
            }
        }

        pub fn names(&self) -> io::Result<Vec<OsString>> {
            let raw = self.fd.try_clone()?.into_raw_fd();
            let dirp = unsafe { libc::fdopendir(raw) };
            if dirp.is_null() {
                let err = io::Error::last_os_error();
                drop(unsafe { OwnedFd::from_raw_fd(raw) });
                return Err(err);
            }
            unsafe { libc::rewinddir(dirp) };
            let mut names = Vec::new();
            let can_tell = clear_errno();
            loop {
                clear_errno();
                let entry = unsafe { libc::readdir(dirp) };
                if entry.is_null() {
                    let err = io::Error::last_os_error();
                    if can_tell && err.raw_os_error().is_some_and(|code| code != 0) {
                        unsafe { libc::closedir(dirp) };
                        return Err(err);
                    }
                    break;
                }
                let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
                if name != b"." && name != b".." {
                    names.push(OsStr::from_bytes(name).to_os_string());
                }
            }
            unsafe { libc::closedir(dirp) };
            Ok(names)
        }

        pub fn repair(&self, name: &OsStr) -> io::Result<()> {
            fchmodat_private(self.fd.as_raw_fd(), name)
        }

        fn open_dir_repairing(&self, name: &OsStr) -> io::Result<OwnedFd> {
            match self.open_dir(name) {
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                    if self.repair(name).is_err() {
                        return Err(e);
                    }
                    self.open_dir(name)
                }
                other => other,
            }
        }

        fn make_usable(&self) -> io::Result<()> {
            let meta = self.meta()?;
            if meta.uid() == euid() && meta.mode() & 0o700 != 0o700 {
                File::from(self.fd.try_clone()?)
                    .set_permissions(std::fs::Permissions::from_mode(0o700))?;
            }
            Ok(())
        }

        pub fn remove_tree(&self, name: &OsStr, depth: usize) -> io::Result<()> {
            match self.open_dir_repairing(name) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied => Err(e),
                Err(_) => self.unlink(name),
                Ok(fd) => {
                    if depth >= MAX_TREE_DEPTH {
                        return Err(io::Error::other("folders nested too deep"));
                    }
                    let inner = Dir {
                        fd,
                        path: self.path.join(name),
                    };
                    inner.make_usable()?;
                    let mut first_error: Option<io::Error> = None;
                    for entry in inner.names()? {
                        if let Err(e) = inner.remove_tree(&entry, depth + 1) {
                            first_error.get_or_insert(e);
                        }
                    }
                    if let Some(e) = first_error {
                        return Err(e);
                    }
                    match unlinkat(self.fd.as_raw_fd(), name, libc::AT_REMOVEDIR) {
                        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                        other => other,
                    }
                }
            }
        }
    }
}

#[cfg(not(unix))]
mod imp {
    use std::ffi::{OsStr, OsString};
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::path::{Path, PathBuf};

    use super::{MAX_TREE_DEPTH, NO_UNIX};

    pub struct Dir {
        path: PathBuf,
    }

    pub fn verify_file_private(_file: &File) -> io::Result<()> {
        Err(io::Error::other(NO_UNIX))
    }

    pub fn is_not_a_folder(e: &io::Error) -> bool {
        e.kind() == io::ErrorKind::InvalidData
    }

    fn real_dir(path: &Path) -> io::Result<()> {
        if std::fs::symlink_metadata(path)?.is_dir() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "refusing a link or a file where a folder belongs",
            ))
        }
    }

    impl Dir {
        pub fn open_root(path: &Path) -> io::Result<Dir> {
            real_dir(path)?;
            Ok(Dir {
                path: path.to_path_buf(),
            })
        }

        pub fn create_root(path: &Path) -> io::Result<()> {
            match std::fs::create_dir_all(path) {
                Err(e) if e.kind() != io::ErrorKind::AlreadyExists => Err(e),
                _ => Ok(()),
            }
        }

        pub fn path(&self) -> &Path {
            &self.path
        }

        pub fn verify_private(&self) -> io::Result<()> {
            Err(io::Error::other(NO_UNIX))
        }

        pub fn owned_by_us(&self) -> io::Result<bool> {
            Ok(true)
        }

        pub fn repair(&self, _name: &OsStr) -> io::Result<()> {
            Ok(())
        }

        pub fn child(&self, name: &OsStr, create: bool) -> io::Result<Option<Dir>> {
            let path = self.path.join(name);
            match real_dir(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound && create => {
                    match std::fs::create_dir(&path) {
                        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
                        _ => {}
                    }
                    real_dir(&path)?;
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e),
            }
            Ok(Some(Dir { path }))
        }

        pub fn create_file(&self, name: &OsStr) -> io::Result<File> {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.path.join(name))
        }

        pub fn open_file(&self, name: &OsStr) -> io::Result<Option<File>> {
            let path = self.path.join(name);
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_file() => File::open(&path).map(Some),
                Ok(_) => Err(io::Error::other("refusing a link or a special file")),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e),
            }
        }

        pub fn unlink(&self, name: &OsStr) -> io::Result<()> {
            match std::fs::remove_file(self.path.join(name)) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                other => other,
            }
        }

        pub fn names(&self) -> io::Result<Vec<OsString>> {
            std::fs::read_dir(&self.path)?
                .map(|entry| entry.map(|e| e.file_name()))
                .collect()
        }

        pub fn remove_tree(&self, name: &OsStr, depth: usize) -> io::Result<()> {
            let path = self.path.join(name);
            match std::fs::symlink_metadata(&path) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
                Ok(meta) if meta.is_dir() => {
                    if depth >= MAX_TREE_DEPTH {
                        return Err(io::Error::other("folders nested too deep"));
                    }
                    let inner = Dir { path: path.clone() };
                    for entry in inner.names()? {
                        inner.remove_tree(&entry, depth + 1)?;
                    }
                    std::fs::remove_dir(&path)
                }
                Ok(_) => self.unlink(name),
            }
        }
    }
}

pub use imp::{is_not_a_folder, verify_file_private, Dir};
