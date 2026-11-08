//! The real [`Handle`]: a process on the engine's forkpty [`Pty`] (`PTYSession.swift`), with the last
//! 2 MB of output kept so a client attaching later sees what is already on screen; and
//! [`PtySpawner`], which starts `claude` or any command that way with a scrubbed login environment.

use crate::host::{ExitHandler, Handle, Listener, Spawner};
use crate::system;
use crate::wire::SessionId;
use crate::HostError;
use notchling_engine::terminal::{Pty, PtyCommand, ReadOutcome};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// Bytes kept so a terminal attached later can show what is already on screen.
pub const SCROLLBACK_LIMIT: usize = 2 * 1024 * 1024;

/// The size a host session starts at (`PTYSession.spawn`'s defaults).
const START_COLUMNS: u16 = 200;
const START_ROWS: u16 = 50;

#[derive(Default)]
struct Delivery {
    scrollback: Vec<u8>,
    listeners: HashMap<u64, Listener>,
    next: u64,
}

#[derive(Default)]
struct Exit {
    done: bool,
    handlers: Vec<ExitHandler>,
}

struct Shared {
    /// Held while output is delivered, so a new listener's replay and the live bytes never interleave.
    delivery: Mutex<Delivery>,
    exit: Mutex<Exit>,
}

impl Shared {
    fn finish(&self) {
        let handlers = {
            let mut exit = self.exit.lock().unwrap_or_else(|p| p.into_inner());
            if exit.done {
                return;
            }
            exit.done = true;
            std::mem::take(&mut exit.handlers)
        };
        for handler in handlers {
            handler();
        }
    }
}

pub struct PtyHandle {
    id: SessionId,
    cwd: PathBuf,
    model: Option<String>,
    started_at_ms: i64,
    pid: i32,
    pty: Arc<Pty>,
    shared: Arc<Shared>,
    /// Input goes through a writer thread: a pty that isn't reading never blocks the caller.
    input: Mutex<Sender<Vec<u8>>>,
}

impl PtyHandle {
    /// Starts `executable` on a pty in `cwd` with exactly `environment` (minus `NO_COLOR`, which the
    /// engine's pty never passes on).
    pub fn spawn(
        executable: &Path,
        arguments: &[String],
        cwd: &Path,
        model: Option<&str>,
        environment: &[(String, String)],
    ) -> Result<Arc<PtyHandle>, HostError> {
        let mut command = PtyCommand::new(executable);
        command
            .args(arguments)
            .cwd(cwd)
            .env_clear()
            .size(START_ROWS, START_COLUMNS);
        for (key, value) in environment {
            command.env(key, value);
        }
        let pty = Pty::spawn(&command)
            .map_err(|error| HostError::Refused(format!("Không chạy được claude: {error}")))?;
        let pty = Arc::new(pty);
        let shared = Arc::new(Shared {
            delivery: Mutex::new(Delivery::default()),
            exit: Mutex::new(Exit::default()),
        });

        let reader_pty = pty.clone();
        let reader_shared = shared.clone();
        std::thread::spawn(move || read_loop(&reader_pty, &reader_shared));

        let (tx, rx) = channel::<Vec<u8>>();
        let writer_pty = pty.clone();
        std::thread::spawn(move || {
            for bytes in rx {
                if writer_pty.write_all(&bytes).is_err() {
                    return;
                }
            }
        });

        Ok(Arc::new(PtyHandle {
            id: SessionId::random(),
            cwd: cwd.to_path_buf(),
            model: model.map(String::from),
            started_at_ms: system::now_ms(),
            pid: pty.pid(),
            pty,
            shared,
            input: Mutex::new(tx),
        }))
    }
}

