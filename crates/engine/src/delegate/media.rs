//! Generated images, videos and audio (Swift `Delegate.mediaFolder`, `media`, `copy`).
//!
//! The tools save what their image tools make in a folder per session. Roots are parameters
//! (`~/.codex/generated_images`, `~/.gemini/antigravity-cli/brain`), so tests use their own.
//!
//! A session id comes from a tool or from the caller's `--session`, and a copy lands wherever
//! `--out` says, so neither is trusted: an id that is not one plain folder name finds nothing,
//! only regular files directly in the session's folder are taken, and a symlink (the folder or a
//! file) is never followed out of it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const MEDIA_EXTENSIONS: [&str; 13] = [
    "png", "jpg", "jpeg", "webp", "gif", "heic", "mp4", "mov", "webm", "m4v", "mp3", "wav", "m4a",
];

/// A file counts when it was written this long before the run began (clock and rounding slack).
const SLACK: Duration = Duration::from_secs(1);

/// One plain folder name: no separator, not `.` or `..`, no NUL.
pub fn safe_component(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', '\0'])
}

/// `root/<session>`, or `None` when `session` is not one plain folder name.
pub fn session_folder(root: &Path, session: &str) -> Option<PathBuf> {
    safe_component(session).then(|| root.join(session))
}

/// Images, videos and audio directly in `folder`, written at or after `since`, oldest first.
pub fn media_in(folder: &Path, since: SystemTime) -> Vec<PathBuf> {
    // A folder that is a symlink may lead anywhere.
    if !fs::symlink_metadata(folder).is_ok_and(|m| m.is_dir()) {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let floor = since.checked_sub(SLACK).unwrap_or(since);
    let mut found: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let ext = path.extension()?.to_str()?.to_lowercase();
            if !MEDIA_EXTENSIONS.contains(&ext.as_str()) {
                return None;
            }
            let meta = fs::symlink_metadata(&path).ok()?;
            let modified = meta.modified().ok()?;
            (meta.is_file() && modified >= floor).then_some((modified, path))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, p)| p).collect()
}

/// `media_in` for a session under `root`; nothing for an id that is not a plain folder name.
pub fn session_media(root: &Path, session: &str, since: SystemTime) -> Vec<PathBuf> {
    session_folder(root, session).map_or_else(Vec::new, |f| media_in(&f, since))
}

/// Copies `files` into `out` (made if missing), never over an existing file: `name-2.png`, and so
/// on. Returns the new paths. A source that is not a regular file is refused.
pub fn copy_to(files: &[PathBuf], out: &Path) -> io::Result<Vec<PathBuf>> {
    fs::create_dir_all(out)?;
    files.iter().map(|file| copy_one(file, out)).collect()
}

fn copy_one(file: &Path, out: &Path) -> io::Result<PathBuf> {
    let name = file
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    if !fs::symlink_metadata(file)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", file.display()),
        ));
    }
    let mut source = open_no_follow(file)?;
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = file.extension().map(|e| e.to_string_lossy().into_owned());
    let mut target = out.join(name);
    let mut n = 2;
    loop {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Ok(mut dest) => {
                if let Err(e) = io::copy(&mut source, &mut dest) {
                    drop(dest);
                    let _ = fs::remove_file(&target);
                    return Err(e);
                }
                return Ok(target);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                target = out.join(match &ext {
                    Some(ext) => format!("{stem}-{n}.{ext}"),
                    None => format!("{stem}-{n}"),
                });
                n += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(unix)]
fn open_no_follow(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[cfg(not(unix))]
fn open_no_follow(path: &Path) -> io::Result<fs::File> {
    fs::File::open(path)
}

#[cfg(test)]
#[path = "media_tests.rs"]
mod tests;
