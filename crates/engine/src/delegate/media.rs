use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const MEDIA_EXTENSIONS: [&str; 13] = [
    "png", "jpg", "jpeg", "webp", "gif", "heic", "mp4", "mov", "webm", "m4v", "mp3", "wav", "m4a",
];

const SLACK: Duration = Duration::from_secs(1);

pub fn safe_component(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', '\0'])
}

pub fn session_folder(root: &Path, session: &str) -> Option<PathBuf> {
    safe_component(session).then(|| root.join(session))
}

pub fn media_in(folder: &Path, since: SystemTime) -> Vec<PathBuf> {
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

pub fn session_media(root: &Path, session: &str, since: SystemTime) -> Vec<PathBuf> {
    session_folder(root, session).map_or_else(Vec::new, |f| media_in(&f, since))
}

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
