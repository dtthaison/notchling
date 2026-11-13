use super::hooks::{plan_hooks, HookPlan, Refused, Wanted};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const MAX_BYTES: u64 = 4 * 1024 * 1024;
pub const KEEP_BACKUPS: usize = 20;
pub const BACKUP_SUFFIX: &str = ".notchling-backup";

#[derive(Debug)]
pub enum FileError {
    Refused(String),
    NoFolder(PathBuf),
    Changed,
    Io(String),
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileError::Refused(why) => write!(f, "{why}"),
            FileError::NoFolder(dir) => write!(f, "{} does not exist", dir.display()),
            FileError::Changed => write!(
                f,
                "settings.json changed while it was being updated; nothing was written"
            ),
            FileError::Io(why) => write!(f, "{why}"),
        }
    }
}

impl From<Refused> for FileError {
    fn from(r: Refused) -> FileError {
        FileError::Refused(r.0)
    }
}

fn io(what: &str, e: std::io::Error) -> FileError {
    FileError::Io(format!("{what}: {e}"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Missing,
    Empty,
    Present,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Missing => "missing",
            State::Empty => "empty",
            State::Present => "present",
        }
    }
}

#[derive(Debug)]
pub struct Loaded {
    pub target: PathBuf,
    pub state: State,
    pub text: String,
    pub read_only: bool,
    pub mode: Option<u32>,
}

pub fn load(path: &Path) -> Result<Loaded, FileError> {
    let missing = |link: bool| -> Result<Loaded, FileError> {
        if link {
            return Err(FileError::Refused(
                "settings.json is a link to a file that is not there; it was left alone".into(),
            ));
        }
        let dir = path.parent().unwrap_or(Path::new("."));
        if !dir.is_dir() {
            return Err(FileError::NoFolder(dir.to_path_buf()));
        }
        Ok(Loaded {
            target: path.to_path_buf(),
            state: State::Missing,
            text: String::new(),
            read_only: false,
            mode: None,
        })
    };
    let linked = fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
    let target = match fs::canonicalize(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return missing(linked),
        Err(e) => return Err(io("settings.json", e)),
    };
    let meta = fs::metadata(&target).map_err(|e| io("settings.json", e))?;
    if !meta.is_file() {
        return Err(FileError::Refused(
            "settings.json is not a regular file; it was left alone".into(),
        ));
    }
    if meta.len() > MAX_BYTES {
        return Err(FileError::Refused(
            "settings.json is too big to edit safely; it was left alone".into(),
        ));
    }
    let bytes = fs::read(&target).map_err(|e| io("settings.json", e))?;
    let text = String::from_utf8(bytes).map_err(|_| {
        FileError::Refused("settings.json is not UTF-8 text; it was left alone".into())
    })?;
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        Some(meta.permissions().mode() & 0o7777)
    };
    #[cfg(not(unix))]
    let mode = None;
    Ok(Loaded {
        target,
        state: if super::jsonc::is_blank(&text) {
            State::Empty
        } else {
            State::Present
        },
        text,
        read_only: meta.permissions().readonly(),
        mode,
    })
}

#[derive(Debug)]
pub struct FilePlan {
    pub path: PathBuf,
    pub state: State,
    pub read_only: bool,
    pub hooks: HookPlan,
}

pub fn plan(path: &Path, wanted: &Wanted) -> Result<FilePlan, FileError> {
    let loaded = load(path)?;
    plan_loaded(path, &loaded, wanted)
}

fn plan_loaded(path: &Path, loaded: &Loaded, wanted: &Wanted) -> Result<FilePlan, FileError> {
    Ok(FilePlan {
        path: path.to_path_buf(),
        state: loaded.state,
        read_only: loaded.read_only,
        hooks: plan_hooks(&loaded.text, wanted)?,
    })
}

#[derive(Debug)]
pub struct Applied {
    pub plan: FilePlan,
    pub wrote: bool,
    pub backup: Option<PathBuf>,
}

pub fn apply(path: &Path, wanted: &Wanted, stamp: u64) -> Result<Applied, FileError> {
    let loaded = load(path)?;
    let plan = plan_loaded(path, &loaded, wanted)?;
    if !plan.hooks.changed {
        return Ok(Applied {
            plan,
            wrote: false,
            backup: None,
        });
    }
    if loaded.read_only {
        return Err(FileError::Refused(
            "settings.json is read-only; it was left alone".into(),
        ));
    }
    let backup = match loaded.state {
        State::Missing => None,
        _ => Some(write_backup(path, &loaded.text, stamp)?),
    };
    write_checked(&loaded, plan.hooks.text.as_bytes())?;
    if let Some(dir) = path.parent() {
        prune_backups(dir, path);
    }
    Ok(Applied {
        plan,
        wrote: true,
        backup,
    })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "settings.json".into())
}

fn write_backup(path: &Path, text: &str, stamp: u64) -> Result<PathBuf, FileError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let base = format!("{}{BACKUP_SUFFIX}-{stamp}", file_name(path));
    for n in 0..100 {
        let name = if n == 0 {
            base.clone()
        } else {
            format!("{base}-{n}")
        };
        let target = dir.join(name);
        if fs::symlink_metadata(&target).is_ok() {
            continue;
        }
        notchling_platform::files::write_private_no_follow(&target, text.as_bytes())
            .map_err(|e| FileError::Io(format!("the backup: {e}")))?;
        return Ok(target);
    }
    Err(FileError::Io("no free name for the backup".into()))
}

fn prune_backups(dir: &Path, path: &Path) {
    let prefix = format!("{}{BACKUP_SUFFIX}-", file_name(path));
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut ours: Vec<(u64, String)> = read
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let stamp = name
                .strip_prefix(&prefix)?
                .split('-')
                .next()?
                .parse()
                .ok()?;
            Some((stamp, name))
        })
        .collect();
    ours.sort();
    let extra = ours.len().saturating_sub(KEEP_BACKUPS);
    for (_, name) in ours.into_iter().take(extra) {
        let _ = fs::remove_file(dir.join(name));
    }
}

pub(super) fn write_checked(loaded: &Loaded, data: &[u8]) -> Result<(), FileError> {
    let target = &loaded.target;
    let dir = target.parent().unwrap_or(Path::new("."));
    let temp = dir.join(format!(
        ".{}.notchling-{}.tmp",
        file_name(target),
        std::process::id()
    ));
    let result = (|| -> Result<(), FileError> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp).map_err(|e| io("the temp file", e))?;
        file.write_all(data).map_err(|e| io("the temp file", e))?;
        #[cfg(unix)]
        if let Some(mode) = loaded.mode {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(mode))
                .map_err(|e| io("the temp file", e))?;
        }
        file.sync_all().map_err(|e| io("the temp file", e))?;
        drop(file);
        let now = fs::read(target).ok();
        match (loaded.state, now) {
            (State::Missing, None) => {
                if fs::hard_link(&temp, target).is_err() {
                    if target.exists() {
                        return Err(FileError::Changed);
                    }
                    fs::rename(&temp, target).map_err(|e| io("settings.json", e))?;
                }
            }
            (State::Missing, Some(_)) => return Err(FileError::Changed),
            (_, Some(bytes)) if bytes == loaded.text.as_bytes() => {
                fs::rename(&temp, target).map_err(|e| io("settings.json", e))?;
            }
            _ => return Err(FileError::Changed),
        }
        Ok(())
    })();
    let _ = fs::remove_file(&temp);
    result
}
