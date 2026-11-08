//! Private files: written atomically, readable by the owner only. [`write_private`] follows a
//! symlink (a lane's shared files are links); [`write_private_no_follow`] refuses one, for the files
//! Notchling owns.
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

/// How many links in a row are followed before it counts as a loop (Linux's own limit is 40).
const MAX_LINK_HOPS: usize = 40;

/// Follows `path` while it is a symlink, a dangling one included, to the file the write is really for.
/// A lane's shared files are links into `~/.claude`: renaming over the link would unshare them.
fn resolve_links(path: &Path) -> std::io::Result<PathBuf> {
    let mut current = path.to_path_buf();
    for _ in 0..MAX_LINK_HOPS {
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let link = fs::read_link(&current)?;
                current = match current.parent() {
                    Some(parent) if link.is_relative() => parent.join(link),
                    _ => link,
                };
            }
            _ => return Ok(current),
        }
    }
    Err(std::io::Error::other(format!(
        "too many levels of symbolic links: {}",
        path.display()
    )))
}

/// Writes `data` to `path` so no reader sees a half-written file and no other user can read it:
/// a 0600 temp file in the same folder, flushed to disk, then renamed over the target. The temp
/// file is removed when anything fails, and the old content stays. When `path` is a symlink the
/// write goes to the file it points at (the link stays a link), with the temp file beside that file.
///
/// That is for a lane's shared files, which are links into `~/.claude`. For a file Notchling owns
/// (the daemon's settings, say), use [`write_private_no_follow`]: a link planted there must not
/// steer the write.
pub fn write_private(path: &Path, data: &[u8]) -> Result<()> {
    let path = resolve_links(path)?;
    write_atomic(&path, data)
}

/// [`write_private`] for files Notchling owns: when `path` is a symlink (dangling or not) it is
/// refused with an error and nothing is written, so a link planted where the file belongs can't send
/// the write to another file. Anything else is written the same way: a 0600 temp file beside it,
/// flushed, then renamed over the old content.
///
/// A link swapped in between the check and the rename is replaced, not followed: the temp file is
/// created exclusively and `rename` replaces the link itself, so no target is ever opened.
pub fn write_private_no_follow(path: &Path, data: &[u8]) -> Result<()> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("refusing to write through a symlink: {}", path.display()),
            )
            .into());
        }
    }
    write_atomic(path, data)
}

/// The temp file and rename both writes share. `path` is written as given (links resolved by the caller).
fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
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
/// already exists with looser access. A symlink is left as it is: the folder it points at (a lane's
/// shared one, say) is not ours to change.
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
        if fs::symlink_metadata(path)?.file_type().is_symlink() {
            return Ok(());
        }
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

    #[cfg(unix)]
    #[test]
    fn writing_to_a_symlink_writes_through_to_its_target() {
        // A lane's shared files are links into ~/.claude; replacing the link would unshare them.
        use std::os::unix::fs::symlink;
        let dir = TempDir::new();
        std::fs::create_dir(dir.path().join("real")).unwrap();
        let real = dir.path().join("real/settings");
        std::fs::write(&real, b"old").unwrap();
        let link = dir.path().join("link");
        symlink(&real, &link).unwrap();
        write_private(&link, b"new").unwrap();
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read(&real).unwrap(), b"new");
        assert_eq!(std::fs::read(&link).unwrap(), b"new");
        let names: Vec<_> = std::fs::read_dir(dir.path().join("real"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("settings")]);
    }

    #[cfg(unix)]
    #[test]
    fn no_follow_refuses_a_symlink_and_leaves_its_target_alone() {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new();
        let victim = dir.path().join("victim");
        std::fs::write(&victim, b"precious").unwrap();
        let link = dir.path().join("settings.json");
        symlink(&victim, &link).unwrap();
        assert!(write_private_no_follow(&link, b"attacker").is_err());
        assert_eq!(std::fs::read(&victim).unwrap(), b"precious");
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        // Nothing was left beside it either.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn no_follow_refuses_a_dangling_symlink_without_creating_its_target() {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new();
        let target = dir.path().join("not-yet");
        let link = dir.path().join("link");
        symlink(&target, &link).unwrap();
        assert!(write_private_no_follow(&link, b"x").is_err());
        assert!(!target.exists());
    }

    #[test]
    fn no_follow_writes_a_plain_file_like_write_private() {
        let dir = TempDir::new();
        let file = dir.path().join("daemon.json");
        write_private_no_follow(&file, b"one").unwrap();
        write_private_no_follow(&file, b"two").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"two");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("daemon.json")]);
    }

    #[cfg(unix)]
    #[test]
    fn a_relative_link_and_a_chain_of_links_are_followed() {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let real = dir.path().join("sub/real");
        std::fs::write(&real, b"old").unwrap();
        symlink("sub/real", dir.path().join("one")).unwrap();
        symlink("one", dir.path().join("two")).unwrap();
        write_private(&dir.path().join("two"), b"new").unwrap();
        assert_eq!(std::fs::read(&real).unwrap(), b"new");
        assert!(std::fs::symlink_metadata(dir.path().join("two"))
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_link_is_filled_in_at_its_target() {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new();
        let target = dir.path().join("not-yet");
        symlink(&target, dir.path().join("link")).unwrap();
        write_private(&dir.path().join("link"), b"hello").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"hello");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_loop_is_an_error_not_a_hang() {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new();
        symlink(dir.path().join("b"), dir.path().join("a")).unwrap();
        symlink(dir.path().join("a"), dir.path().join("b")).unwrap();
        assert!(write_private(&dir.path().join("a"), b"x").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_dir_is_not_chmodded_through_the_link() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = TempDir::new();
        let shared = dir.path().join("shared");
        std::fs::create_dir(&shared).unwrap();
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o755)).unwrap();
        let link = dir.path().join("link");
        symlink(&shared, &link).unwrap();
        ensure_private_dir(&link).unwrap();
        let mode = std::fs::metadata(&shared).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn dir_over_a_file_is_an_error() {
        let dir = TempDir::new();
        let f = dir.path().join("f");
        std::fs::write(&f, b"x").unwrap();
        assert!(ensure_private_dir(&f).is_err());
    }
}
