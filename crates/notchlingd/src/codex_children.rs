use crate::op_group::OpGroup;
use notchling_engine::tools::codex::children::{
    day_folders, refresh, rollout_files, Head, Index, BACKFILL_DAYS,
};
use notchling_local_api::{ops, ApiError, Caller, OpSpec};
use notchling_platform::watch::{Batch, WatchConfig, Watcher};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(200);
const RETRY: Duration = Duration::from_secs(5);
const WATCH: WatchConfig = WatchConfig {
    poll: Duration::from_secs(2),
    debounce: Duration::from_millis(300),
    max_latency: Duration::from_secs(2),
};

pub struct CodexChildren {
    sessions: PathBuf,
    db: PathBuf,
    index: OnceLock<Arc<Index>>,
    stop: AtomicBool,
    thread: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn is_rollout(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.starts_with("rollout-") && name.ends_with(".jsonl")
}

fn head_value(head: &Head) -> Value {
    json!({
        "path": head.path.to_string_lossy(),
        "thread": head.thread,
        "parent": head.parent,
        "depth": head.depth,
        "nickname": head.nickname,
        "role": head.role,
        "started": head.started,
    })
}

impl CodexChildren {
    pub fn new(sessions: PathBuf, db: PathBuf) -> CodexChildren {
        CodexChildren {
            sessions,
            db,
            index: OnceLock::new(),
            stop: AtomicBool::new(false),
            thread: Mutex::new(None),
        }
    }

    pub fn start(self: &Arc<Self>) {
        let mut slot = lock(&self.thread);
        if slot.is_some() {
            return;
        }
        let me = self.clone();
        *slot = Some(std::thread::spawn(move || me.run()));
    }

    pub fn close(&self) {
        self.stop.store(true, Ordering::SeqCst);
        let handle = lock(&self.thread).take();
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }

    pub fn is_child(&self, thread: &str) -> bool {
        self.index.get().is_some_and(|i| i.is_child(thread))
    }

    pub fn children(&self, thread: &str) -> Vec<Head> {
        self.index
            .get()
            .map(|i| i.children(thread))
            .unwrap_or_default()
    }

    fn backfill(&self, index: &Index) {
        for day in day_folders(&self.sessions, now_ms(), BACKFILL_DAYS) {
            if self.stop.load(Ordering::SeqCst) {
                return;
            }
            self.read_day(index, &day);
        }
    }

    fn read_day(&self, index: &Index, day: &Path) {
        let files = rollout_files(day);
        refresh(index, &files);
        index.sweep(day, &files);
    }

    fn apply(&self, index: &Index, batch: &Batch) {
        if batch.rescan {
            self.backfill(index);
            return;
        }
        let days: BTreeSet<PathBuf> = batch
            .paths
            .iter()
            .filter(|p| is_rollout(p))
            .filter_map(|p| p.parent().map(Path::to_path_buf))
            .collect();
        for day in days {
            self.read_day(index, &day);
        }
    }

    fn run(&self) {
        let Ok(index) = Index::open(&self.db) else {
            return;
        };
        let index = Arc::new(index);
        let _ = self.index.set(index.clone());
        self.backfill(&index);
        let mut watcher: Option<(Watcher, _)> = None;
        let mut tried = Instant::now() - RETRY;
        while !self.stop.load(Ordering::SeqCst) {
            if watcher.is_none() && tried.elapsed() >= RETRY && self.sessions.is_dir() {
                tried = Instant::now();
                watcher = Watcher::start(vec![self.sessions.clone()], WATCH).ok();
                if watcher.is_some() {
                    self.backfill(&index);
                }
            }
            match &watcher {
                Some((_, rx)) => {
                    if let Ok(batch) = rx.recv_timeout(TICK) {
                        self.apply(&index, &batch);
                    }
                }
                None => std::thread::sleep(TICK),
            }
        }
    }
}

impl OpGroup for CodexChildren {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        (op == ops::CODEX_CHILDREN).then_some(OpSpec::APP_QUIET)
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::CODEX_CHILDREN => {
                let thread = args.get("thread").and_then(Value::as_str).unwrap_or("");
                let children: Vec<Value> = self.children(thread).iter().map(head_value).collect();
                Ok(json!({ "children": children }))
            }
            other => Err(ApiError::internal(format!("{other} has no handler"))),
        }
    }
}
