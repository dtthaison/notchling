use super::folders::{expand, find_folded, within};
use super::record::{Document, Problem};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Inside,
    Contains,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overlap {
    pub folder: String,
    pub organization: String,
    pub other: String,
    pub relation: Relation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Moved {
    pub folder: String,
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderPreview {
    pub folders: Vec<String>,
    pub problem: Option<Problem>,
    pub overlaps: Vec<Overlap>,
    pub moved: Vec<Moved>,
}

pub struct Lookup<'a> {
    pub home: &'a Path,
    pub resolve: &'a dyn Fn(&Path) -> Option<PathBuf>,
    pub fold: bool,
    pub key: &'a dyn Fn(&str) -> String,
}

pub fn preview(
    doc: &Document,
    id: &str,
    wanted: &[String],
    sessions: &[PathBuf],
    look: &Lookup,
) -> FolderPreview {
    let mut after = doc.clone();
    if let Err(problem) = after.set_folders_with(id, wanted, look.key) {
        return FolderPreview {
            problem: Some(problem),
            ..FolderPreview::default()
        };
    }
    let kept = after
        .find(id)
        .map(|o| o.folders.clone())
        .unwrap_or_default();
    let mut overlaps = Vec::new();
    for folder in &kept {
        let Some(path) = expand(folder, look.home) else {
            continue;
        };
        for org in doc.organizations.iter().filter(|o| o.id != id) {
            for claimed in &org.folders {
                let Some(other) = expand(claimed, look.home) else {
                    continue;
                };
                let relation = if within(&path, &other, look.fold) {
                    Relation::Inside
                } else if within(&other, &path, look.fold) {
                    Relation::Contains
                } else {
                    continue;
                };
                overlaps.push(Overlap {
                    folder: folder.clone(),
                    organization: org.id.clone(),
                    other: claimed.clone(),
                    relation,
                });
            }
        }
    }
    let mut seen: Vec<&PathBuf> = sessions.iter().collect();
    seen.sort();
    seen.dedup();
    let mut moved = Vec::new();
    for session in seen {
        let find = |doc: &Document| {
            find_folded(
                &doc.organizations,
                session,
                look.home,
                look.resolve,
                look.fold,
            )
            .organization
        };
        let (from, to) = (find(doc), find(&after));
        if from != to {
            moved.push(Moved {
                folder: session.to_string_lossy().into_owned(),
                from,
                to,
            });
        }
    }
    FolderPreview {
        folders: kept,
        problem: None,
        overlaps,
        moved,
    }
}
