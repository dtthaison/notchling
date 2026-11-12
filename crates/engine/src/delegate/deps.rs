//! What a delegated run needs from the machine, all of it a parameter: the tools' binaries and
//! how they are started, the folders their media lands in, the PATH, the stop flag. Tests hand in
//! fakes and scratch folders; [`super::real`] builds the real ones.

use crate::tools::acp::Spawner as AcpSpawner;
use crate::tools::codex::process::ToolSpawner;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

/// Runs a program to its end with these arguments and returns what it printed on standard output,
/// or `None` when it did not start or did not exit 0 (`agy models`).
pub type Capture = Arc<dyn Fn(&Path, &[&str]) -> Option<String> + Send + Sync>;

#[derive(Clone)]
pub struct Deps {
    /// The `codex` command, if there is one.
    pub codex_binary: Option<PathBuf>,
    pub codex: Arc<dyn ToolSpawner>,
    /// Codex's own folder (`$CODEX_HOME` or `~/.codex`): `models_cache.json` is read from it.
    pub codex_home: PathBuf,
    /// Where Codex saves what its image tool makes, a folder per session
    /// (`<codex home>/generated_images`).
    pub codex_media_root: PathBuf,
    /// The PATH the tools Codex runs are found in.
    pub path: String,
    /// "Allow Network in Codex Turns" (`codexNetwork`): used only with `--write`, never in the
    /// read-only sandbox.
    pub network: bool,

    /// The `agy` command, if there is one.
    pub agy_binary: Option<String>,
    pub agy: Arc<dyn AcpSpawner>,
    /// Antigravity's conversation folders, one per session
    /// (`~/.gemini/antigravity-cli/brain`).
    pub agy_media_root: PathBuf,

    /// Runs `agy models`.
    pub capture: Capture,
    /// This run's nesting depth, given to the tools' own processes.
    pub depth: u32,
    /// Set by a signal (Ctrl+C, a task stop): the run stops its tool and reports.
    pub stop: Arc<AtomicBool>,
    /// After asking agy to cancel (timeout or stop), how long it gets to end the turn cleanly.
    pub cancel_grace: Duration,
    /// The two files that let a folder Claude Code trusts be trusted by agy ([`super::agy_trust`]);
    /// `None` leaves agy's list alone.
    pub agy_trust: Option<super::agy_trust::Files>,
    /// The record of this job for Chat ([`super::record`]); set by [`super::command::run`] when the
    /// caller is an agent session, so the runs only say what they learn.
    pub recorder: Option<Arc<super::record::Recorder>>,
    /// After interrupting Codex, how long before it is terminated if it is still running.
    pub terminate_after: Duration,
}

impl Deps {
    pub const CANCEL_GRACE: Duration = Duration::from_secs(2);
    pub const TERMINATE_AFTER: Duration = Duration::from_secs(5);
}
