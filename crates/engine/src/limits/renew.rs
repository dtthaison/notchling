use crate::control::{Command, Launcher};
use crate::sessions::jobs::read_sessions;
use crate::sessions::{Probes, Roots};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub const RENEW_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seat {
    pub account: String,
    pub config_dir: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenewState {
    Renewing,
    Failed,
}

impl RenewState {
    pub fn wire(self) -> &'static str {
        match self {
            RenewState::Renewing => "renewing",
            RenewState::Failed => "failed",
        }
    }
}

pub fn targets(
    seats: &[Seat],
    expired: &BTreeSet<String>,
    busy: impl Fn(Option<&str>) -> bool,
) -> Vec<Seat> {
    let mut seen = BTreeSet::new();
    seats
        .iter()
        .filter(|s| expired.contains(&s.account))
        .filter(|s| seen.insert(s.account.clone()))
        .filter(|s| !busy(s.config_dir.as_deref()))
        .cloned()
        .collect()
}

pub fn renew_command(config_dir: Option<&str>) -> Command {
    Command::new(&["claude", "auth", "status"]).for_config_dir(config_dir)
}

pub trait LoginRenewer: Send + Sync {
    fn busy(&self, config_dir: Option<&str>) -> bool;
    fn renew(&self, config_dir: Option<&str>) -> Result<(), String>;
}

pub struct ClaudeRenewer {
    launcher: Arc<dyn Launcher>,
    roots: Roots,
    alive: Arc<dyn Fn(u32) -> bool + Send + Sync>,
}

impl ClaudeRenewer {
    pub fn new(launcher: Arc<dyn Launcher>, roots: Roots) -> ClaudeRenewer {
        ClaudeRenewer {
            launcher,
            roots,
            alive: Arc::new(|pid| (Probes::from_proc().is_alive)(pid)),
        }
    }

    pub fn with_alive(mut self, alive: Arc<dyn Fn(u32) -> bool + Send + Sync>) -> ClaudeRenewer {
        self.alive = alive;
        self
    }
}

impl LoginRenewer for ClaudeRenewer {
    fn busy(&self, config_dir: Option<&str>) -> bool {
        let dir = config_dir.map_or_else(|| self.roots.main_dir.clone(), PathBuf::from);
        !read_sessions(&dir, &*self.alive).is_empty()
    }

    fn renew(&self, config_dir: Option<&str>) -> Result<(), String> {
        let out = self
            .launcher
            .run(&renew_command(config_dir), RENEW_TIMEOUT)
            .map_err(|e| e.to_string())?;
        if out.code == 0 {
            Ok(())
        } else {
            Err(format!("claude exited with {}", out.code))
        }
    }
}
