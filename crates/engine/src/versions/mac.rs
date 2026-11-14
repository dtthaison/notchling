use super::{Identity, IdentityCache, Method, Tool, Version};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MAX_JSON: u64 = 1024 * 1024;
const MAX_ENTRIES: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Roots {
    pub home: PathBuf,
    pub npm_prefixes: Vec<PathBuf>,
    pub brew_prefixes: Vec<PathBuf>,
}

impl Roots {
    pub fn for_home(home: &Path) -> Roots {
        Roots {
            home: home.to_path_buf(),
            npm_prefixes: vec![
                home.join(".npm-global"),
                PathBuf::from("/opt/homebrew"),
                PathBuf::from("/usr/local"),
            ],
            brew_prefixes: vec![PathBuf::from("/opt/homebrew"), PathBuf::from("/usr/local")],
        }
    }

    pub fn inside(root: &Path) -> Roots {
        Roots {
            home: root.to_path_buf(),
            npm_prefixes: vec![
                root.join(".npm-global"),
                root.join("opt/homebrew"),
                root.join("usr/local"),
            ],
            brew_prefixes: vec![root.join("opt/homebrew"), root.join("usr/local")],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Problem {
    TwoInstalls { other: Method },
    CantTell,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    pub tool: Tool,
    pub version: Option<Version>,
    pub method: Method,
    pub path: String,
    pub identity: Option<Identity>,
    pub problems: Vec<Problem>,
}

pub fn detect(roots: &Roots, tool: Tool, cache: &mut IdentityCache) -> Option<Installed> {
    let mut found: Vec<Found> = Vec::new();
    match tool {
        Tool::Claude => {
            found.extend(claude_native(roots));
            found.extend(npm(roots, "@anthropic-ai", "claude-code", "claude"));
            found.extend(cask(roots, "claude-code", "claude"));
        }
        Tool::Codex => {
            found.extend(codex_standalone(roots));
            found.extend(npm(roots, "@openai", "codex", "codex"));
            found.extend(cask(roots, "codex", "codex"));
        }
        Tool::Agy => found.extend(agy(roots)),
        Tool::Notchling => {}
    }
    let mut found = found.into_iter();
    let first = found.next()?;
    let mut problems = Vec::new();
    if let Some(other) = found.next() {
        problems.push(Problem::TwoInstalls {
            other: other.method,
        });
    }
    if first.version.is_none() && first.expected_version {
        problems.push(Problem::CantTell);
    }
    let identity = first
        .identity_path
        .as_deref()
        .and_then(|p| cache.identity(p));
    Some(Installed {
        tool,
        version: first.version,
        method: first.method,
        path: first.path.to_string_lossy().into_owned(),
        identity,
        problems,
    })
}

pub fn watch_paths(roots: &Roots) -> Vec<PathBuf> {
    let mut all = vec![
        roots.home.join(".local/bin"),
        roots.home.join(".local/share/claude/versions"),
        roots.home.join(".codex/packages/standalone"),
    ];
    for prefix in &roots.npm_prefixes {
        all.push(prefix.join("lib/node_modules/@anthropic-ai"));
        all.push(prefix.join("lib/node_modules/@openai"));
    }
    for prefix in &roots.brew_prefixes {
        all.push(prefix.join("Caskroom"));
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for path in all {
        if path.is_dir() && !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

struct Found {
    version: Option<Version>,
    expected_version: bool,
    method: Method,
    path: PathBuf,
    identity_path: Option<PathBuf>,
}

fn link_text(path: &Path) -> Option<String> {
    std::fs::read_link(path)
        .ok()
        .map(|t| t.to_string_lossy().into_owned())
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

fn claude_native(roots: &Roots) -> Option<Found> {
    let launcher = roots.home.join(".local/bin/claude");
    if !exists(&launcher) {
        return None;
    }
    let target = link_text(&launcher);
    let version = target.as_deref().and_then(version_after_versions);
    let path_text = launcher.to_string_lossy().into_owned();
    let method = Method::from_path(Tool::Claude, &path_text, target.as_deref());
    if method != Method::Native {
        return None;
    }
    Some(Found {
        version,
        expected_version: true,
        method,
        path: launcher.clone(),
        identity_path: Some(launcher),
    })
}

fn version_after_versions(target: &str) -> Option<Version> {
    let path = Path::new(target);
    let name = path.file_name()?.to_str()?;
    let parent = path.parent()?.file_name()?.to_str()?;
    if parent == "versions" {
        Version::parse(name)
    } else {
        None
    }
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_JSON {
        return None;
    }
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn bin_entry(package: &serde_json::Value, command: &str) -> Option<String> {
    match package.get("bin")? {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(map) => map
            .get(command)
            .or_else(|| map.values().next())
            .and_then(|v| v.as_str())
            .map(str::to_string),
        _ => None,
    }
}

fn npm(roots: &Roots, scope: &str, package: &str, command: &str) -> Option<Found> {
    for prefix in &roots.npm_prefixes {
        let dir = prefix.join("lib/node_modules").join(scope).join(package);
        let Some(json) = read_json(&dir.join("package.json")) else {
            continue;
        };
        let version = json
            .get("version")
            .and_then(|v| v.as_str())
            .and_then(Version::parse);
        let bin = bin_entry(&json, command)
            .filter(|b| !b.contains("..") && !b.starts_with('/'))
            .map(|b| dir.join(b));
        let path = bin.clone().unwrap_or_else(|| dir.join("package.json"));
        return Some(Found {
            version,
            expected_version: true,
            method: Method::Npm,
            path,
            identity_path: bin,
        });
    }
    None
}

fn sorted_names(dir: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = read
        .filter_map(|e| e.ok())
        .take(MAX_ENTRIES)
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    names
}

fn cask(roots: &Roots, name: &str, command: &str) -> Option<Found> {
    for prefix in &roots.brew_prefixes {
        let caskroom = prefix.join("Caskroom");
        for folder in sorted_names(&caskroom) {
            let matches = folder == name || folder.starts_with(&format!("{name}@"));
            if !matches {
                continue;
            }
            let base = caskroom.join(&folder);
            let newest = sorted_names(&base)
                .into_iter()
                .filter_map(|n| Version::parse(&n).map(|v| (v, n)))
                .max();
            let Some((version, dir_name)) = newest else {
                continue;
            };
            let binary = base.join(&dir_name).join(command);
            let identity_path = binary.is_file().then(|| binary.clone());
            return Some(Found {
                version: Some(version),
                expected_version: true,
                method: Method::Brew,
                path: binary,
                identity_path,
            });
        }
    }
    None
}

fn codex_standalone(roots: &Roots) -> Option<Found> {
    let base = roots.home.join(".codex/packages/standalone");
    let mut folders = vec![base.clone()];
    for first in sorted_names(&base) {
        let dir = base.join(&first);
        if dir.is_dir() {
            for second in sorted_names(&dir) {
                let nested = dir.join(&second);
                if nested.is_dir() {
                    folders.push(nested);
                }
            }
            folders.push(dir);
        }
    }
    let mut best: Option<(Option<Version>, PathBuf)> = None;
    for folder in folders {
        let Some(json) = read_json(&folder.join("codex-package.json")) else {
            continue;
        };
        let version = json
            .get("version")
            .and_then(|v| v.as_str())
            .and_then(Version::parse);
        let better = match &best {
            None => true,
            Some((current, _)) => version > *current,
        };
        if better {
            best = Some((version, folder));
        }
    }
    let (version, folder) = best?;
    let binary = ["codex", "bin/codex"]
        .iter()
        .map(|rel| folder.join(rel))
        .find(|p| p.is_file());
    Some(Found {
        version,
        expected_version: true,
        method: Method::Standalone,
        path: binary.clone().unwrap_or(folder),
        identity_path: binary,
    })
}

fn agy(roots: &Roots) -> Option<Found> {
    let launcher = roots.home.join(".local/bin/agy");
    if !exists(&launcher) {
        return None;
    }
    Some(Found {
        version: None,
        expected_version: false,
        method: Method::Standalone,
        path: launcher.clone(),
        identity_path: Some(launcher),
    })
}
