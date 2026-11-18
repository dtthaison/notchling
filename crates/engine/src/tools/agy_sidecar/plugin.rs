use super::{Roots, PLUGIN, SIDECAR_FOLDER};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const MARK: &str = ".notchling-plugin";
pub const MARK_TEXT: &str =
    "Written by Notchling. Turn it off in Notchling > Settings > Agents. If you edit these files, Notchling stops changing them.\n";

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

fn put(path: &Path, value: &Value) -> Result<bool, String> {
    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
    );
    put_text(path, &text)
}

fn put_text(path: &Path, text: &str) -> Result<bool, String> {
    if is_link(path) {
        return Err(format!("{} is a link; it was left alone", path.display()));
    }
    if std::fs::read_to_string(path).is_ok_and(|now| now == text) {
        return Ok(false);
    }
    notchling_platform::files::write_private_no_follow(path, text.as_bytes())
        .map_err(|e| format!("could not write {}: {e}", path.display()))?;
    Ok(true)
}

pub fn install(roots: &Roots, binary: &Path) -> Result<bool, String> {
    let sidecar = sidecar_json(binary)?;
    match state(roots) {
        Plugin::Foreign => {
            return Err(
                "a plugin named notchling is already in Antigravity's plugins folder and isn't Notchling's; it was left alone"
                    .into(),
            )
        }
        Plugin::Absent | Plugin::Ours => {}
    }
    if !roots.config.is_dir() {
        return Err("Antigravity's config folder is not there yet".into());
    }
    let dir = roots.plugin_dir();
    let leaf = sidecar_dir(roots);
    for folder in [&dir, &leaf] {
        if is_link(folder) {
            return Err(format!("{} is a link; it was left alone", folder.display()));
        }
        std::fs::create_dir_all(folder)
            .map_err(|e| format!("could not make {}: {e}", folder.display()))?;
    }
    let mut changed = put_text(&dir.join(MARK), MARK_TEXT)?;
    changed |= put(&dir.join("plugin.json"), &manifest())?;
    changed |= put(&leaf.join("sidecar.json"), &sidecar)?;
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
