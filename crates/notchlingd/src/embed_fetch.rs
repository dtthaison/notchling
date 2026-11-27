use crate::embed_jobs::{After, Jobs, Settle};
use crate::embed_paths;
pub use crate::embed_paths::{TABLE, TABLE_HASH};
use notchling_engine::search::models::{
    fetch_file, file_matches, manifest, Fetch, ModelEntry, Progress, Receiver, CANCELLED,
};
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub const FLAT: &str = "table-f32.safetensors";
pub const TURN_ON: &str = "Turn on Semantic Search first.";
pub const STOPPING: &str = "The download is still stopping. Try again in a moment.";
pub const REMOVING: &str = "The model is being removed. Try again in a moment.";
const ANNOUNCE_EVERY: Duration = Duration::from_secs(1);
const CONVERT_POLL: Duration = Duration::from_millis(50);

pub trait Converter: Send + Sync {
    fn convert(&self, dir: &Path, cancel: &AtomicBool) -> Result<String, String>;
}

pub struct ProcessConvert {
    pub exe: PathBuf,
}

fn drain(pipe: Option<impl Read>) -> String {
    let mut bytes = Vec::new();
    if let Some(mut pipe) = pipe {
        let _ = pipe.read_to_end(&mut bytes);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

impl Converter for ProcessConvert {
    fn convert(&self, dir: &Path, cancel: &AtomicBool) -> Result<String, String> {
        let mut child = std::process::Command::new(&self.exe)
            .arg("convert-static")
            .arg(dir)
            .arg(dir.join(TABLE))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("The helper could not start ({e})."))?;
        let (said, heard) = channel();
        let (out, err) = (child.stdout.take(), child.stderr.take());
        std::thread::spawn(move || {
            let _ = said.send((drain(out), drain(err)));
        });
        let (out, err) = loop {
            match heard.recv_timeout(CONVERT_POLL) {
                Ok(text) => break text,
                Err(RecvTimeoutError::Disconnected) => break Default::default(),
                Err(RecvTimeoutError::Timeout) if cancel.load(Ordering::SeqCst) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CANCELLED.into());
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        };
        let ended = child
            .wait()
            .map_err(|e| format!("The helper could not prepare the model ({e})."))?;
        if !ended.success() {
            let line = err.lines().next().unwrap_or("it stopped");
            return Err(format!("The helper could not prepare the model ({line})."));
        }
        out.split_whitespace()
            .next()
            .map(str::to_string)
            .ok_or_else(|| "The helper gave no checksum.".to_string())
    }
}

struct Offline;

impl Fetch for Offline {
    fn get(
        &self,
        _: &str,
        _: u64,
        _: &mut dyn Receiver,
    ) -> Result<notchling_engine::search::models::Head, String> {
        Err("Downloads are off in this build.".into())
    }
}

impl Converter for Offline {
    fn convert(&self, _: &Path, _: &AtomicBool) -> Result<String, String> {
        Err("Downloads are off in this build.".into())
    }
}

#[derive(Clone)]
pub struct ModelNet {
    pub fetch: Arc<dyn Fetch>,
    pub convert: Arc<dyn Converter>,
}

impl ModelNet {
    pub fn offline() -> ModelNet {
        ModelNet {
            fetch: Arc::new(Offline),
            convert: Arc::new(Offline),
        }
    }

    pub fn real(helper: PathBuf) -> ModelNet {
        ModelNet {
            fetch: Arc::new(notchling_engine::search::models::net::TlsFetch),
            convert: Arc::new(ProcessConvert { exe: helper }),
        }
    }
}

pub type Announce = Arc<dyn Fn(Value) + Send + Sync>;
pub type Finished = Arc<dyn Fn(&str, bool) + Send + Sync>;

pub struct Fetcher {
    data: PathBuf,
    net: ModelNet,
    jobs: Mutex<Jobs>,
    idle: Condvar,
    results: Mutex<HashMap<String, Value>>,
    announce: Announce,
    finished: Finished,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

pub fn progress_json(
    model: &str,
    state: &str,
    bytes: u64,
    total: u64,
    error: Option<&str>,
) -> Value {
    let mut v = json!({"model": model, "state": state, "bytes": bytes, "total": total});
    if let Some(e) = error {
        v["error"] = json!(e);
    }
    v
}

fn bad_model() -> ApiError {
    ApiError::bad_request("embed.fetch takes {\"model\": a listed model id}")
}

pub fn entry_of(args: &Value, op: &str) -> Result<&'static ModelEntry, ApiError> {
    let id = args
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad_request(format!("{op} takes {{\"model\": string}}")))?;
    manifest().model(id).ok_or_else(bad_model)
}

