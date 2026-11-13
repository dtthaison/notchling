#![allow(dead_code)]

use notchling_ptyhost::host::{ExitHandler, Handle, HostOptions, Listener, SessionHost, Spawner};
use notchling_ptyhost::state::StateStore;
use notchling_ptyhost::wire::SessionId;
use notchling_ptyhost::HostError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Scratch {
    pub dir: PathBuf,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

impl Scratch {
    pub fn new() -> Scratch {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = PathBuf::from(format!("/tmp/ph-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch { dir }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[derive(Default)]
struct FakeInner {
    running: bool,
    exit_handlers: Vec<ExitHandler>,
    listeners: HashMap<u64, Listener>,
    next: u64,
    screen: Vec<u8>,
    written: Vec<Vec<u8>>,
    terminated: bool,
    resizes: Vec<(u16, u16)>,
}

pub struct FakeHandle {
    id: SessionId,
    cwd: PathBuf,
    model: Option<String>,
    pid: i32,
    started_at_ms: i64,
    inner: Mutex<FakeInner>,
}

impl FakeHandle {
    fn new(cwd: &Path, model: Option<&str>, pid: i32) -> FakeHandle {
        FakeHandle {
            id: SessionId::random(),
            cwd: cwd.to_path_buf(),
            model: model.map(String::from),
            pid,
            started_at_ms: now_ms(),
            inner: Mutex::new(FakeInner {
                running: true,
                ..FakeInner::default()
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FakeInner> {
        self.inner.lock().expect("fake lock")
    }

    pub fn emit(&self, text: &str) {
        let all: Vec<Listener> = {
            let mut inner = self.lock();
            inner.screen.extend_from_slice(text.as_bytes());
            inner.listeners.values().cloned().collect()
        };
        for listener in all {
            listener(text.as_bytes());
        }
    }

    pub fn finish(&self) {
        let handlers = {
            let mut inner = self.lock();
            inner.running = false;
            std::mem::take(&mut inner.exit_handlers)
        };
        for handler in handlers {
            handler();
        }
    }

    pub fn written(&self) -> Vec<Vec<u8>> {
        self.lock().written.clone()
    }
    pub fn terminated(&self) -> bool {
        self.lock().terminated
    }
    pub fn resizes(&self) -> Vec<(u16, u16)> {
        self.lock().resizes.clone()
    }
}

impl Handle for FakeHandle {
    fn id(&self) -> SessionId {
        self.id
    }
    fn cwd(&self) -> &Path {
        &self.cwd
    }
    fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }
    fn started_at_ms(&self) -> i64 {
        self.started_at_ms
    }
    fn root_pid(&self) -> i32 {
        self.pid
    }
    fn is_running(&self) -> bool {
        self.lock().running
    }
    fn on_exit(&self, handler: ExitHandler) {
        let mut inner = self.lock();
        if !inner.running {
            drop(inner);
            handler();
            return;
        }
        inner.exit_handlers.push(handler);
    }
    fn add_listener(&self, listener: Listener) -> u64 {
        let (token, replay) = {
            let mut inner = self.lock();
            inner.next += 1;
            let token = inner.next;
            inner.listeners.insert(token, listener.clone());
            (token, inner.screen.clone())
        };
        if !replay.is_empty() {
            listener(&replay);
        }
        token
    }
    fn remove_listener(&self, token: u64) {
        self.lock().listeners.remove(&token);
    }
    fn write(&self, bytes: &[u8]) {
        self.lock().written.push(bytes.to_vec());
    }
    fn resize(&self, columns: u16, rows: u16) {
        self.lock().resizes.push((columns, rows));
    }
    fn terminate(&self) {
        self.lock().terminated = true;
        self.finish();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spawn {
    pub executable: String,
    pub arguments: Vec<String>,
    pub cwd: String,
    pub model: Option<String>,
}

#[derive(Default)]
pub struct FakeSpawner {
    spawns: Mutex<Vec<Spawn>>,
    handles: Mutex<Vec<Arc<FakeHandle>>>,
    next_pid: Mutex<i32>,
}

impl FakeSpawner {
    pub fn new() -> Arc<FakeSpawner> {
        Arc::new(FakeSpawner {
            next_pid: Mutex::new(5000),
            ..FakeSpawner::default()
        })
    }
    pub fn spawns(&self) -> Vec<Spawn> {
        self.spawns.lock().expect("lock").clone()
    }
    pub fn handle(&self, index: usize) -> Arc<FakeHandle> {
        self.handles.lock().expect("lock")[index].clone()
    }
}

impl Spawner for FakeSpawner {
    fn claude_executable(&self) -> Result<PathBuf, HostError> {
        Ok(PathBuf::from("/fake/claude"))
    }
    fn spawn(
        &self,
        executable: &Path,
        arguments: &[String],
        cwd: &Path,
        model: Option<&str>,
    ) -> Result<Arc<dyn Handle>, HostError> {
        self.spawns.lock().expect("lock").push(Spawn {
            executable: executable.to_string_lossy().into_owned(),
            arguments: arguments.to_vec(),
            cwd: cwd.to_string_lossy().into_owned(),
            model: model.map(String::from),
        });
        let pid = {
            let mut next = self.next_pid.lock().expect("lock");
            *next += 1;
            *next
        };
        let handle = Arc::new(FakeHandle::new(cwd, model, pid));
        self.handles.lock().expect("lock").push(handle.clone());
        Ok(handle)
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn make_host(
    scratch: &Scratch,
    spawner: Arc<FakeSpawner>,
    is_alive: fn(i32) -> bool,
    discover: fn(i32) -> Option<String>,
) -> (Arc<SessionHost>, Arc<StateStore>) {
    let state = Arc::new(StateStore::new(scratch.path("state.json")));
    let options = HostOptions {
        is_alive: Arc::new(is_alive),
        discover_session_id: Arc::new(discover),
        discovery_interval: Duration::from_millis(20),
        discovery_attempts: 50,
    };
    let host = SessionHost::new(spawner, state.clone(), options);
    (host, state)
}

pub fn never_alive(_: i32) -> bool {
    false
}

pub fn nothing_found(_: i32) -> Option<String> {
    None
}

pub fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !condition() {
        assert!(Instant::now() < deadline, "condition not met in 3 s");
        std::thread::sleep(Duration::from_millis(10));
    }
}
