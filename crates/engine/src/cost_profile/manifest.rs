use crate::catalog::Tool;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const FILE: &str = "cost-setup.json";
pub const VERSION: u64 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    File,
    Keys,
    Block,
}

impl Part {
    pub fn as_str(self) -> &'static str {
        match self {
            Part::File => "file",
            Part::Keys => "keys",
            Part::Block => "block",
        }
    }

    pub fn parse(text: &str) -> Option<Part> {
        match text {
            "file" => Some(Part::File),
            "keys" => Some(Part::Keys),
            "block" => Some(Part::Block),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyRec {
    pub path: Vec<String>,
    pub before: Option<String>,
    pub after: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub tool: Tool,
    pub path: PathBuf,
    pub part: Part,
    pub removed: bool,
    pub hash: Option<String>,
    pub backup: Option<PathBuf>,
    pub existed: bool,
    pub keys: Vec<KeyRec>,
    pub at: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    pub entries: Vec<Entry>,
}

impl Manifest {
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join("daemon").join(FILE)
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn put(&mut self, entry: Entry) {
        match self.entries.iter_mut().find(|e| e.id == entry.id) {
            Some(slot) => *slot = entry,
            None => self.entries.push(entry),
        }
    }

    pub fn drop_id(&mut self, id: &str) {
        self.entries.retain(|e| e.id != id);
    }

    pub fn of_tool(&self, tool: Tool) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.tool == tool).collect()
    }

    pub fn load(data_dir: &Path) -> Manifest {
        let Ok(text) = std::fs::read_to_string(Manifest::path(data_dir)) else {
            return Manifest::default();
        };
        serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| Manifest::from_value(&v))
            .unwrap_or_default()
    }

    pub fn save(&self, data_dir: &Path) -> Result<(), String> {
        let path = Manifest::path(data_dir);
        if let Some(dir) = path.parent() {
            notchling_platform::files::ensure_private_dir(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(&self.to_value()).map_err(|e| e.to_string())?;
        notchling_platform::files::write_private_no_follow(&path, text.as_bytes())
            .map_err(|e| format!("the manifest: {e}"))
    }

    pub fn to_value(&self) -> Value {
        json!({
            "version": VERSION,
            "entries": self.entries.iter().map(|e| json!({
                "id": e.id,
                "tool": e.tool.name(),
                "path": e.path.to_string_lossy(),
                "part": e.part.as_str(),
                "removed": e.removed,
                "hash": e.hash,
                "backup": e.backup.as_ref().map(|b| b.to_string_lossy().into_owned()),
                "existed": e.existed,
                "at": e.at,
                "keys": e.keys.iter().map(|k| json!({"path": k.path, "before": k.before, "after": k.after})).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }

    pub fn from_value(v: &Value) -> Option<Manifest> {
        if v.get("version")?.as_u64()? != VERSION {
            return None;
        }
        let entries = v
            .get("entries")?
            .as_array()?
            .iter()
            .filter_map(entry_of)
            .collect();
        Some(Manifest { entries })
    }
}

fn entry_of(v: &Value) -> Option<Entry> {
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    Some(Entry {
        id: text("id")?,
        tool: Tool::parse(&text("tool")?)?,
        path: PathBuf::from(text("path")?),
        part: Part::parse(&text("part")?)?,
        removed: v.get("removed")?.as_bool()?,
        hash: text("hash"),
        backup: text("backup").map(PathBuf::from),
        existed: v.get("existed")?.as_bool()?,
        at: v.get("at").and_then(Value::as_u64).unwrap_or(0),
        keys: v
            .get("keys")
            .and_then(Value::as_array)
            .map(|keys| {
                keys.iter()
                    .filter_map(|k| {
                        Some(KeyRec {
                            path: k
                                .get("path")?
                                .as_array()?
                                .iter()
                                .filter_map(|s| s.as_str().map(str::to_string))
                                .collect(),
                            before: k.get("before").and_then(Value::as_str).map(str::to_string),
                            after: k.get("after")?.as_str()?.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

pub fn hash_of(text: &str) -> String {
    crate::bridge::net::crypto::sha256_hex(text.as_bytes())
}
