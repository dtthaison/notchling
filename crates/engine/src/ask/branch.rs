use std::io::Read;
use std::path::{Path, PathBuf};

const HEAD_BYTES: u64 = 4096;

fn read_small(path: &Path) -> Option<String> {
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(HEAD_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

fn name_of(head: &str) -> Option<String> {
    let head = head.trim();
    if let Some(reference) = head.strip_prefix("ref:") {
        let name = reference.trim().strip_prefix("refs/heads/")?;
        return (!name.is_empty()).then(|| name.to_string());
    }
    let hex = head.len() >= 8 && head.chars().all(|c| c.is_ascii_hexdigit());
    hex.then(|| head[..8].to_string())
}

fn git_dir(dir: &Path, dot_git: &Path) -> Option<PathBuf> {
    let meta = dot_git.metadata().ok()?;
    if meta.is_dir() {
        return Some(dot_git.to_path_buf());
    }
    let text = read_small(dot_git)?;
    let path = text.lines().next()?.strip_prefix("gitdir:")?.trim();
    (!path.is_empty()).then(|| dir.join(path))
}

pub fn branch(folder: &Path) -> Option<String> {
    let mut dir = folder;
    loop {
        let dot_git = dir.join(".git");
        if dot_git.exists() {
            let head = git_dir(dir, &dot_git)?.join("HEAD");
            return name_of(&read_small(&head)?);
        }
        dir = dir.parent()?;
    }
}