pub fn off_error() -> ApiError {
    ApiError::new(ErrorCode::Forbidden, TURN_ON).with_reason("semantic_off")
}

impl Fetcher {
    pub fn new(data: PathBuf, net: ModelNet, announce: Announce, finished: Finished) -> Fetcher {
        Fetcher {
            data,
            net,
            jobs: Mutex::new(Jobs::new()),
            idle: Condvar::new(),
            results: Mutex::new(HashMap::new()),
            announce,
            finished,
        }
    }

    fn downloading(&self, entry: &ModelEntry) {
        (self.announce)(progress_json(
            &entry.id,
            "downloading",
            0,
            entry.download_bytes,
            None,
        ));
    }

    pub fn start(self: &Arc<Self>, entry: &'static ModelEntry) -> Result<(), ApiError> {
        let cancel = match lock(&self.jobs).admit(&entry.id, entry.download_bytes) {
            Ok(Some(cancel)) => cancel,
            Ok(None) => return Ok(()),
            Err(msg) => return Err(ApiError::new(ErrorCode::Failed, msg)),
        };
        lock(&self.results).remove(&entry.id);
        self.downloading(entry);
        let me = self.clone();
        let spawned = std::thread::Builder::new()
            .name("embed-fetch".into())
            .spawn(move || me.run(entry, cancel));
        if spawned.is_err() {
            lock(&self.jobs).abort(&entry.id);
            self.idle.notify_all();
            return Err(ApiError::internal("The download could not start."));
        }
        Ok(())
    }

    fn run(&self, entry: &'static ModelEntry, mut cancel: Arc<AtomicBool>) {
        let total = entry.download_bytes;
        loop {
            let outcome = self.download(entry, &cancel);
            let mut jobs = lock(&self.jobs);
            let after = match jobs.settle(&entry.id, total) {
                Settle::Again(next) => {
                    cancel = next;
                    drop(jobs);
                    self.downloading(entry);
                    continue;
                }
                Settle::Done(after) => after,
            };
            if after != After::Report {
                jobs.close(&entry.id);
                drop(jobs);
                self.idle.notify_all();
                return;
            }
            let ok = outcome.is_ok();
            let result = match outcome {
                Ok(()) => progress_json(&entry.id, "ready", total, total, None),
                Err(e) if e == CANCELLED => progress_json(&entry.id, "cancelled", 0, total, None),
                Err(e) => progress_json(&entry.id, "failed", 0, total, Some(&e)),
            };
            lock(&self.results).insert(entry.id.clone(), result.clone());
            drop(jobs);
            (self.announce)(result);
            (self.finished)(&entry.id, ok);
            lock(&self.jobs).close(&entry.id);
            self.idle.notify_all();
            return;
        }
    }

