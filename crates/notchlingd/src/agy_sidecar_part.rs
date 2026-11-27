use super::AgySidecarOps;
use notchling_engine::cost_profile::diff::counts;
use notchling_engine::install::text_hash;
use notchling_engine::tools::agy_sidecar::config_file::enabled_text;
use notchling_engine::tools::agy_sidecar::plugin::{sidecar_dir, MARK};
use notchling_engine::tools::agy_sidecar::SIDECAR_ID;
use serde_json::{json, Value};
use std::path::PathBuf;

pub const PART: &str = "sidecar";
pub const HOOKS_PART: &str = "sidecar.hooks";
pub const MARK_PART: &str = "sidecar.plugin_mark";
pub const MANIFEST_PART: &str = "sidecar.plugin_json";
pub const SIDECAR_JSON_PART: &str = "sidecar.sidecar_json";

pub enum Config {
    Part(Value),
    Already,
    Blocked(String),
}

pub trait Sidecar: Send + Sync {
    fn config_part(&self) -> Config;
}

impl Sidecar for AgySidecarOps {
    fn config_part(&self) -> Config {
        AgySidecarOps::config_part(self)
    }
}

impl AgySidecarOps {
    pub(crate) fn config_part(&self) -> Config {
        let path = self.roots.config_json();
        let Ok(before) = std::fs::read_to_string(&path) else {
            return Config::Blocked("Antigravity has no config.json yet.".to_string());
        };
        match enabled_text(&before, SIDECAR_ID) {
            Err(e) => Config::Blocked(e),
            Ok(None) => Config::Already,
            Ok(Some(after)) => {
                let (added, removed) = counts(&before, &after);
                Config::Part(json!({
                    "id": PART,
                    "path": path.to_string_lossy(),
                    "tool": "antigravity",
                    "change": "update",
                    "added": added,
                    "removed": removed,
                    "before": text_hash(&before),
                    "after": text_hash(&after),
                    "notes": ["turn on the sidecar", "add the sidecar hook to hooks.json"],
                    "also": self
                        .other_files()
                        .into_iter()
                        .map(|(id, path)| {
                            json!({
                                "id": id,
                                "path": path.to_string_lossy(),
                                "before": file_hash(&path),
                            })
                        })
                        .collect::<Vec<_>>(),
                }))
            }
        }
    }

    fn other_files(&self) -> Vec<(&'static str, PathBuf)> {
        let dir = self.roots.plugin_dir();
        vec![
            (HOOKS_PART, self.roots.hooks_json()),
            (MARK_PART, dir.join(MARK)),
            (MANIFEST_PART, dir.join("plugin.json")),
            (
                SIDECAR_JSON_PART,
                sidecar_dir(&self.roots).join("sidecar.json"),
            ),
        ]
    }

    pub(super) fn files_are(&self, args: &Value) -> bool {
        let Some(given) = args.get("parts").filter(|p| !p.is_null()) else {
            return true;
        };
        let Some(given) = given.as_object() else {
            return false;
        };
        let Some(config) = given.get(PART).and_then(Value::as_str) else {
            return false;
        };
        if !std::fs::read_to_string(self.roots.config_json())
            .is_ok_and(|text| text_hash(&text) == config)
        {
            return false;
        }
        self.other_files().iter().all(|(id, path)| {
            let asked = match given.get(*id) {
                Some(Value::Null) => None,
                Some(Value::String(hash)) => Some(hash.as_str()),
                _ => return false,
            };
            asked == file_hash(path).as_deref()
        })
    }
}

fn file_hash(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text_hash(&text))
}