fn read_loop(pty: &Pty, shared: &Shared) {
    let mut buf = vec![0u8; 65536];
    loop {
        match pty.read(&mut buf) {
            Ok(ReadOutcome::Data(n)) => {
                let chunk = &buf[..n];
                let mut delivery = shared.delivery.lock().unwrap_or_else(|p| p.into_inner());
                delivery.scrollback.extend_from_slice(chunk);
                let over = delivery.scrollback.len().saturating_sub(SCROLLBACK_LIMIT);
                if over > 0 {
                    delivery.scrollback.drain(..over);
                }
                let listeners: Vec<Listener> = delivery.listeners.values().cloned().collect();
                for listener in listeners {
                    listener(chunk);
                }
            }
            Ok(ReadOutcome::Timeout) => continue,
            Ok(ReadOutcome::Closed) | Err(_) => break,
        }
    }
    // The child's side closed: reap it, then tell whoever waits.
    let _ = pty.wait();
    shared.finish();
}

impl Handle for PtyHandle {
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
        let done = self
            .shared
            .exit
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .done;
        !done && matches!(self.pty.try_wait(), Ok(None))
    }
    fn on_exit(&self, handler: ExitHandler) {
        let mut exit = self.shared.exit.lock().unwrap_or_else(|p| p.into_inner());
        if exit.done {
            drop(exit);
            handler();
            return;
        }
        exit.handlers.push(handler);
    }
    fn add_listener(&self, listener: Listener) -> u64 {
        let mut delivery = self
            .shared
            .delivery
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        delivery.next += 1;
        let token = delivery.next;
        delivery.listeners.insert(token, listener.clone());
        if !delivery.scrollback.is_empty() {
            listener(&delivery.scrollback);
        }
        token
    }
    fn remove_listener(&self, token: u64) {
        self.shared
            .delivery
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .listeners
            .remove(&token);
    }
    fn write(&self, bytes: &[u8]) {
        let input = self.input.lock().unwrap_or_else(|p| p.into_inner());
        let _ = input.send(bytes.to_vec());
    }
    fn resize(&self, columns: u16, rows: u16) {
        let _ = self.pty.resize(rows, columns);
    }
    /// SIGTERM first; the process gets two seconds to save its session before SIGKILL.
    fn terminate(&self) {
        if !self.is_running() {
            self.shared.finish();
            return;
        }
        let pty = self.pty.clone();
        std::thread::spawn(move || {
            pty.terminate(Duration::from_secs(2));
        });
    }
}

/// The real spawner: `claude` (or any command) on a pty, with the same scrubbed environment as a new
/// session. The login environment is read once, on the first spawn.
pub struct PtySpawner {
    environment: OnceLock<Vec<(String, String)>>,
    home: Option<PathBuf>,
}

impl PtySpawner {
    pub fn new() -> PtySpawner {
        PtySpawner {
            environment: OnceLock::new(),
            home: system::home(),
        }
    }

    /// A spawner with a fixed environment (scrubbed all the same), for tests and tools.
    pub fn with_environment(environment: Vec<(String, String)>) -> PtySpawner {
        let spawner = PtySpawner::new();
        let _ = spawner.environment.set(system::scrubbed(environment));
        spawner
    }

    fn environment(&self) -> &[(String, String)] {
        self.environment
            .get_or_init(|| system::scrubbed(system::login_environment(Duration::from_secs(5))))
    }
}

impl Default for PtySpawner {
    fn default() -> Self {
        PtySpawner::new()
    }
}

impl Spawner for PtySpawner {
    fn spawn(
        &self,
        executable: &Path,
        arguments: &[String],
        cwd: &Path,
        model: Option<&str>,
    ) -> Result<Arc<dyn Handle>, HostError> {
        let handle = PtyHandle::spawn(executable, arguments, cwd, model, self.environment())?;
        Ok(handle)
    }

    fn claude_executable(&self) -> Result<PathBuf, HostError> {
        let home = self.home.as_deref().ok_or_else(|| {
            HostError::Refused("Không tìm thấy lệnh claude (~/.local/bin/claude).".to_string())
        })?;
        system::resolve_claude(home)
    }
}
