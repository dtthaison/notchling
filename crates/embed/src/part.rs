use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub(crate) fn part_path(out: &Path) -> PathBuf {
    let mut name = out.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

pub(crate) fn create_part(path: &Path) -> Result<File, String> {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", path.display())),
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW).mode(0o600);
    }
    options
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

pub fn write_atomic(out: &Path, bytes: &[u8]) -> Result<(), String> {
    let part = part_path(out);
    let mut file = create_part(&part)?;
    let finished = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("{}: {e}", part.display()))
        .and_then(|()| std::fs::rename(&part, out).map_err(|e| format!("{}: {e}", out.display())));
    if let Err(e) = finished {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    Ok(())
}
