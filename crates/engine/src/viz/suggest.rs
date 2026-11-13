use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VizTab {
    Phone,
    Web,
    Flow,
    Diagram,
    Entity,
    Chart,
}

impl VizTab {
    pub fn as_str(self) -> &'static str {
        match self {
            VizTab::Phone => "phone",
            VizTab::Web => "web",
            VizTab::Flow => "flow",
            VizTab::Diagram => "diagram",
            VizTab::Entity => "entity",
            VizTab::Chart => "chart",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VizSignals {
    pub has_recent_mermaid: bool,
    pub has_localhost_url: bool,
}

pub fn suggest(cwd: &str, signals: &VizSignals) -> VizTab {
    if signals.has_recent_mermaid {
        return VizTab::Diagram;
    }
    if cwd.is_empty() {
        return if signals.has_localhost_url {
            VizTab::Web
        } else {
            VizTab::Flow
        };
    }
    let root = Path::new(cwd);
    let entries: Vec<String> = std::fs::read_dir(root)
        .map(|dir| {
            dir.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let has = |name: &str| entries.iter().any(|e| e == name);
    let has_suffix = |suffix: &str| entries.iter().any(|e| e.ends_with(suffix));

    if has("pubspec.yaml")
        || has("android")
        || has_suffix(".xcodeproj")
        || has_suffix(".xcworkspace")
    {
        return VizTab::Phone;
    }
    if has_web_framework(&root.join("package.json")) || signals.has_localhost_url {
        return VizTab::Web;
    }
    if has("prisma")
        || root.join("prisma/schema.prisma").exists()
        || has_suffix(".sql")
        || contains_drift_tables(root)
    {
        return VizTab::Entity;
    }
    VizTab::Flow
}

fn has_web_framework(package_json: &Path) -> bool {
    let Some(object) = std::fs::read(package_json)
        .ok()
        .and_then(|data| serde_json::from_slice::<Value>(&data).ok())
    else {
        return false;
    };
    ["dependencies", "devDependencies"].iter().any(|key| {
        object.get(key).and_then(Value::as_object).is_some_and(|d| {
            ["next", "vite", "react"]
                .iter()
                .any(|name| d.contains_key(*name))
        })
    })
}

fn contains_drift_tables(root: &Path) -> bool {
    let mut visited = 0;
    drift_in(&root.join("lib"), &mut visited)
}

fn drift_in(folder: &Path, visited: &mut usize) -> bool {
    let Ok(read) = std::fs::read_dir(folder) else {
        return false;
    };
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        *visited += 1;
        if *visited > 400 {
            return false;
        }
        if name.ends_with("tables.dart") || name == "tables" || name.ends_with(".drift") {
            return true;
        }
        let is_folder = entry.file_type().is_ok_and(|t| t.is_dir());
        if is_folder && drift_in(&entry.path(), visited) {
            return true;
        }
        if *visited > 400 {
            return false;
        }
    }
    false
}

pub fn git_branch(cwd: &str) -> Option<String> {
    let mut directory = Path::new(cwd).to_path_buf();
    for _ in 0..6 {
        let git = directory.join(".git");
        if let Ok(text) = std::fs::read_to_string(git.join("HEAD")) {
            let line = text.trim();
            return Some(match line.strip_prefix("ref: refs/heads/") {
                Some(branch) => branch.to_string(),
                None => line.chars().take(7).collect(),
            });
        }
        if git.exists() {
            return None;
        }
        if !directory.pop() {
            break;
        }
    }
    None
}
