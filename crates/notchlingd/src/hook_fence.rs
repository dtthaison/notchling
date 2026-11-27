use crate::layout::Layout;
use notchling_engine::cluster::fence::{
    after_wait, decide_bytes, Fence, FENCE_FILE, MAX_FENCE_BYTES, POLL_MS, WAIT_MAX_MS,
};
use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

const MAX_INPUT: u64 = 16 * 1024 * 1024;
const STDIN_WAIT: Duration = Duration::from_secs(5);
const TOTAL_BUDGET: Duration = Duration::from_millis(WAIT_MAX_MS + 5_000);
const CLOCK_FAILED: u64 = u64::MAX - 1;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Seen {
    Absent,
    Bytes(Vec<u8>),
    Unreadable,
}

pub fn deny_output(reason: &str) -> String {
    json!({"hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": reason}})
    .to_string()
}

fn session_of(stdin: &[u8]) -> Option<String> {
    let event: Value = serde_json::from_slice(stdin).ok()?;
    let id = event.get("session_id")?.as_str()?;
    (!id.is_empty()).then(|| id.to_string())
}

#[cfg(unix)]
fn open_plain(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_plain(path: &Path) -> std::io::Result<File> {
    if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(std::io::Error::other("a link"));
    }
    OpenOptions::new().read(true).open(path)
}

pub(crate) fn read_fence(path: &Path) -> Seen {
    let file = match open_plain(path) {
        Ok(f) => f,
        Err(e) if e.kind() == ErrorKind::NotFound => return Seen::Absent,
        Err(_) => return Seen::Unreadable,
    };
    match file.metadata() {
        Ok(m) if m.is_file() => {}
        _ => return Seen::Unreadable,
    }
    let mut bytes = Vec::new();
    match file
        .take(MAX_FENCE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
    {
        Ok(_) => Seen::Bytes(bytes),
        Err(_) => Seen::Unreadable,
    }
}

fn look(path: &Path, session: &str, boot_id: &str, now: u64) -> Fence {
    match read_fence(path) {
        Seen::Absent => Fence::Pass,
        Seen::Bytes(b) => decide_bytes(Some(&b), session, boot_id, now),
        Seen::Unreadable => Fence::Wait,
    }
}

pub(crate) fn look_all(path: &Path, sessions: &[&str], boot_id: &str, now: u64) -> Fence {
    let mut wait = false;
    for session in sessions {
        match look(path, session, boot_id, now) {
            Fence::Deny(reason) => return Fence::Deny(reason),
            Fence::Wait => wait = true,
            Fence::Pass => {}
        }
    }
    if wait {
        Fence::Wait
    } else {
        Fence::Pass
    }
}

pub(crate) fn settle(
    path: &Path,
    sessions: &[&str],
    boot_id: &str,
    clock: (&dyn Fn() -> u64, &dyn Fn(u64)),
    max_wait: u64,
) -> Result<(), String> {
    let (now, sleep) = clock;
    let max_wait = max_wait.min(WAIT_MAX_MS);
    let started = now();
    let mut polls = 0u64;
    loop {
        match look_all(path, sessions, boot_id, now()) {
            Fence::Pass => return Ok(()),
            Fence::Deny(reason) => return Err(reason),
            Fence::Wait => {}
        }
        polls += 1;
        let waited = now().saturating_sub(started);
        if waited >= max_wait || polls * POLL_MS > max_wait {
            break;
        }
        sleep(POLL_MS);
    }
    match after_wait() {
        Fence::Deny(reason) => Err(reason),
        _ => Err(String::new()),
    }
}

pub fn answer(
    stdin: &[u8],
    data: &Path,
    boot_id: &str,
    now: &dyn Fn() -> u64,
    sleep: &dyn Fn(u64),
) -> Option<String> {
    answer_within(stdin, data, boot_id, now, sleep, WAIT_MAX_MS)
}

pub(crate) fn answer_within(
    stdin: &[u8],
    data: &Path,
    boot_id: &str,
    now: &dyn Fn() -> u64,
    sleep: &dyn Fn(u64),
    max_wait: u64,
) -> Option<String> {
    let session = session_of(stdin)?;
    let path = data.join(FENCE_FILE);
    match settle(&path, &[&session], boot_id, (now, sleep), max_wait) {
        Ok(()) => None,
        Err(reason) => Some(deny_output(&reason)),
    }
}

fn after_wait_output() -> String {
    match after_wait() {
        Fence::Deny(reason) => deny_output(&reason),
        _ => deny_output(""),
    }
}

pub(crate) fn real_now() -> u64 {
    notchling_platform::clock::boot_ms().unwrap_or(CLOCK_FAILED)
}

pub(crate) fn real_sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

#[cfg(unix)]
fn read_stdin(limit: Duration) -> Option<Vec<u8>> {
    read_fd_within(0, limit, MAX_INPUT)
}

#[cfg(unix)]
pub(crate) fn read_fd_within(fd: i32, limit: Duration, cap: u64) -> Option<Vec<u8>> {
    let deadline = Instant::now() + limit;
    let mut input = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let ms = left.as_millis().min(i32::MAX as u128) as i32;
        let mut p = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut p, 1, ms) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == ErrorKind::Interrupted {
                continue;
            }
            return None;
        }
        if ready == 0 {
            return None;
        }
        let n = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
        if n < 0 {
            if std::io::Error::last_os_error().kind() == ErrorKind::Interrupted {
                continue;
            }
            return None;
        }
        if n == 0 {
            return Some(input);
        }
        input.extend_from_slice(&chunk[..n as usize]);
        if input.len() as u64 >= cap {
            input.truncate(cap as usize);
            return Some(input);
        }
    }
}

