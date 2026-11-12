//! The Artifact tool's hook events inside `notchling hook`: the local pages library, answered in
//! the hook's own process (the engine's [`notchling_engine::pages::hook`] does the work).
//!
//! Like the Swift `--hook`, which handled these "before the app-running check", this needs no
//! daemon, no app and no `acting` switch: a publish is saved, a whole-page read of a saved link is
//! answered from the copy, and a failed read is pointed at it, whether or not Notchling runs.
//!
//! - The library is `<data dir>/pages`, the folder `notchling pages` and the pages window read
//!   (`pages.json`, written under its `.lock` flock, so a hook in another session, the daemon's
//!   reads and this one never lose each other's entry). Under a fixture root it is the root's.
//! - A first publish is saved under the account in use where the session runs: the lane the
//!   hook's `CLAUDE_CONFIG_DIR` names, else the main login ([`notchling_engine::pages::account`]).
//! - Never a failure for Claude Code: any error, a panic or [`DEADLINE`] is a pass (no output,
//!   exit 0), and a publish that couldn't be saved says nothing.
//! - Not handled: publishing to a page another saved account owns. That needs the accounts
//!   switch (the daemon's) and a saved login; see `notchling_engine::pages::owner`.

use crate::layout::Layout;
use notchling_engine::lanes::VARIABLE;
use notchling_engine::pages::account::active_email;
use notchling_engine::pages::{hook, PageLibrary, RecordContext};
use serde_json::Value;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long the work may take: under the 10 s Claude Code gives the page hooks that only copy
/// files. The library's lock alone waits at most 2 s, and every file read is capped, so this is
/// for a disk that stops answering.
pub const DEADLINE: Duration = Duration::from_secs(8);

/// Where the hook reads and writes.
#[derive(Clone, Debug)]
pub struct PagesEnv {
    /// The library: `<data dir>/pages`.
    pub pages_dir: PathBuf,
    /// Read as `~`: where `~/.claude.json` is, and where a relative `file_path` is read from when
    /// the event has no `cwd`.
    pub home: PathBuf,
    /// Notchling's data folder (`lanes.json`).
    pub data_dir: PathBuf,
    /// The hook's `CLAUDE_CONFIG_DIR`: a lane's folder, else unset.
    pub config_dir: Option<String>,
    pub deadline: Duration,
}

impl PagesEnv {
    /// For this process.
    pub fn for_process(layout: &Layout) -> PagesEnv {
        PagesEnv {
            pages_dir: layout.data_dir.join("pages"),
            home: layout.home(),
            data_dir: layout.data_dir.clone(),
            config_dir: std::env::var(VARIABLE).ok(),
            deadline: DEADLINE,
        }
    }
}

/// What `notchling hook` prints for an Artifact `event`, or `None` for nothing. Does the work on
/// a thread that is given up on after `env.deadline` (the process then exits and ends it), and a
/// panic there is a pass too.
pub fn answer(event: Value, env: PagesEnv) -> Option<Value> {
    let deadline = env.deadline;
    with_deadline(deadline, move || answer_now(&event, &env)).flatten()
}

/// [`answer`] on this thread.
pub fn answer_now(event: &Value, env: &PagesEnv) -> Option<Value> {
    let lib = PageLibrary::writable(&env.pages_dir);
    // Only a publish needs the account, and finding it reads `~/.claude.json`.
    let account = if hook::may_publish(event) {
        active_email(&env.home, &env.data_dir, env.config_dir.as_deref())
    } else {
        String::new()
    };
    let ctx = RecordContext {
        account: &account,
        now: now_secs(),
        home: &env.home,
    };
    hook::answer(&lib, event, &ctx)
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// `work`'s result, or `None` when it panicked, couldn't start or took longer than `limit`.
fn with_deadline<T: Send + 'static>(
    limit: Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("hook-pages".into())
        .spawn(move || {
            let _ = tx.send(catch_unwind(AssertUnwindSafe(work)));
        })
        .ok()?;
    rx.recv_timeout(limit).ok()?.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_that_panics_or_hangs_is_a_pass() {
        assert_eq!(with_deadline(Duration::from_secs(5), || 7), Some(7));
        assert_eq!(
            with_deadline(Duration::from_secs(5), || -> u8 { panic!("boom") }),
            None
        );
        let started = std::time::Instant::now();
        let slow = with_deadline(Duration::from_millis(100), || {
            std::thread::sleep(Duration::from_secs(3));
            1
        });
        assert_eq!(slow, None);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
