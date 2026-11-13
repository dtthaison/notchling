use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde_json::{json, Map, Value};

use super::{LiveModel, Tool};

const MAX_AGE_SECS: f64 = 86_400.0;

static WRITING: Mutex<()> = Mutex::new(());
static TEMPS: AtomicU64 = AtomicU64::new(0);

pub struct LiveCache {
    path: PathBuf,
}

impl LiveCache {
    pub fn new(path: PathBuf) -> LiveCache {
        LiveCache { path }
    }

    pub fn get(&self, tool: Tool) -> Option<(f64, Vec<LiveModel>)> {
        let entry = self.read().remove(tool.name())?;
        let at = entry.get("at")?.as_f64()?;
        let models = entry
            .get("models")?
            .as_array()?
            .iter()
            .filter_map(|m| serde_json::from_value(m.clone()).ok())
            .collect();
        Some((at, models))
    }

    pub fn put(&self, tool: Tool, at: f64, models: &[LiveModel]) {
        let _guard = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        let mut all = self.read();
        all.insert(tool.name().to_string(), json!({"at": at, "models": models}));
        let _ = self.write(&Value::Object(all));
    }

    fn read(&self) -> Map<String, Value> {
        match std::fs::read(&self.path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
        {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        }
    }

    fn write(&self, value: &Value) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temp = self.path.with_extension(format!(
            "tmp-{}-{}",
            std::process::id(),
            TEMPS.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
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

pub fn refresh_due(at: Option<f64>, now: f64) -> bool {
    match at {
        None => true,
        Some(at) => now < at || now - at > MAX_AGE_SECS,
    }
}
