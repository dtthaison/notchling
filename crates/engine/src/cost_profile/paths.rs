use super::detect::{ORCHESTRATOR_FILE, PSTACK_RULE};
use super::manifest::Entry;
use super::plan::{
    ID_BUILDER, ID_ORCHESTRATOR, ID_OWN_RULES, ID_POINTER, ID_PSTACK, ID_READER, ID_REVIEWER,
    ID_RULE, ID_SETTINGS, LEGACY_ID_OWN_RULES,
};
use super::plan_other::{
    ID_AGY_PLUGIN_JSON, ID_AGY_PLUGIN_MARK, ID_AGY_RULE, ID_CODEX_AGENTS, ID_CODEX_CONFIG,
};
use super::render as r;
use super::safe::{guard, is_backup_of};
use crate::catalog::Tool;
use crate::install::ToolRoots;
use crate::tools::agy_sidecar::plugin::MARK;
use crate::tools::agy_sidecar::Roots;
use std::path::{Component, Path, PathBuf};

pub const ID_ORCHESTRATOR_NEW: &str = "claude.orchestrator.new";

fn plain(rel: &str) -> Option<PathBuf> {
    if rel.is_empty() {
        return None;
    }
    let mut out = PathBuf::new();
    for part in Path::new(rel).components() {
        match part {
            Component::Normal(name) => out.push(name),
            _ => return None,
        }
    }
    Some(out)
}

pub fn pointer_id(roots: &ToolRoots, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(&roots.claude).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| format!("{ID_POINTER}{}", parts.join("/")))
}

pub fn target_of(roots: &ToolRoots, id: &str) -> Option<(Tool, PathBuf)> {
    let claude = &roots.claude;
    let rules = claude.join("rules");
    let agents = claude.join("agents");
    let agy = Roots {
        config: roots.antigravity.clone(),
        data: PathBuf::new(),
        apps: Vec::new(),
    }
    .plugin_dir();
    let found = match id {
        ID_RULE => (Tool::Claude, rules.join(r::RULE_NAME)),
        ID_BUILDER => (Tool::Claude, agents.join(format!("{}.md", r::BUILDER))),
        ID_READER => (Tool::Claude, agents.join(format!("{}.md", r::READER))),
        ID_REVIEWER => (Tool::Claude, agents.join(format!("{}.md", r::REVIEWER))),
        ID_ORCHESTRATOR => (Tool::Claude, agents.join(ORCHESTRATOR_FILE)),
        ID_ORCHESTRATOR_NEW => (Tool::Claude, agents.join(format!("{}.md", r::ORCHESTRATOR))),
        ID_SETTINGS => (Tool::Claude, claude.join("settings.json")),
        ID_PSTACK => (Tool::Claude, rules.join(PSTACK_RULE)),
        ID_CODEX_AGENTS => (Tool::Codex, roots.codex.join("AGENTS.md")),
        ID_CODEX_CONFIG => (Tool::Codex, roots.codex.join("config.toml")),
        ID_AGY_RULE => (Tool::Antigravity, agy.join("rules").join(r::RULE_NAME)),
        ID_AGY_PLUGIN_JSON => (Tool::Antigravity, agy.join("plugin.json")),
        ID_AGY_PLUGIN_MARK => (Tool::Antigravity, agy.join(MARK)),
        other => {
            if let Some(name) = other.strip_prefix(ID_OWN_RULES) {
                let one = plain(name).filter(|p| p.components().count() == 1)?;
                (Tool::Claude, rules.join(one))
            } else {
                let rel = other.strip_prefix(ID_POINTER)?;
                (Tool::Claude, claude.join(plain(rel)?))
            }
        }
    };
    Some(found)
}

pub fn validate(roots: &ToolRoots, e: &Entry) -> Result<(), String> {
    let legacy = e.id == LEGACY_ID_OWN_RULES;
    if legacy {
        let rules = roots.claude.join("rules");
        let ok = e.tool == Tool::Claude
            && e.path.parent() == Some(rules.as_path())
            && e.path.extension().is_some_and(|x| x == "md");
        if !ok {
            return Err("not-ours: the recorded path is not a rule file".into());
        }
    } else {
        let Some((tool, path)) = target_of(roots, &e.id) else {
            return Err("not-ours: no such part".into());
        };
        if tool != e.tool || path != e.path {
            return Err("not-ours: the recorded path is not this part's file".into());
        }
    }
    if let Some(b) = &e.backup {
        if !is_backup_of(&e.path, b) {
            return Err("not-ours: the recorded backup is not beside the file".into());
        }
    }
    guard(roots, &e.path).map_err(|why| format!("not-ours: {why}"))
}
