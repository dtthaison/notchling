use std::path::{Component, Path, PathBuf};

pub fn clean_path(path: &str) -> Result<String, &'static str> {
    if path.is_empty() {
        return Err("path is empty");
    }
    if path.contains('\0') {
        return Err("path has a NUL");
    }
    let p = Path::new(path);
    if p.is_absolute() {
        return Err("path is absolute");
    }
    for part in p.components() {
        match part {
            Component::Normal(_) | Component::CurDir => {}
            _ => return Err("path leaves the repository"),
        }
    }
    Ok(path.to_owned())
}

pub fn folder_allowed(cwd: &Path, known: &[PathBuf]) -> bool {
    if !cwd.is_absolute() || cwd.components().any(|c| c == Component::ParentDir) {
        return false;
    }
    let Ok(real) = std::fs::canonicalize(cwd) else {
        return false;
    };
    known.iter().any(|folder| {
        std::fs::canonicalize(folder)
            .map(|k| real.starts_with(&k))
            .unwrap_or(false)
    })
}

pub fn stays_inside(root: &Path, path: &str) -> bool {
    let Ok(root) = std::fs::canonicalize(root) else {
        return false;
    };
    let mut current = root.clone();
    for part in Path::new(path).components() {
        let Component::Normal(name) = part else {
            continue;
        };
        current.push(name);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => match std::fs::canonicalize(&current) {
                Ok(real) if real.starts_with(&root) => current = real,
                _ => return false,
            },
            Ok(_) => {}
            Err(_) => return true,
        }
    }
    true
}
