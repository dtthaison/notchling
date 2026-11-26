#![cfg(unix)]

use std::path::{Path, PathBuf};

pub(crate) fn open_under(root: &Path) -> Vec<PathBuf> {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut found: Vec<PathBuf> = open_paths()
        .into_iter()
        .filter(|path| path.starts_with(&root))
        .collect();
    found.sort();
    found
}

#[cfg(target_os = "linux")]
fn open_paths() -> Vec<PathBuf> {
    let Ok(dir) = std::fs::read_dir("/proc/self/fd") else {
        return Vec::new();
    };
    dir.flatten()
        .filter_map(|entry| std::fs::read_link(entry.path()).ok())
        .collect()
}

#[cfg(not(target_os = "linux"))]
fn open_paths() -> Vec<PathBuf> {
    let Ok(dir) = std::fs::read_dir("/dev/fd") else {
        return Vec::new();
    };
    dir.flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<i32>().ok())
        .filter_map(path_of)
        .collect()
}

#[cfg(not(target_os = "linux"))]
fn path_of(fd: i32) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut buf = [0u8; libc::PATH_MAX as usize];
    let got = unsafe { libc::fcntl(fd, libc::F_GETPATH, buf.as_mut_ptr()) };
    if got != 0 {
        return None;
    }
    let end = buf.iter().position(|byte| *byte == 0)?;
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(&buf[..end])))
}

#[test]
fn open_under_sees_a_file_while_it_is_open_and_not_after() {
    let root = std::env::temp_dir().join(format!("nl-fdkit-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let file = std::fs::File::create(root.join("held")).unwrap();
    let held = open_under(&root);
    assert_eq!(held.len(), 1, "{held:?}");
    drop(file);
    assert!(open_under(&root).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}
