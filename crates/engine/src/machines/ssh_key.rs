use std::path::{Path, PathBuf};

pub const MAX_LEN: usize = 1024;
pub const SHAPE_PROBLEM: &str = "SSH key: the full path of a key file, or one starting with ~/";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyIssue {
    BadShape,
    Missing,
    NotAFile,
    TooOpen,
    Unreadable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyStatus {
    pub issue: Option<KeyIssue>,
    pub public: bool,
}

impl KeyStatus {
    pub fn is_usable(&self) -> bool {
        self.issue.is_none()
    }
}

pub fn shape_ok(path: &str) -> bool {
    let typed = path.trim();
    !typed.is_empty()
        && typed.len() <= MAX_LEN
        && !typed.chars().any(|c| c.is_control() || c == '%')
        && (typed.starts_with('/') || typed.starts_with("~/"))
        && !typed.ends_with('/')
}

pub fn expand(path: &str, home: &Path) -> PathBuf {
    let typed = path.trim();
    match typed.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(typed),
    }
}

pub fn tidy(path: &str, home: &Path) -> String {
    let typed = path.trim();
    let under = home.to_string_lossy();
    let under = under.trim_end_matches('/');
    if !under.is_empty() {
        if let Some(rest) = typed.strip_prefix(under) {
            if rest.starts_with('/') && rest.len() > 1 {
                return format!("~{rest}");
            }
        }
    }
    typed.to_owned()
}

pub fn file_name(path: &str) -> String {
    path.trim()
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or("")
        .to_owned()
}

pub fn is_public(path: &str) -> bool {
    path.trim().to_ascii_lowercase().ends_with(".pub")
}

#[cfg(unix)]
fn too_open(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o077 != 0
}

#[cfg(not(unix))]
fn too_open(_meta: &std::fs::Metadata) -> bool {
    false
}

pub fn inspect(path: &str, home: &Path) -> KeyStatus {
    let public = is_public(path);
    let issue = if !shape_ok(path) {
        Some(KeyIssue::BadShape)
    } else {
        let file = expand(path, home);
        match std::fs::metadata(&file) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(KeyIssue::Missing),
            Err(_) => Some(KeyIssue::Unreadable),
            Ok(meta) if !meta.is_file() => Some(KeyIssue::NotAFile),
            Ok(meta) if too_open(&meta) && !public => Some(KeyIssue::TooOpen),
            Ok(_) => std::fs::File::open(&file)
                .err()
                .map(|_| KeyIssue::Unreadable),
        }
    };
    KeyStatus { issue, public }
}
