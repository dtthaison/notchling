use super::render::{RULE_NAME, VERSION_TAG};
use crate::install::ToolRoots;
use std::fs;
use std::path::{Path, PathBuf};

pub const PSTACK_RULE: &str = "pstack-models.md";
pub const ORCHESTRATOR_FILE: &str = "orchestrator.md";
pub const OLD_RULE_REF: &str = "model-choice.md";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Detected {
    pub claude: bool,
    pub codex: bool,
    pub antigravity: bool,
    pub pstack: bool,
    pub own_rules: Vec<PathBuf>,
    pub orchestrator: Option<PathBuf>,
    pub pointers: Vec<PathBuf>,
}

fn is_dir(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_dir())
}

fn read(path: &Path) -> Option<String> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 1024 * 1024 {
        return None;
    }
    fs::read_to_string(path).ok()
}

pub fn ours(text: &str) -> bool {
    text.contains(VERSION_TAG)
}

fn pstack_installed(claude: &Path) -> bool {
    let cache = claude.join("plugins").join("cache");
    if let Ok(markets) = fs::read_dir(&cache) {
        for market in markets.filter_map(|e| e.ok()) {
            if is_dir(&market.path().join("pstack")) {
                return true;
            }
        }
    }
    read(&claude.join("plugins").join("installed_plugins.json"))
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| {
            v.get("plugins")?
                .as_object()
                .map(|m| m.keys().any(|k| k.starts_with("pstack@")))
        })
        .unwrap_or(false)
}

fn sorted_files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = read
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .filter(|p| fs::symlink_metadata(p).is_ok_and(|m| m.is_file()))
        .collect();
    files.sort();
    files
}

pub fn detect(roots: &ToolRoots) -> Detected {
    let claude = &roots.claude;
    let mut found = Detected {
        claude: is_dir(claude),
        codex: is_dir(&roots.codex),
        antigravity: is_dir(&roots.antigravity),
        pstack: pstack_installed(claude),
        ..Detected::default()
    };
    for file in sorted_files(&claude.join("rules"), "md") {
        let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == RULE_NAME || name == PSTACK_RULE || !name.contains("model") {
            continue;
        }
        if read(&file).is_some_and(|t| !ours(&t)) {
            found.own_rules.push(file);
        }
    }
    let agents = claude.join("agents");
    let orchestrator = agents.join(ORCHESTRATOR_FILE);
    if read(&orchestrator).is_some_and(|t| !ours(&t)) {
        found.orchestrator = Some(orchestrator.clone());
    }
    let mut candidates = sorted_files(&agents, "md");
    candidates.push(claude.join("CLAUDE.md"));
    for file in candidates {
        let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if file == orchestrator || name.starts_with("notchling-") {
            continue;
        }
        if read(&file).is_some_and(|t| t.contains(OLD_RULE_REF) && !ours(&t)) {
            found.pointers.push(file);
        }
    }
    found
}
