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

pub fn shaped(tool: Tool, id: &str, home: &Path, path: &Path, transcript: bool) -> bool {
    let Some(parts) = parts_below(home, path) else {
        return false;
    };
    match tool {
        Tool::Claude => {
            let rest = match parts.as_slice() {
                [".claude", rest @ ..] => rest,
                [".claude-lanes", lane, rest @ ..] if !lane.is_empty() => rest,
                _ => return false,
            };
            match (transcript, rest) {
                (true, ["projects", slug, name]) => {
                    !slug.is_empty() && name.strip_suffix(".jsonl").is_some_and(|s| !s.is_empty())
                }
                (false, ["projects", slug, dir]) => !slug.is_empty() && *dir == id,
                (false, ["file-history", dir]) => *dir == id,
                (false, ["todos", name]) => {
                    name.starts_with(&format!("{id}-")) && name.ends_with(".json")
                }
                _ => false,
            }
        }
        Tool::Codex => {
            let uuid = id.strip_prefix("codex:").unwrap_or(id);
            match (transcript, parts.as_slice()) {
                (true, [".codex", "sessions", between @ .., name]) => {
                    between.iter().all(|part| !part.is_empty())
                        && name.starts_with("rollout-")
                        && name.ends_with(".jsonl")
                        && !uuid.is_empty()
                        && name.contains(uuid)
                }
                (false, [".codex", "session_index.jsonl"]) => true,
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
