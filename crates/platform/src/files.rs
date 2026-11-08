//! Private files: written atomically, readable by the owner only.
//!
//! On Unix the file is created 0600 and folders 0700. On Windows the default access control of the
//! user's profile folders already limits them to the user, so only the atomic write applies.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::Result;

/// A temp file beside `target`, so the final rename stays on one filesystem.
fn temp_beside(target: &Path) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    );
    target.with_file_name(temp)
}

fn create_private(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Writes `data` to `path` so no reader sees a half-written file and no other user can read it:
/// a 0600 temp file in the same folder, flushed to disk, then renamed over the target. The temp
/// file is removed when anything fails, and the old content stays.
pub fn write_private(path: &Path, data: &[u8]) -> Result<()> {
    let temp = temp_beside(path);
    let result = (|| -> std::io::Result<()> {
        let mut file = create_private(&temp)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        sync_parent(path);
        Ok(())
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&temp);
        return Err(e.into());
    }
    Ok(())
}

/// Makes the rename itself durable. A failure here only loses durability, so it is ignored.
fn sync_parent(path: &Path) {
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Creates `path` and any missing parents as 0700 folders, and tightens `path` itself if it
/// already exists with looser access.
pub fn ensure_private_dir(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path)?.permissions().mode();
        if mode & 0o077 != 0 {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod testdir {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A scratch folder under the system temp dir, removed on drop.
    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new() -> TempDir {
            static N: AtomicU32 = AtomicU32::new(0);
            let name = format!(
                "notchling-platform-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst)
            );
            let path = std::env::temp_dir().join(name);
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("make temp dir");
            TempDir(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testdir::TempDir;
    use super::*;

    #[test]
    fn writes_and_replaces_content() {
        let dir = TempDir::new();
        let file = dir.path().join("token");
        write_private(&file, b"one").unwrap();
        write_private(&file, b"two").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"two");
    }

    #[test]
    fn leaves_no_temp_files_behind() {
        let dir = TempDir::new();
        write_private(&dir.path().join("a"), b"x").unwrap();
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("a")]);
    }

    #[test]
    fn failure_leaves_the_old_file_and_no_temp() {
        let dir = TempDir::new();
        // The target is a directory, so the rename fails.
        let target = dir.path().join("t");
        std::fs::create_dir(&target).unwrap();
        assert!(write_private(&target, b"x").is_err());
        let count = std::fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(count, 1);
    }

    #[test]
    fn missing_parent_is_an_error() {
        let dir = TempDir::new();
        assert!(write_private(&dir.path().join("no/such/file"), b"x").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn file_is_mode_0600_even_under_a_loose_umask() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new();
        let file = dir.path().join("secret");
        write_private(&file, b"x").unwrap();
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        // Replacing keeps the mode private too.
        write_private(&file, b"y").unwrap();
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn private_dir_is_created_with_parents() {
        let dir = TempDir::new();
        let nested = dir.path().join("a/b/c");
        ensure_private_dir(&nested).unwrap();
        assert!(nested.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&nested).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
    }

    #[cfg(unix)]
    #[test]
    fn existing_loose_dir_is_tightened() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new();
        let d = dir.path().join("loose");
        std::fs::create_dir(&d).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
        ensure_private_dir(&d).unwrap();
        let mode = std::fs::metadata(&d).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[test]
    fn dir_over_a_file_is_an_error() {
        let dir = TempDir::new();
        let f = dir.path().join("f");
        std::fs::write(&f, b"x").unwrap();
        assert!(ensure_private_dir(&f).is_err());
    }
}
