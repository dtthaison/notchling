use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::vault::entry::valid_name;

pub const EXCLUDED_DIRS: &[&str] = &["cluster/grants", "cluster/logins", "cluster/incoming"];

const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_VALUE_BYTES: usize = 32 * 1024;
const NO_UNIX: &str = "grant files need a unix system";
const FILE_VERSION: u32 = 1;
const MAX_TREE_DEPTH: usize = 8;

fn wipe(bytes: &mut [u8]) {
    for byte in bytes.iter_mut() {
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

fn wipe_vec(bytes: &mut Vec<u8>) {
    let capacity = bytes.capacity();
    bytes.resize(capacity, 0);
    wipe(bytes);
    bytes.clear();
}

#[derive(Clone)]
pub struct GrantValue(String);

impl GrantValue {
    pub fn new(value: String) -> GrantValue {
        GrantValue(value)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for GrantValue {
    fn drop(&mut self) {
        let mut bytes = std::mem::take(&mut self.0).into_bytes();
        wipe_vec(&mut bytes);
    }
}

impl fmt::Debug for GrantValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<hidden>")
    }
}

impl fmt::Display for GrantValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<hidden>")
    }
}

impl Serialize for GrantValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for GrantValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<GrantValue, D::Error> {
        String::deserialize(deserializer).map(GrantValue)
    }
}

#[derive(Clone, Debug)]
pub struct GrantFile {
    pub job: String,
    pub name: String,
    pub expires_ms: i64,
    pub value: GrantValue,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    v: u32,
    job: String,
    name: String,
    #[serde(rename = "expiresMs")]
    expires_ms: i64,
    value: GrantValue,
}

impl Serialize for GrantFile {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Out<'a> {
            v: u32,
            job: &'a str,
            name: &'a str,
            #[serde(rename = "expiresMs")]
            expires_ms: i64,
            value: &'a GrantValue,
        }
        Out {
            v: FILE_VERSION,
            job: &self.job,
            name: &self.name,
            expires_ms: self.expires_ms,
            value: &self.value,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for GrantFile {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<GrantFile, D::Error> {
        let stored = Stored::deserialize(deserializer)?;
        if stored.v != FILE_VERSION {
            return Err(serde::de::Error::custom("unsupported grant file version"));
        }
        Ok(GrantFile {
            job: stored.job,
            name: stored.name,
            expires_ms: stored.expires_ms,
            value: stored.value,
        })
    }
}

fn check_job(job: &str) -> Result<(), String> {
    let ok = job.strip_prefix("j-").is_some_and(|hex| {
        hex.len() == 12 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    });
    if ok {
        Ok(())
    } else {
        Err("not a job id".to_string())
    }
}

fn check_name(name: &str) -> Result<(), String> {
    if valid_name(name) {
        Ok(())
    } else {
        Err("not a secret name".to_string())
    }
}

fn file_name(name: &str) -> String {
    format!("{name}.json")
}

fn stem_of(file: &OsStr) -> Option<&str> {
    file.to_str()?
        .strip_suffix(".json")
        .filter(|stem| valid_name(stem))
}

fn read_grant(mut file: std::fs::File, path: &Path) -> Result<GrantFile, String> {
    let bad = || format!("malformed grant file {}", path.display());
    let size = file.metadata().map_or(0, |m| m.len()).min(MAX_FILE_BYTES);
    let mut buffer = Vec::with_capacity(size as usize + 1);
    let read = (&mut file)
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut buffer);
    let parsed = match read {
        Ok(n) if (n as u64) <= MAX_FILE_BYTES => {
            serde_json::from_slice::<GrantFile>(&buffer).map_err(|_| bad())
        }
        Ok(_) => Err(bad()),
        Err(e) => Err(format!("can't read grant file {}: {e}", path.display())),
    };
    wipe_vec(&mut buffer);
    parsed
}

#[cfg(unix)]
mod sys {
    use std::ffi::{CStr, CString, OsStr, OsString};
    use std::fs::{DirBuilder, File, OpenOptions};
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
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

