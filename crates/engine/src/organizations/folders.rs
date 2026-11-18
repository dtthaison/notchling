use super::record::{normalize_folder, Organization, PERSONAL_ID};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderMatch {
    pub organization: String,
    pub folder: Option<String>,
}

pub fn standardize(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut out = PathBuf::from("/");
    for part in path.components() {
        match part {
            Component::Normal(p) => out.push(p),
            Component::ParentDir => {
                out.pop();
            }
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
    }
    Some(out)
}

pub fn expand(folder: &str, home: &Path) -> Option<PathBuf> {
    let norm = normalize_folder(folder)?;
    let path = if norm == "~" {
        home.to_path_buf()
    } else if let Some(rest) = norm.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(norm)
    };
    standardize(&path)
}

fn depth(path: &Path) -> usize {
    path.components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count()
}

pub fn real(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

pub fn find(
    organizations: &[Organization],
    folder: &Path,
    home: &Path,
    resolve: &dyn Fn(&Path) -> Option<PathBuf>,
) -> FolderMatch {
    let personal = FolderMatch {
        organization: PERSONAL_ID.into(),
        folder: None,
    };
    let Some(session) = standardize(folder) else {
        return personal;
    };
    let session_real = resolve(&session);
    let mut best: Option<(usize, &str, String)> = None;
    for org in organizations {
        for claimed in &org.folders {
            let Some(path) = expand(claimed, home) else {
                continue;
            };
            let real = resolve(&path);
            let textual = session.starts_with(&path);
            let canonical = match (&real, &session_real) {
                (Some(a), Some(b)) => b.starts_with(a),
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
