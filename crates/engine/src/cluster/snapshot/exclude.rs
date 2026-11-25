pub use crate::cluster::grant_files::EXCLUDED_DIRS;
use std::path::{Component, Path};

pub const MAX_FILE: u64 = 100 * 1024 * 1024;

pub fn excluded_dir(path: &Path) -> bool {
    let parts: Vec<Component> = path.components().collect();
    if parts
        .iter()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return true;
    }
    EXCLUDED_DIRS.iter().any(|dir| {
        let want: Vec<Component> = Path::new(dir).components().collect();
        parts.windows(want.len()).any(|window| window == want)
    })
}

pub fn excluded(path: &Path) -> bool {
    if excluded_dir(path) {
        return true;
    }
    match std::fs::symlink_metadata(path) {
        Ok(meta) => meta.is_file() && meta.len() > MAX_FILE,
        Err(_) => false,
    }
}
