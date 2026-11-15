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

pub fn answer(
    stdin: &[u8],
    data: &Path,
    boot_id: &str,
    now: &dyn Fn() -> u64,
    sleep: &dyn Fn(u64),
) -> Option<String> {
    let session = session_of(stdin)?;
    let path = data.join(FENCE_FILE);
    let started = now();
    let mut polls = 0u64;
    loop {
        match look(&path, &session, boot_id, now()) {
            Fence::Pass => return None,
            Fence::Deny(reason) => return Some(deny_output(&reason)),
            Fence::Wait => {}
        }
        polls += 1;
        let waited = now().saturating_sub(started);
        if waited >= WAIT_MAX_MS || polls * POLL_MS > WAIT_MAX_MS {
            break;
        }
        sleep(POLL_MS);
    }
    Some(after_wait_output())
}

fn after_wait_output() -> String {
    match after_wait() {
        Fence::Deny(reason) => deny_output(&reason),
        _ => deny_output(""),
    }
}

fn real_now() -> u64 {
    notchling_platform::clock::boot_ms().unwrap_or(CLOCK_FAILED)
}

fn real_sleep(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn read_stdin(limit: Duration) -> Option<Vec<u8>> {
    let (tx, rx) = channel();
    let spawned = std::thread::Builder::new()
        .name("hook-fence-stdin".into())
        .spawn(move || {
            let _ = tx.send(slurp());
        });
    if spawned.is_err() {
        return slurp();
    }
    rx.recv_timeout(limit).ok()?
}

fn slurp() -> Option<Vec<u8>> {
    let mut input = Vec::new();
    std::io::stdin()
        .take(MAX_INPUT)
        .read_to_end(&mut input)
        .ok()
        .map(|_| input)
}

fn answer_for_real(input: &[u8], data: &Path) -> Option<String> {
    let boot_id = notchling_platform::clock::boot_id().unwrap_or_default();
    answer(input, data, &boot_id, &real_now, &real_sleep)
}

fn answer_with_deadline(input: Vec<u8>, data: PathBuf, limit: Duration) -> Option<String> {
    let (tx, rx) = channel();
    let work = (input.clone(), data.clone());
    let spawned = std::thread::Builder::new()
        .name("hook-fence".into())
        .spawn(move || {
            let _ = tx.send(answer_for_real(&work.0, &work.1));
        });
    if spawned.is_err() {
        return answer_for_real(&input, &data);
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
        |input, left| answer_with_deadline(input, layout.data_dir.clone(), left),
    );
    if let Some(text) = out {
        let mut stdout = std::io::stdout();
        let _ = stdout.write_all(text.as_bytes());
        let _ = stdout.flush();
    }
    0
}
