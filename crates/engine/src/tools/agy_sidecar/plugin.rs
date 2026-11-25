use super::{Roots, PLUGIN, SIDECAR_FOLDER};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const MARK: &str = ".notchling-plugin";
pub const MARK_TEXT: &str =
    "Notchling manages these files and rewrites them when it updates; an edited file is copied to a notchling-backup file next to Antigravity's config first. Turn it off in Notchling > Settings.\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plugin {
    Absent,
    Ours,
    Foreign,
}

pub fn sidecar_json(binary: &Path) -> Result<Value, String> {
    let command = binary
        .to_str()
        .filter(|c| binary.is_absolute() && !c.is_empty())
        .ok_or_else(|| "the Notchling program has no plain absolute path".to_string())?;
    Ok(json!({
        "command": command,
        "args": ["agy-sidecar"],
        "restart_policy": "always",
        "display_name": "Notchling",
        "description": "Sends the messages you write in Notchling (Chat, the notch, your phone) to Antigravity conversations.",
    }))
}

pub fn manifest_json() -> Value {
    manifest()
}

fn manifest() -> Value {
    json!({
        "name": PLUGIN,
        "displayName": "Notchling",
        "description": "Notchling's link to Antigravity: a sidecar that delivers your messages.",
    })
}

pub fn sidecar_dir(roots: &Roots) -> PathBuf {
    roots.plugin_dir().join("sidecars").join(SIDECAR_FOLDER)
}

fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

pub fn state(roots: &Roots) -> Plugin {
    let dir = roots.plugin_dir();
    match std::fs::symlink_metadata(&dir) {
        Err(_) => Plugin::Absent,
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() && dir.join(MARK).is_file() => {
            Plugin::Ours
        }
        Ok(_) => Plugin::Foreign,
    }
}

pub fn installed_for(roots: &Roots, binary: &Path) -> bool {
    let Ok(want) = sidecar_json(binary) else {
        return false;
    };
    state(roots) == Plugin::Ours
        && std::fs::read_to_string(sidecar_dir(roots).join("sidecar.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            == Some(want)
}

fn put(roots: &Roots, path: &Path, value: &Value, stamp: u64) -> Result<bool, String> {
    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
    );
    put_text(roots, path, &text, stamp)
}

fn back_up(roots: &Roots, path: &Path, old: &str, stamp: u64) -> Result<(), String> {
    let name = path
        .strip_prefix(roots.plugin_dir())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('/', "-");
    let base = format!("notchling-plugin-{name}.notchling-backup-{stamp}");
    for n in 0..100 {
        let leaf = if n == 0 {
            base.clone()
        } else {
            format!("{base}-{n}")
        };
        let target = roots.config.join(leaf);
        if std::fs::symlink_metadata(&target).is_ok() {
            continue;
        }
        return notchling_platform::files::write_private_no_follow(&target, old.as_bytes())
            .map_err(|e| format!("could not back up {}: {e}", path.display()));
    }
    Err(format!("no free name to back up {}", path.display()))
}

fn put_text(roots: &Roots, path: &Path, text: &str, stamp: u64) -> Result<bool, String> {
    if is_link(path) {
        return Err(format!("{} is a link; it was left alone", path.display()));
    }
    let old = std::fs::read_to_string(path).ok();
    if old.as_deref() == Some(text) {
        return Ok(false);
    }
    if let Some(old) = old.filter(|old| !old.is_empty()) {
        back_up(roots, path, &old, stamp)?;
    }
    notchling_platform::files::write_private_no_follow(path, text.as_bytes())
        .map_err(|e| format!("could not write {}: {e}", path.display()))?;
    Ok(true)
}

pub fn preflight(roots: &Roots, binary: &Path) -> Result<(), String> {
    sidecar_json(binary)?;
    if state(roots) == Plugin::Foreign {
        return Err(
            "a plugin named notchling is already in Antigravity's plugins folder and isn't Notchling's; it was left alone"
                .into(),
        );
    }
    if !roots.config.is_dir() {
        return Err("Antigravity's config folder is not there yet".into());
    }
    let dir = roots.plugin_dir();
    let leaf = sidecar_dir(roots);
    let files = [
        dir.join(MARK),
        dir.join("plugin.json"),
        leaf.join("sidecar.json"),
    ];
    for path in [&dir, &dir.join("sidecars"), &leaf]
        .into_iter()
        .chain(&files)
    {
        if is_link(path) {
            return Err(format!("{} is a link; it was left alone", path.display()));
        }
    }
    for path in &files {
        if path.is_dir() {
            return Err(format!("{} is a folder; it was left alone", path.display()));
        }
    }
    Ok(())
}

pub fn install(roots: &Roots, binary: &Path, stamp: u64) -> Result<bool, String> {
    preflight(roots, binary)?;
    let sidecar = sidecar_json(binary)?;
    let dir = roots.plugin_dir();
    let leaf = sidecar_dir(roots);
    for folder in [&dir, &leaf] {
        std::fs::create_dir_all(folder)
            .map_err(|e| format!("could not make {}: {e}", folder.display()))?;
    }
    let mut changed = put_text(roots, &dir.join(MARK), MARK_TEXT, stamp)?;
    changed |= put(roots, &dir.join("plugin.json"), &manifest(), stamp)?;
    changed |= put(roots, &leaf.join("sidecar.json"), &sidecar, stamp)?;
    Ok(changed)
}

pub fn remove(roots: &Roots) -> Result<bool, String> {
    if state(roots) != Plugin::Ours {
        return Ok(false);
    }
    let dir = roots.plugin_dir();
    let leaf = sidecar_dir(roots);
    let mut changed = false;
    let sidecar = leaf.join("sidecar.json");
    if !is_link(&sidecar) && std::fs::remove_file(&sidecar).is_ok() {
        changed = true;
    }
    let _ = std::fs::remove_dir(&leaf);
    let _ = std::fs::remove_dir(dir.join("sidecars"));
    let only_ours = std::fs::read_dir(&dir).is_ok_and(|entries| {
        entries
            .filter_map(|e| e.ok())
            .all(|e| matches!(e.file_name().to_str(), Some("plugin.json" | MARK)))
    });
    if only_ours {
        for name in ["plugin.json", MARK] {
            let file = dir.join(name);
            if !is_link(&file) && std::fs::remove_file(&file).is_ok() {
                changed = true;
            }
        }
        let _ = std::fs::remove_dir(&dir);
    }
    Ok(changed)
}
