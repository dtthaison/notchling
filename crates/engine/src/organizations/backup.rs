use std::path::{Path, PathBuf};

pub const DIR: &str = "organizations-backup";

pub fn path(data_dir: &Path, doc: &str) -> PathBuf {
    data_dir
        .join(DIR)
        .join(format!("{}.json", doc.replace('/', "-")))
}

pub fn keep(data_dir: &Path, doc: &str, text: &str) -> Result<bool, String> {
    let file = path(data_dir, doc);
    match std::fs::symlink_metadata(&file) {
        Ok(meta) if meta.is_file() => return Ok(false),
        Ok(_) => return Err(format!("{} is not a regular file", file.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    if let Some(dir) = file.parent() {
        notchling_platform::files::ensure_private_dir(dir).map_err(|e| e.to_string())?;
    }
    notchling_platform::files::write_private_no_follow(&file, text.as_bytes())
        .map_err(|e| e.to_string())?;
    if std::fs::read(&file).map_err(|e| e.to_string())? != text.as_bytes() {
        let _ = std::fs::remove_file(&file);
        return Err(format!("{} did not read back", file.display()));
    }
    Ok(true)
}
