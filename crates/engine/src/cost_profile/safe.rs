use crate::install::ToolRoots;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

pub const BACKUP_MARK: &str = ".notchling-setup-";
pub const LEGACY_BACKUP_MARK: &str = ".notchling-backup-";
pub const KEEP_BACKUPS: usize = 20;

#[derive(Debug, PartialEq, Eq)]
pub enum Publish {
    Exists,
    Failed(String),
}

impl std::fmt::Display for Publish {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Publish::Exists => write!(f, "the file appeared; nothing was written"),
            Publish::Failed(why) => write!(f, "{why}"),
        }
    }
}

fn roots_of(roots: &ToolRoots) -> [&Path; 3] {
    [&roots.claude, &roots.codex, &roots.antigravity]
}

pub fn guard(roots: &ToolRoots, path: &Path) -> Result<(), String> {
    let root = roots_of(roots)
        .into_iter()
        .filter(|r| !r.as_os_str().is_empty() && path.starts_with(r))
        .max_by_key(|r| r.components().count())
        .ok_or_else(|| "it is outside the tools' folders".to_string())?;
    let rest = path
        .strip_prefix(root)
        .map_err(|_| "it is outside the tools' folders".to_string())?;
    let mut now = root.to_path_buf();
    for part in rest.components() {
        match part {
            Component::Normal(name) => now.push(name),
            _ => return Err("the path is not plain".to_string()),
        }
        match fs::symlink_metadata(&now) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(format!("{} is a link", now.display()));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

pub fn read_in(roots: &ToolRoots, path: &Path) -> Result<Option<String>, String> {
    guard(roots, path)?;
    super::action::read_text(path)
}

fn temp_beside(target: &Path) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    target.with_file_name(format!(
        ".{name}.cost-{}-{}.tmp",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ))
}

fn write_temp(temp: &Path, data: &[u8], mode: Option<u32>) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(temp)?;
    file.write_all(data)?;
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    file.sync_all()
}

pub fn make_parent(path: &Path) -> Result<(), String> {
    match path.parent() {
        Some(dir) => fs::create_dir_all(dir).map_err(|e| format!("the folder: {e}")),
        None => Ok(()),
    }
}

pub fn publish_new(path: &Path, data: &[u8]) -> Result<(), Publish> {
    let failed = |e: std::io::Error| Publish::Failed(e.to_string());
    let temp = temp_beside(path);
    write_temp(&temp, data, None).map_err(failed)?;
    let linked = fs::hard_link(&temp, path);
    let _ = fs::remove_file(&temp);
    match linked {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(Publish::Exists),
        Err(_) if fs::symlink_metadata(path).is_ok() => Err(Publish::Exists),
        Err(_) => create_exclusive(path, data),
    }
}

pub fn create_exclusive(path: &Path, data: &[u8]) -> Result<(), Publish> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => Publish::Exists,
        _ => Publish::Failed(e.to_string()),
    })?;
    let written = file.write_all(data).and_then(|_| file.sync_all());
    if let Err(e) = written {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(Publish::Failed(e.to_string()));
    }
    Ok(())
}

pub fn replace_if(path: &Path, expected: &str, blank_ok: bool, data: &[u8]) -> Result<(), String> {
    let same = |now: &str| now == expected || (blank_ok && now.trim().is_empty());
    let changed = || "the file changed while the plan was made; nothing was written".to_string();
    let Some(now) = super::action::read_text(path)? else {
        return Err(changed());
    };
    if !same(&now) {
        return Err(changed());
    }
    let mode = mode_of(path);
    let temp = temp_beside(path);
    write_temp(&temp, data, mode).map_err(|e| e.to_string())?;
    let result = (|| {
        match super::action::read_text(path)? {
            Some(again) if same(&again) => {}
            _ => return Err(changed()),
        }
        fs::rename(&temp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn mode_of(path: &Path) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .ok()
            .map(|m| m.permissions().mode() & 0o7777)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

pub fn backup(path: &Path, text: &str, stamp: u64) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let base = format!("{name}{BACKUP_MARK}{stamp}");
    for n in 0..1000 {
        let target = path.with_file_name(if n == 0 {
            base.clone()
        } else {
            format!("{base}-{n}")
        });
        match publish_new(&target, text.as_bytes()) {
            Ok(()) => return Ok(target),
            Err(Publish::Exists) => continue,
            Err(Publish::Failed(why)) => return Err(format!("the backup: {why}")),
        }
    }
    Err("no free name for the backup".into())
}

pub fn is_backup_of(path: &Path, backup: &Path) -> bool {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return false;
    };
    let Some(have) = backup.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return false;
    };
    backup.parent() == path.parent()
        && [BACKUP_MARK, LEGACY_BACKUP_MARK]
            .iter()
            .any(|mark| have.starts_with(&format!("{name}{mark}")))
}

pub fn prune_backups(path: &Path, pinned: &HashSet<PathBuf>) {
    let (Some(dir), Some(name)) = (
        path.parent(),
        path.file_name().map(|n| n.to_string_lossy().into_owned()),
    ) else {
        return;
    };
    let prefix = format!("{name}{BACKUP_MARK}");
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut ours: Vec<(u64, String)> = read
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            let stamp = file
                .strip_prefix(&prefix)?
                .split('-')
                .next()?
                .parse()
                .ok()?;
            Some((stamp, file))
        })
        .filter(|(_, file)| !pinned.contains(&dir.join(file)))
        .collect();
    ours.sort();
    let extra = ours.len().saturating_sub(KEEP_BACKUPS);
    for (_, file) in ours.into_iter().take(extra) {
        let _ = fs::remove_file(dir.join(file));
    }
}