        pub fn remove_tree(&self, name: &OsStr, depth: usize) -> io::Result<()> {
            match self.open_dir(name) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(_) => self.unlink(name),
                Ok(fd) => {
                    if depth >= MAX_TREE_DEPTH {
                        return Err(io::Error::other("folders nested too deep"));
                    }
                    let inner = Dir {
                        fd,
                        path: self.path.join(name),
                    };
                    for entry in inner.names()? {
                        inner.remove_tree(&entry, depth + 1)?;
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
mod sys {
    use std::ffi::{OsStr, OsString};
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::path::{Path, PathBuf};

    use super::MAX_TREE_DEPTH;

    pub struct Dir {
        path: PathBuf,
    }

    pub fn verify_file_private(_file: &File) -> io::Result<()> {
        Err(io::Error::other(super::NO_UNIX))
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
            Err(io::Error::other(super::NO_UNIX))
        }

        pub fn owned_by_us(&self) -> io::Result<bool> {
            Ok(true)
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

use sys::{is_not_a_folder, verify_file_private, Dir};

fn clock_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

pub struct GrantStore {
    root: PathBuf,
    lock: Mutex<()>,
}

impl GrantStore {
    pub fn new(cluster_dir: &Path) -> GrantStore {
        GrantStore {
            root: cluster_dir.join("grants"),
            lock: Mutex::new(()),
        }
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn root_dir(&self, create: bool) -> Result<Option<Dir>, String> {
        let fail = |e: std::io::Error| format!("grants folder {}: {e}", self.root.display());
        match Dir::open_root(&self.root) {
            Ok(dir) => Ok(Some(dir)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => {
                Dir::create_root(&self.root).map_err(fail)?;
                Dir::open_root(&self.root).map(Some).map_err(fail)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(fail(e)),
        }
    }

    fn job_dir(&self, root: &Dir, job: &str, create: bool) -> Result<Option<Dir>, String> {
        root.child(OsStr::new(job), create)
            .map_err(|e| format!("grant folder {}: {e}", root.path().join(job).display()))
    }

    fn private_pair(&self, root: &Dir, dir: &Dir) -> Result<(), String> {
        root.verify_private()
            .map_err(|e| format!("grants folder {}: {e}", root.path().display()))?;
        dir.verify_private()
            .map_err(|e| format!("grant folder {}: {e}", dir.path().display()))
    }

    pub fn put(&self, grant: &GrantFile) -> Result<(), String> {
        if !cfg!(unix) {
            return Err(NO_UNIX.to_string());
        }
        check_job(&grant.job)?;
        check_name(&grant.name)?;
        if grant.value.expose().len() > MAX_VALUE_BYTES {
            return Err("a secret value is too large to deliver".to_string());
        }
        if grant.expires_ms <= clock_ms() {
            return Err("grant already expired".to_string());
        }
        let name = grant.name.to_lowercase();
        let _guard = self.guard();
        let Some(root) = self.root_dir(true)? else {
            return Err("grants folder is missing".to_string());
        };
        root.verify_private()
            .map_err(|e| format!("grants folder {}: {e}", root.path().display()))?;
        let Some(dir) = self.job_dir(&root, &grant.job, true)? else {
            return Err("grant folder is missing".to_string());
        };
        self.private_pair(&root, &dir)?;
        let file = file_name(&name);
        let path = dir.path().join(&file);
        match dir.open_file(OsStr::new(&file)) {
            Ok(Some(_)) => dir
                .unlink(OsStr::new(&file))
                .map_err(|e| format!("can't replace grant file {}: {e}", path.display()))?,
            Ok(None) => {}
            Err(e) => return Err(format!("grant file {}: {e}", path.display())),
        }
        let stored = GrantFile {
            job: grant.job.clone(),
            name,
            expires_ms: grant.expires_ms,
            value: grant.value.clone(),
        };
        let mut bytes = Vec::with_capacity(stored.value.expose().len() * 6 + 256);
        let encoded = serde_json::to_writer(&mut bytes, &stored)
            .map_err(|_| format!("can't encode grant file {}", path.display()));
        let written = encoded.and_then(|()| {
            let mut out = dir
                .create_file(OsStr::new(&file))
                .map_err(|e| format!("can't write grant file {}: {e}", path.display()))?;
            let result = out.write_all(&bytes).and_then(|()| out.sync_all());
            if result.is_err() {
                let _ = dir.unlink(OsStr::new(&file));
            }
            result.map_err(|e| format!("can't write grant file {}: {e}", path.display()))
        });
        wipe_vec(&mut bytes);
        written
    }

    pub fn get(&self, job: &str, name: &str, now_ms: i64) -> Result<Option<GrantFile>, String> {
        if !cfg!(unix) {
            return Err(NO_UNIX.to_string());
        }
        check_job(job)?;
        check_name(name)?;
        let name = name.to_lowercase();
        let _guard = self.guard();
        let Some(root) = self.root_dir(false)? else {
            return Ok(None);
        };
        let Some(dir) = self.job_dir(&root, job, false)? else {
            return Ok(None);
        };
        let file = file_name(&name);
        let path = dir.path().join(&file);
        let opened = match dir.open_file(OsStr::new(&file)) {
            Ok(Some(opened)) => opened,
            Ok(None) => return Ok(None),
            Err(e) => return Err(format!("grant file {}: {e}", path.display())),
        };
        let private = verify_file_private(&opened);
        let grant = read_grant(opened, &path)?;
        if grant.job != job || grant.name != name {
            return Err(format!("malformed grant file {}", path.display()));
        }
        if grant.expires_ms <= now_ms {
            dir.unlink(OsStr::new(&file))
                .map_err(|e| format!("can't delete grant file {}: {e}", path.display()))?;
            return Ok(None);
        }
        private.map_err(|e| format!("grant file {}: {e}", path.display()))?;
        self.private_pair(&root, &dir)?;
        Ok(Some(grant))
    }

    pub fn remove(&self, job: &str, name: &str) -> Result<(), String> {
        check_job(job)?;
        check_name(name)?;
        let name = name.to_lowercase();
        let _guard = self.guard();
        let Some(root) = self.root_dir(false)? else {
            return Ok(());
        };
        let Some(dir) = self.job_dir(&root, job, false)? else {
            return Ok(());
        };
        let file = file_name(&name);
        dir.unlink(OsStr::new(&file)).map_err(|e| {
            format!(
                "can't delete grant file {}: {e}",
                dir.path().join(&file).display()
            )
        })?;
        owned_or_err(&dir)
    }

    pub fn remove_job(&self, job: &str) -> Result<(), String> {
        check_job(job)?;
        let _guard = self.guard();
        let Some(root) = self.root_dir(false)? else {
            return Ok(());
        };
        let outcome = remove_folder(&root, OsStr::new(job));
        outcome.and(owned_or_err(&root))
    }

    pub fn sweep(&self, now_ms: i64, live_jobs: &BTreeSet<String>) -> Result<usize, String> {
        let _guard = self.guard();
        let Some(root) = self.root_dir(false)? else {
            return Ok(0);
        };
        let mut removed = 0;
        let mut first_error: Option<String> = None;
        let names = root
            .names()
            .map_err(|e| format!("grants folder {}: {e}", self.root.display()))?;
        for entry in names {
            let live = entry
                .to_str()
                .is_some_and(|n| check_job(n).is_ok() && live_jobs.contains(n));
            let outcome = if live {
                match root.child(&entry, false) {
                    Ok(Some(dir)) => {
                        sweep_job(&dir, now_ms).and_then(|n| owned_or_err(&dir).map(|()| n))
                    }
                    Ok(None) => Ok(0),
                    Err(e) if is_not_a_folder(&e) => remove_folder(&root, &entry).map(|()| 1),
                    Err(e) => Err(format!(
                        "grant folder {}: {e}",
                        root.path().join(&entry).display()
                    )),
                }
            } else {
                remove_folder(&root, &entry).map(|()| 1)
            };
            match outcome {
                Ok(n) => removed += n,
                Err(e) => {
                    first_error.get_or_insert(e);
                }
            }
        }
        if let Err(e) = owned_or_err(&root) {
            first_error.get_or_insert(e);
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(removed),
        }
    }
}

fn remove_folder(root: &Dir, entry: &OsStr) -> Result<(), String> {
    let path = root.path().join(entry);
    let foreign = matches!(
        root.child(entry, false),
        Ok(Some(dir)) if !dir.owned_by_us().unwrap_or(false)
    );
    root.remove_tree(entry, 0)
        .map_err(|e| format!("can't delete grant folder {}: {e}", path.display()))?;
    if foreign {
        return Err(format!(
            "grant folder {} was owned by someone else",
            path.display()
        ));
    }
    Ok(())
}

fn owned_or_err(dir: &Dir) -> Result<(), String> {
    match dir.owned_by_us() {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!(
            "grant folder {} is owned by someone else",
            dir.path().display()
        )),
        Err(e) => Err(format!("grant folder {}: {e}", dir.path().display())),
    }
}

fn sweep_job(dir: &Dir, now_ms: i64) -> Result<usize, String> {
    let mut removed = 0;
    let mut first_error: Option<String> = None;
    let names = dir
        .names()
        .map_err(|e| format!("grant folder {}: {e}", dir.path().display()))?;
    for entry in names {
        let path = dir.path().join(&entry);
        let keep = match (stem_of(&entry), dir.open_file(&entry)) {
            (Some(stem), Ok(Some(file))) if verify_file_private(&file).is_ok() => {
                read_grant(file, &path).ok().is_some_and(|g| {
                    g.name == stem
                        && dir.path().file_name().and_then(|n| n.to_str()) == Some(g.job.as_str())
                        && g.expires_ms > now_ms
                })
            }
            _ => false,
        };
        if keep {
            continue;
        }
        match dir.remove_tree(&entry, 0) {
            Ok(()) => removed += 1,
            Err(e) => {
                first_error
                    .get_or_insert(format!("can't delete grant file {}: {e}", path.display()));
            }
        }
    }
    match first_error {
        Some(e) => Err(e),
        None => Ok(removed),
    }
}
