//! The last live model list of each tool, kept on disk so an offline start still has it. The
//! picker shows this, else the data file's; a missing or broken file is just no cache.

use std::io::Write;
use std::path::PathBuf;

use serde_json::{json, Map, Value};

use super::{LiveModel, Tool};

/// A list older than this is fetched again.
const MAX_AGE_SECS: f64 = 86_400.0;

pub struct LiveCache {
    path: PathBuf,
}

impl LiveCache {
    pub fn new(path: PathBuf) -> LiveCache {
        LiveCache { path }
    }

    /// When a tool's list was fetched (seconds) and the list.
    pub fn get(&self, tool: Tool) -> Option<(f64, Vec<LiveModel>)> {
        let entry = self.read().remove(tool.name())?;
        let at = entry.get("at")?.as_f64()?;
        let models = serde_json::from_value(entry.get("models")?.clone()).ok()?;
        Some((at, models))
    }

    /// Replaces one tool's list and keeps the others'. Best effort: a cache that can't be written
    /// only means the next start fetches again.
    pub fn put(&self, tool: Tool, at: f64, models: &[LiveModel]) {
        let mut all = self.read();
        all.insert(tool.name().to_string(), json!({"at": at, "models": models}));
        let _ = self.write(&Value::Object(all));
    }

    /// Every tool's entry; nothing readable is an empty map.
    fn read(&self) -> Map<String, Value> {
        match std::fs::read(&self.path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
        {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        }
    }

    /// A temp file beside the cache, then a rename, so a reader never sees half a file.
    fn write(&self, value: &Value) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temp = self
            .path
            .with_extension(format!("tmp-{}", std::process::id()));
        let result = (|| {
            // A leftover from a crash would keep its own mode.
            let _ = std::fs::remove_file(&temp);
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temp)?;
            file.write_all(value.to_string().as_bytes())?;
            file.sync_all()?;
            std::fs::rename(&temp, &self.path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result
    }
}

/// Whether to fetch again: nothing cached, or the list is over a day old (a clock set back counts
/// as old, so a list can't stay fresh forever).
pub fn refresh_due(at: Option<f64>, now: f64) -> bool {
    match at {
        None => true,
        Some(at) => now < at || now - at > MAX_AGE_SECS,
    }
}
