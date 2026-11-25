#[cfg(unix)]
use crate::hook_fence::read_fd_within;
use crate::layout::Layout;
use notchling_engine::guard::{self, Would};
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const MAX_INPUT: u64 = 2 * 1024 * 1024;
pub const DEADLINE: Duration = Duration::from_millis(200);

fn clock() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

pub(crate) fn bounded<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    limit: Duration,
) -> Option<T> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("hook-guard".into())
        .spawn(move || {
            let _ = tx.send(work());
        })
        .ok()?;
    rx.recv_timeout(limit).ok()
}

pub fn process(input: Vec<u8>, dir: PathBuf, now: f64, limit: Duration) -> Option<Vec<Would>> {
    bounded(move || guard::record(&input, &dir, now), limit)
}

pub fn guard_dir(layout: &Layout) -> PathBuf {
    layout.data_dir.join(guard::DIR)
}

pub fn run_in(dir: &Path, read: impl FnOnce(Duration) -> Option<Vec<u8>>) -> i32 {
    let started = Instant::now();
    let Some(input) = read(DEADLINE) else {
        return 0;
    };
    let Some(left) = DEADLINE
        .checked_sub(started.elapsed())
        .filter(|d| !d.is_zero())
    else {
        return 0;
    };
    let _ = process(input, dir.to_path_buf(), clock(), left);
    0
}

#[cfg(unix)]
fn read_stdin(limit: Duration) -> Option<Vec<u8>> {
    read_fd_within(0, limit, MAX_INPUT)
}

#[cfg(not(unix))]
fn read_stdin(_limit: Duration) -> Option<Vec<u8>> {
    None
}

pub fn run(layout: &Layout) -> i32 {
    run_in(&guard_dir(layout), read_stdin)
}
