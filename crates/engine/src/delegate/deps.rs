use crate::tools::acp::Spawner as AcpSpawner;
use crate::tools::codex::process::ToolSpawner;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

pub type Capture = Arc<dyn Fn(&Path, &[&str]) -> Option<String> + Send + Sync>;

#[derive(Clone)]
pub struct Deps {
    pub codex_binary: Option<PathBuf>,
    pub codex: Arc<dyn ToolSpawner>,
    pub codex_home: PathBuf,
    pub codex_media_root: PathBuf,
    pub path: String,
    pub network: bool,
    pub catalog_file: Option<PathBuf>,

    pub notchling_binary: PathBuf,
    pub agy_binary: Option<String>,
    pub agy: Arc<dyn AcpSpawner>,
    pub agy_media_root: PathBuf,

    pub capture: Capture,
    pub depth: u32,
    pub stop: Arc<AtomicBool>,
    pub cancel_grace: Duration,
    pub agy_trust: Option<super::agy_trust::Files>,
    pub recorder: Option<Arc<super::record::Recorder>>,
    pub terminate_after: Duration,
    pub quota: Option<super::quota::Source>,
}

impl Deps {
    pub fn codex_snapshot(&self) -> Option<PathBuf> {
        self.catalog_file
            .as_ref()
            .map(|f| f.with_file_name(crate::tools::codex::models::SNAPSHOT))
    }

    pub fn settings_file(&self) -> Option<PathBuf> {
        self.catalog_file
            .as_ref()
            .map(|f| f.with_file_name("settings.json"))
    }

    pub const CANCEL_GRACE: Duration = Duration::from_secs(2);
    pub const TERMINATE_AFTER: Duration = Duration::from_secs(5);
}
