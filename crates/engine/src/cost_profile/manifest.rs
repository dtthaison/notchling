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
    pub existing: Option<usize>,
    pub kept: bool,
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
    pub pending: bool,
    pub before_hash: Option<String>,
    pub released: bool,
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

    pub fn load(data_dir: &Path) -> Result<Manifest, String> {
        let path = Manifest::path(data_dir);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Manifest::default()),
            Err(e) => return Err(format!("the setup record can't be read: {e}")),
        };
        let unreadable = || {
            format!(
                "the setup record {} is damaged; nothing was changed. Move it aside to start over.",
                path.display()
            )
        };
        let value: Value = serde_json::from_str(&text).map_err(|_| unreadable())?;
        Manifest::from_value(&value).ok_or_else(unreadable)
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
                "pending": e.pending,
                "beforeHash": e.before_hash,
                "released": e.released,
                "keys": e.keys.iter().map(|k| json!({"path": k.path, "before": k.before, "after": k.after, "existing": k.existing, "kept": k.kept})).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })
    }

    pub fn from_value(v: &Value) -> Option<Manifest> {
        if v.get("version")?.as_u64()? != VERSION {
            return None;
        }
        let entries: Vec<Entry> = v
            .get("entries")?
            .as_array()?
            .iter()
            .map(entry_of)
            .collect::<Option<_>>()?;
        let mut ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        (ids.len() == entries.len()).then_some(Manifest { entries })
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
        pending: v.get("pending").and_then(Value::as_bool).unwrap_or(false),
        before_hash: text("beforeHash"),
        released: v.get("released").and_then(Value::as_bool).unwrap_or(false),
        keys: v
            .get("keys")
            .and_then(Value::as_array)
            .map(|keys| keys.iter().map(key_of).collect::<Option<Vec<_>>>())
            .unwrap_or(Some(Vec::new()))?,
    })
}

fn key_of(k: &Value) -> Option<KeyRec> {
    Some(KeyRec {
        path: k
            .get("path")?
            .as_array()?
            .iter()
            .map(|s| s.as_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()?,
        before: k.get("before").and_then(Value::as_str).map(str::to_string),
        after: k.get("after")?.as_str()?.to_string(),
        existing: k
            .get("existing")
            .and_then(Value::as_u64)
            .map(|n| n as usize),
        kept: k.get("kept").and_then(Value::as_bool).unwrap_or(false),
    })
}

pub fn hash_of(text: &str) -> String {
    crate::bridge::net::crypto::sha256_hex(text.as_bytes())
}