#[cfg(not(unix))]
fn read_stdin(limit: Duration) -> Option<Vec<u8>> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("hook-fence-stdin".into())
        .spawn(move || {
            let mut input = Vec::new();
            let ok = std::io::stdin()
                .take(MAX_INPUT)
                .read_to_end(&mut input)
                .is_ok();
            let _ = tx.send(ok.then_some(input));
        })
        .ok()?;
    rx.recv_timeout(limit).ok()?
}

pub(crate) type Spawn = fn(Box<dyn FnOnce() + Send>) -> std::io::Result<()>;
pub(crate) type Work = fn(&[u8], &Path, u64) -> Option<String>;

pub(crate) fn spawn_real(work: Box<dyn FnOnce() + Send>) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("hook-fence".into())
        .spawn(work)
        .map(|_| ())
}

pub(crate) fn work_real(input: &[u8], data: &Path, max_wait: u64) -> Option<String> {
    let boot_id = notchling_platform::clock::boot_id().unwrap_or_default();
    answer_within(input, data, &boot_id, &real_now, &real_sleep, max_wait)
}

pub(crate) fn bounded(
    spawn: Spawn,
    work: Work,
    input: Vec<u8>,
    data: PathBuf,
    limit: Duration,
) -> Option<String> {
    let max_wait = limit.as_millis().min(WAIT_MAX_MS as u128) as u64;
    let (tx, rx) = channel();
    let (sent_input, sent_data) = (input.clone(), data.clone());
    let started = spawn(Box::new(move || {
        let _ = tx.send(work(&sent_input, &sent_data, max_wait));
    }));
    if started.is_err() {
        return work(&input, &data, max_wait);
    }
    match rx.recv_timeout(limit) {
        Ok(out) => out,
        Err(_) => Some(after_wait_output()),
    }
}

pub(crate) fn budgeted(
    total: Duration,
    stdin_max: Duration,
    elapsed: &dyn Fn() -> Duration,
    read: impl FnOnce(Duration) -> Option<Vec<u8>>,
    decide: impl FnOnce(Vec<u8>, Duration) -> Option<String>,
) -> Option<String> {
    let input = read(stdin_max.min(total.saturating_sub(elapsed())))?;
    session_of(&input)?;
    decide(input, total.saturating_sub(elapsed()))
}

pub fn run(layout: &Layout) -> i32 {
    let started = Instant::now();
    let out = budgeted(
        TOTAL_BUDGET,
        STDIN_WAIT,
        &|| started.elapsed(),
        read_stdin,
        |input, left| bounded(spawn_real, work_real, input, layout.data_dir.clone(), left),
    );
    if let Some(text) = out {
        let mut stdout = std::io::stdout();
        let _ = stdout.write_all(text.as_bytes());
        let _ = stdout.flush();
    }
    0
}
