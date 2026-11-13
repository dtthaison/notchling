use super::plan::{ExitInfo, TurnSpec};
use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Out,
    Err,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolLine {
    pub stream: Stream,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    Line(ToolLine),
    Exited(ExitInfo),
}

pub trait ToolProcess: Send + Sync {
    fn next(&self) -> Next;
    fn interrupt(&self);
    fn terminate(&self) {
        self.interrupt();
    }
    fn kill(&self) {
        self.terminate();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnError(pub String);

impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "couldn't start it: {}", self.0)
    }
}

impl std::error::Error for SpawnError {}

pub trait ToolSpawner: Send + Sync {
    fn spawn(&self, spec: &TurnSpec) -> Result<Arc<dyn ToolProcess>, SpawnError>;
}

pub const MAX_LINE_BYTES: usize = 1024 * 1024;

const DRAIN_GRACE: Duration = Duration::from_millis(300);

pub fn read_capped_lines(reader: impl Read, cap: usize, mut emit: impl FnMut(String) -> bool) {
    let mut reader = BufReader::with_capacity(64 * 1024, reader);
    let mut line: Vec<u8> = Vec::new();
    loop {
        let chunk = match reader.fill_buf() {
            Ok([]) | Err(_) => break,
            Ok(chunk) => chunk,
        };
        let (take, newline) = match chunk.iter().position(|b| *b == b'\n') {
            Some(at) => (at, true),
            None => (chunk.len(), false),
        };
        let room = cap.saturating_sub(line.len());
        line.extend_from_slice(&chunk[..take.min(room)]);
        reader.consume(take + usize::from(newline));
        if newline {
            let text = String::from_utf8_lossy(&line)
                .trim_end_matches('\r')
                .to_string();
            line.clear();
            if !emit(text) {
                return;
            }
        }
    }
    if !line.is_empty() {
        emit(String::from_utf8_lossy(&line).into_owned());
    }
}

enum Msg {
    Line(ToolLine),
    Exit(ExitInfo),
}

pub struct RealSpawner;

impl ToolSpawner for RealSpawner {
    fn spawn(&self, spec: &TurnSpec) -> Result<Arc<dyn ToolProcess>, SpawnError> {
        let mut command = std::process::Command::new(&spec.program);
        command
            .args(&spec.args)
            .current_dir(&spec.cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        command.env_remove("NO_COLOR");
        for (key, value) in &spec.env {
            match value {
                Some(v) => command.env(key, v),
                None => command.env_remove(key),
            };
        }
        let mut child = command.spawn().map_err(|e| SpawnError(e.to_string()))?;
        let pid = child.id();
        let (tx, rx) = channel();
        let readers = Arc::new(AtomicUsize::new(2));
        if let Some(out) = child.stdout.take() {
            pump(out, Stream::Out, tx.clone(), readers.clone());
        } else {
            readers.fetch_sub(1, Ordering::SeqCst);
        }
        if let Some(err) = child.stderr.take() {
            pump(err, Stream::Err, tx.clone(), readers.clone());
        } else {
            readers.fetch_sub(1, Ordering::SeqCst);
        }
        let child = Arc::new(Mutex::new(child));
        let waiting = child.clone();
        std::thread::spawn(move || wait_for_exit(&waiting, &readers, &tx));
        Ok(Arc::new(RealProcess {
            rx: Mutex::new(rx),
            exit: Mutex::new(None),
            pid,
            child,
        }))
    }
}

fn pump(
    source: impl Read + Send + 'static,
    stream: Stream,
    tx: Sender<Msg>,
    alive: Arc<AtomicUsize>,
) {
    std::thread::spawn(move || {
        read_capped_lines(source, MAX_LINE_BYTES, |text| {
            tx.send(Msg::Line(ToolLine { stream, text })).is_ok()
        });
        alive.fetch_sub(1, Ordering::SeqCst);
    });
}

fn wait_for_exit(child: &Mutex<std::process::Child>, readers: &AtomicUsize, tx: &Sender<Msg>) {
    let status = loop {
        let polled = child.lock().unwrap_or_else(|e| e.into_inner()).try_wait();
        match polled {
            Ok(Some(status)) => break Some(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break None,
        }
    };
    let until = Instant::now() + DRAIN_GRACE;
    while readers.load(Ordering::SeqCst) > 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    let info = status.map_or(
        ExitInfo {
            code: None,
            signaled: false,
        },
        exit_info,
    );
    let _ = tx.send(Msg::Exit(info));
}

#[cfg(unix)]
fn exit_info(status: std::process::ExitStatus) -> ExitInfo {
    use std::os::unix::process::ExitStatusExt;
    ExitInfo {
        code: status.code(),
        signaled: status.signal().is_some(),
    }
}

#[cfg(not(unix))]
fn exit_info(status: std::process::ExitStatus) -> ExitInfo {
    ExitInfo {
        code: status.code(),
        signaled: false,
    }
}

struct RealProcess {
    rx: Mutex<Receiver<Msg>>,
    exit: Mutex<Option<ExitInfo>>,
    pid: u32,
    child: Arc<Mutex<std::process::Child>>,
}

impl ToolProcess for RealProcess {
    fn next(&self) -> Next {
        let mut exit = self.exit.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(info) = *exit {
            return Next::Exited(info);
        }
        let rx = self.rx.lock().unwrap_or_else(|e| e.into_inner());
        let info = match rx.recv() {
            Ok(Msg::Line(line)) => return Next::Line(line),
            Ok(Msg::Exit(info)) => info,
            Err(_) => ExitInfo {
                code: None,
                signaled: false,
            },
        };
        *exit = Some(info);
        Next::Exited(info)
    }

    fn interrupt(&self) {
        #[cfg(unix)]
        self.signal(libc::SIGINT);
        #[cfg(not(unix))]
        self.signal(0);
    }

    fn terminate(&self) {
        #[cfg(unix)]
        self.signal(libc::SIGTERM);
        #[cfg(not(unix))]
        self.signal(0);
    }

    fn kill(&self) {
        #[cfg(unix)]
        self.signal(libc::SIGKILL);
        #[cfg(not(unix))]
        self.signal(0);
    }
}

impl RealProcess {
    fn signal(&self, signal: i32) {
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        #[cfg(unix)]
        {
            if let Ok(pid) = i32::try_from(self.pid) {
                // SAFETY: `kill` takes a pid and a signal number and touches no memory; the child
                // has not been reaped (checked above, under the lock that reaps it).
                unsafe {
                    libc::kill(pid, signal);
                }
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (self.pid, signal);
            let _ = child.kill();
        }
    }
}

#[cfg(all(test, unix))]
#[path = "process_tests.rs"]
mod tests;
