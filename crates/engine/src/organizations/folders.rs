use super::record::{normalize_folder, Organization, PERSONAL_ID};
use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderMatch {
    pub organization: String,
    pub folder: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum AbsRoot {
    Unix,
    Drive(char),
    Unc(String, String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AbsPath {
    root: AbsRoot,
    parts: Vec<String>,
}

fn strip_verbatim(raw: &str) -> Cow<'_, str> {
    if raw.len() >= 8 && raw[..8].eq_ignore_ascii_case(r"\\?\UNC\") {
        return Cow::Owned(format!(r"\\{}", &raw[8..]));
    }
    if let Some(after) = raw.strip_prefix(r"\\?\") {
        let b = after.as_bytes();
        if b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && (b[2] == b'\\' || b[2] == b'/')
        {
            return Cow::Borrowed(after);
        }
    }
    Cow::Borrowed(raw)
}

impl AbsPath {
    fn parse(raw: &str) -> Option<AbsPath> {
        let s = strip_verbatim(raw);
        let b = s.as_bytes();
        let (root, rest) = if let Some(after) = s.strip_prefix("\\\\") {
            let sep = |c: char| c == '\\' || c == '/';
            let i = after.find(sep)?;
            let server = &after[..i];
            if server.is_empty() || server == "." || server == ".." {
                return None;
            }
            let tail = &after[i + 1..];
            if tail.starts_with(sep) {
                return None;
            }
            let (share, rest) = match tail.find(sep) {
                Some(j) => (&tail[..j], &tail[j + 1..]),
                None => (tail, ""),
            };
            if share.is_empty() || share == "." || share == ".." {
                return None;
            }
            (AbsRoot::Unc(server.to_owned(), share.to_owned()), rest)
        } else if b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && (b[2] == b'\\' || b[2] == b'/')
        {
            (AbsRoot::Drive((b[0] as char).to_ascii_uppercase()), &s[3..])
        } else {
            (AbsRoot::Unix, s.strip_prefix('/')?)
        };
        let sep: &[char] = if root == AbsRoot::Unix {
            &['/']
        } else {
            &['/', '\\']
        };
        let mut parts = Vec::new();
        for part in rest.split(sep) {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                p => parts.push(p.to_owned()),
            }
        }
        Some(AbsPath { root, parts })
    }

    fn as_string(&self) -> String {
        match (&self.root, self.parts.is_empty()) {
            (AbsRoot::Unix, true) => "/".into(),
            (AbsRoot::Unix, false) => format!("/{}", self.parts.join("/")),
            (AbsRoot::Drive(d), true) => format!("{d}:\\"),
            (AbsRoot::Drive(d), false) => format!("{d}:\\{}", self.parts.join("\\")),
            (AbsRoot::Unc(server, share), true) => format!("\\\\{server}\\{share}"),
            (AbsRoot::Unc(server, share), false) => {
                format!("\\\\{server}\\{share}\\{}", self.parts.join("\\"))
            }
        }
    }

    fn key_string(&self, fold: bool) -> String {
        let base = match (&self.root, self.parts.is_empty()) {
            (AbsRoot::Unc(server, share), true) => {
                format!(
                    "\\\\{}\\{}",
                    server.to_ascii_lowercase(),
                    share.to_ascii_lowercase()
                )
            }
            (AbsRoot::Unc(server, share), false) => format!(
                "\\\\{}\\{}\\{}",
                server.to_ascii_lowercase(),
                share.to_ascii_lowercase(),
                self.parts.join("\\")
            ),
            _ => self.as_string(),
        };
        if fold {
            base.to_lowercase()
        } else {
            base
        }
    }

    fn depth(&self) -> usize {
        match &self.root {
            AbsRoot::Unc(_, _) => 2 + self.parts.len(),
            AbsRoot::Unix | AbsRoot::Drive(_) => self.parts.len(),
        }
    }
}

pub fn standardize(path: &Path) -> Option<PathBuf> {
    let abs = AbsPath::parse(&path.to_string_lossy())?;
    Some(PathBuf::from(abs.as_string()))
}

pub fn expand(folder: &str, home: &Path) -> Option<PathBuf> {
    let norm = normalize_folder(folder)?;
    let abs = if norm == "~" {
        AbsPath::parse(&home.to_string_lossy())?
    } else if let Some(rest) = norm.strip_prefix("~/") {
        let mut base = AbsPath::parse(&home.to_string_lossy())?;
        for part in rest.split('/') {
            if !part.is_empty() {
                base.parts.push(part.to_owned());
            }
        }
        base
    } else {
        AbsPath::parse(&norm)?
    };
    Some(PathBuf::from(abs.as_string()))
}

fn depth(path: &Path) -> usize {
    match AbsPath::parse(&path.to_string_lossy()) {
        Some(abs) => abs.depth(),
        None => path
            .components()
            .filter(|c| matches!(c, Component::Normal(_)))
            .count(),
    }
}

pub fn real(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

pub fn folds_case() -> bool {
    cfg!(any(target_os = "macos", windows))
}

fn same(a: &std::ffi::OsStr, b: &std::ffi::OsStr, fold: bool) -> bool {
    if fold {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

fn same_str(a: &str, b: &str, fold: bool) -> bool {
    if fold {
        a.to_lowercase() == b.to_lowercase()
    } else {
        a == b
    }
}

fn same_root(a: &AbsRoot, b: &AbsRoot) -> bool {
    match (a, b) {
        (AbsRoot::Unix, AbsRoot::Unix) => true,
        (AbsRoot::Drive(x), AbsRoot::Drive(y)) => x == y,
        (AbsRoot::Unc(s1, sh1), AbsRoot::Unc(s2, sh2)) => {
            s1.eq_ignore_ascii_case(s2) && sh1.eq_ignore_ascii_case(sh2)
        }
        _ => false,
    }
}

pub fn within(path: &Path, base: &Path, fold: bool) -> bool {
    if let (Some(p), Some(b)) = (
        AbsPath::parse(&path.to_string_lossy()),
        AbsPath::parse(&base.to_string_lossy()),
    ) {
        return same_root(&p.root, &b.root)
            && b.parts.len() <= p.parts.len()
            && p.parts
                .iter()
                .zip(&b.parts)
                .all(|(x, y)| same_str(x, y, fold));
    }
    let mut path = path.components();
    base.components().all(|b| {
        path.next()
            .is_some_and(|p| same(p.as_os_str(), b.as_os_str(), fold))
    })
}

pub fn overlap_key(folder: &str, home: &Path, fold: bool) -> String {
    let Some(path) = expand(folder, home) else {
        return folder.to_owned();
    };
    let path = real(&path).and_then(|p| standardize(&p)).unwrap_or(path);
    if let Some(abs) = AbsPath::parse(&path.to_string_lossy()) {
        return abs.key_string(fold);
    }
    let text = path.to_string_lossy().into_owned();
    if fold {
        text.to_lowercase()
    } else {
        text
    }
}

pub fn find(
    organizations: &[Organization],
    folder: &Path,
    home: &Path,
    resolve: &dyn Fn(&Path) -> Option<PathBuf>,
) -> FolderMatch {
    find_folded(organizations, folder, home, resolve, folds_case())
}

pub fn find_folded(
    organizations: &[Organization],
    folder: &Path,
    home: &Path,
    resolve: &dyn Fn(&Path) -> Option<PathBuf>,
    fold: bool,
) -> FolderMatch {
    let personal = FolderMatch {
        organization: PERSONAL_ID.into(),
        folder: None,
    };
    let Some(session) = standardize(folder) else {
        return personal;
    };
    let session_real = resolve(&session).and_then(|p| standardize(&p));
    let mut best: Option<(usize, &str, String)> = None;
    for org in organizations {
        for claimed in &org.folders {
            let Some(path) = expand(claimed, home) else {
                continue;
            };
            let real = resolve(&path).and_then(|p| standardize(&p));
            let textual = within(&session, &path, fold);
            let canonical = match (&real, &session_real) {
                (Some(a), Some(b)) => within(b, a, fold),
                _ => false,
            };
            if !textual && !canonical {
                continue;
            }
            let length = depth(real.as_deref().unwrap_or(&path));
            let Some(norm) = normalize_folder(claimed) else {
                continue;
            };
            let better = match &best {
                None => true,
                Some((n, id, folder)) => {
                    length > *n || (length == *n && (org.id.as_str(), &norm) < (*id, folder))
                }
            };
            if better {
                best = Some((length, org.id.as_str(), norm));
            }
        }
    }
    best.map_or(personal, |(_, id, folder)| FolderMatch {
        organization: id.to_owned(),
        folder: Some(folder),
    })
}
