use crate::cluster::id::Tool;
use crate::cluster::main_sessions::MainSession;
use crate::transcript::continued::earlier;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResumeSet {
    pub session: String,
    pub tool: Tool,
    pub transcripts: Vec<PathBuf>,
    pub files: Vec<PathBuf>,
    pub dirs: Vec<PathBuf>,
}

fn claude(m: &MainSession, home: &Path) -> ResumeSet {
    let config = m
        .config_dir
        .as_deref()
        .map_or_else(|| home.join(".claude"), PathBuf::from);
    let transcript = m.transcript.as_deref().map(Path::new);
    let id = transcript
        .and_then(Path::file_stem)
        .and_then(|stem| stem.to_str())
        .unwrap_or(&m.id)
        .to_string();
    let mut transcripts = Vec::new();
    let mut dirs = Vec::new();
    if let (Some(path), Some(text)) = (transcript, m.transcript.as_deref()) {
        transcripts.extend(earlier(text).into_iter().map(PathBuf::from));
        transcripts.push(path.to_path_buf());
        if let Some(folder) = path.parent() {
            dirs.push(folder.join(&id));
        }
    }
    dirs.push(config.join("file-history").join(&id));
    ResumeSet {
        session: m.id.clone(),
        tool: Tool::Claude,
        transcripts,
        files: todos(&config.join("todos"), &id),
        dirs,
    }
}

fn todos(folder: &Path, id: &str) -> Vec<PathBuf> {
    let prefix = format!("{id}-");
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|item| item.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|item| {
            let name = item.file_name();
            let name = name.to_string_lossy();
            name.starts_with(&prefix) && name.ends_with(".json")
        })
        .map(|item| item.path())
        .collect();
    found.sort();
    found
}

fn codex(m: &MainSession, home: &Path) -> ResumeSet {
    let mut set = ResumeSet {
        session: m.id.clone(),
        tool: Tool::Codex,
        transcripts: Vec::new(),
        files: Vec::new(),
        dirs: Vec::new(),
    };
    let Some(rollout) = m.transcript.as_deref().map(Path::new) else {
        return set;
    };
    let codex_home = rollout
        .ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == "sessions"))
        .and_then(Path::parent)
        .map_or_else(|| home.join(".codex"), Path::to_path_buf);
    set.transcripts.push(rollout.to_path_buf());
    set.files.push(codex_home.join("session_index.jsonl"));
    set
}

fn parts_below<'a>(home: &Path, path: &'a Path) -> Option<Vec<&'a str>> {
    let rel = path.strip_prefix(home).ok()?;
    rel.components()
        .map(|part| match part {
            std::path::Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Transcript,
    File,
    Dir,
}

pub fn uuid_like(text: &str) -> bool {
    (32..=64).contains(&text.len()) && text.bytes().all(|b| b == b'-' || b.is_ascii_hexdigit())
}

fn claude_config<'a, 'b>(parts: &'b [&'a str]) -> Option<(&'b [&'a str], &'b [&'a str])> {
    match parts {
        [".claude", ..] => Some(parts.split_at(1)),
        [".claude-lanes", lane, ..] if !lane.is_empty() => Some(parts.split_at(2)),
        _ => None,
    }
}

fn stem_of(name: &str) -> Option<&str> {
    name.strip_suffix(".jsonl").filter(|stem| uuid_like(stem))
}

fn claude_shaped(id: &str, own: &[PathBuf], home: &Path, parts: &[&str], shape: Shape) -> bool {
    let Some(main) = own.last().and_then(|main| parts_below(home, main)) else {
        return false;
    };
    let Some((main_config, ["projects", slug, name])) = claude_config(&main) else {
        return false;
    };
    if stem_of(name).is_none() {
        return false;
    }
    let Some((config, rest)) = claude_config(parts) else {
        return false;
    };
    if config != main_config {
        return false;
    }
    let mut ids: Vec<&str> = own
        .iter()
        .filter_map(|path| path.file_stem().and_then(|stem| stem.to_str()))
        .collect();
    ids.push(id);
    ids.retain(|candidate| uuid_like(candidate));
    match (shape, rest) {
        (Shape::Transcript, ["projects", folder, name]) => {
            folder == slug && stem_of(name).is_some()
        }
        (Shape::Dir, ["projects", folder, dir]) => folder == slug && ids.contains(dir),
        (Shape::Dir, ["file-history", dir]) => ids.contains(dir),
        (Shape::File, ["todos", name]) => ids.iter().any(|each| {
            name.strip_prefix(*each)
                .and_then(|tail| tail.strip_prefix('-'))
                .is_some_and(|tail| tail.len() > ".json".len() && tail.ends_with(".json"))
        }),
        _ => false,
    }
}

pub fn shaped(
    tool: Tool,
    id: &str,
    own: &[PathBuf],
    home: &Path,
    path: &Path,
    shape: Shape,
) -> bool {
    let Some(parts) = parts_below(home, path) else {
        return false;
    };
    match tool {
        Tool::Claude => claude_shaped(id, own, home, &parts, shape),
        Tool::Codex => {
            let Some(uuid) = id.strip_prefix("codex:").filter(|uuid| uuid_like(uuid)) else {
                return false;
            };
            match (shape, parts.as_slice()) {
                (Shape::Transcript, [".codex", "sessions", between @ .., name]) => {
                    between.iter().all(|part| !part.is_empty())
                        && name.starts_with("rollout-")
                        && name.ends_with(&format!("{uuid}.jsonl"))
                }
                (Shape::File, [".codex", "session_index.jsonl"]) => true,
                _ => false,
            }
        }
        Tool::Agy | Tool::Shell => false,
    }
}

pub fn resume_set(m: &MainSession, home: &Path) -> ResumeSet {
    match m.tool {
        Tool::Claude => claude(m, home),
        Tool::Codex => codex(m, home),
        Tool::Agy | Tool::Shell => ResumeSet {
            session: m.id.clone(),
            tool: m.tool,
            transcripts: Vec::new(),
            files: Vec::new(),
            dirs: Vec::new(),
        },
    }
}