    fn download(&self, entry: &'static ModelEntry, cancel: &Arc<AtomicBool>) -> Result<(), String> {
        let models = embed_paths::models_dir(&self.data);
        let dir = models.join(&entry.folder);
        if is_link(&models) || is_link(&dir) {
            return Err("refusing to write through a link".into());
        }
        let hosts = &manifest().hosts;
        let mut done = 0u64;
        let last = Mutex::new(Instant::now() - ANNOUNCE_EVERY);
        for file in &entry.files {
            if cancel.load(Ordering::SeqCst) {
                return Err(CANCELLED.into());
            }
            if is_link(&models) || is_link(&dir) {
                return Err("refusing to write through a link".into());
            }
            let base = done;
            let report = |p: Progress| {
                let bytes = base + p.bytes;
                lock(&self.jobs).progress(&entry.id, bytes);
                let mut at = lock(&last);
                if at.elapsed() >= ANNOUNCE_EVERY {
                    *at = Instant::now();
                    (self.announce)(progress_json(
                        &entry.id,
                        "downloading",
                        bytes,
                        entry.download_bytes,
                        None,
                    ));
                }
            };
            let target = dir.join(&file.save_as);
            let kept = file_matches(&target, file);
            let converted = file.save_as == FLAT && embed_paths::table_ready(&dir);
            if !kept && !converted {
                fetch_file(self.net.fetch.as_ref(), hosts, file, &dir, cancel, &report)?;
            }
            done += file.size;
        }
        if entry.files.iter().any(|f| f.save_as == FLAT) {
            self.convert(&dir, cancel)?;
        }
        Ok(())
    }

    fn convert(&self, dir: &Path, cancel: &AtomicBool) -> Result<(), String> {
        if is_link(&embed_paths::models_dir(&self.data)) || is_link(dir) {
            return Err("refusing to write through a link".into());
        }
        if !embed_paths::table_ready(dir) {
            if cancel.load(Ordering::SeqCst) {
                return Err(CANCELLED.into());
            }
            let _ = std::fs::remove_file(dir.join(TABLE));
            let _ = std::fs::remove_file(dir.join(TABLE_HASH));
            let hash = self.net.convert.convert(dir, cancel)?;
            notchling_platform::files::write_private_no_follow(
                &dir.join(TABLE_HASH),
                hash.as_bytes(),
            )
            .map_err(|e| format!("The checksum could not be saved ({e})."))?;
            if !embed_paths::table_ready(dir) {
                return Err("The converted table did not match its checksum.".into());
            }
        }
        let _ = std::fs::remove_file(dir.join(FLAT));
        let _ = std::fs::remove_file(dir.join(format!("{FLAT}.part")));
        Ok(())
    }

    pub fn cancel(&self, id: &str) -> bool {
        lock(&self.jobs).cancel(id)
    }

    pub fn cancel_all(&self) {
        lock(&self.jobs).cancel_all();
    }

    pub fn running(&self) -> bool {
        !lock(&self.jobs).is_empty()
    }

    pub fn list(&self) -> Vec<Value> {
        let jobs = lock(&self.jobs);
        jobs.list(&lock(&self.results))
    }

    pub fn wait_idle(&self, wait: Duration) -> bool {
        let jobs = lock(&self.jobs);
        let (jobs, _) = self
            .idle
            .wait_timeout_while(jobs, wait, |jobs| !jobs.is_empty())
            .unwrap_or_else(|e| e.into_inner());
        jobs.is_empty()
    }

    pub fn remove(&self, entry: &ModelEntry, wait: Duration) -> Result<bool, ApiError> {
        let mut jobs = lock(&self.jobs);
        jobs.register_remover(&entry.id);
        let (mut jobs, _) = self
            .idle
            .wait_timeout_while(jobs, wait, |jobs| jobs.waiting(&entry.id))
            .unwrap_or_else(|e| e.into_inner());
        if jobs.waiting(&entry.id) {
            let removed = jobs.leave(&entry.id);
            drop(jobs);
            if removed {
                self.idle.notify_all();
            }
            return Err(ApiError::new(ErrorCode::Failed, STOPPING));
        }
        let models = embed_paths::models_dir(&self.data);
        let deleted = if is_link(&models) {
            false
        } else {
            let folder = models.join(&entry.folder);
            match std::fs::symlink_metadata(&folder) {
                Ok(meta) if meta.file_type().is_symlink() => std::fs::remove_file(&folder).is_ok(),
                Ok(meta) if meta.file_type().is_dir() => std::fs::remove_dir_all(&folder).is_ok(),
                Ok(_) => std::fs::remove_file(&folder).is_ok(),
                Err(_) => false,
            }
        };
        lock(&self.results).remove(&entry.id);
        let removed = jobs.leave(&entry.id);
        drop(jobs);
        if removed {
            self.idle.notify_all();
        }
        Ok(deleted)
    }
}
