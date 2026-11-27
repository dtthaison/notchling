use crate::cluster_fence::fence_path;
use crate::cluster_fence_keeper::CodexTurns;
use crate::codex_ops::CodexOps;
use crate::hook_fence::{real_now, real_sleep, settle};
use notchling_engine::cluster::fence::WAIT_MAX_MS;
use std::path::{Path, PathBuf};

pub type Now = Box<dyn Fn() -> u64 + Send + Sync>;
pub type Nap = Box<dyn Fn(u64) + Send + Sync>;

pub struct CodexFence {
    path: PathBuf,
    boot_id: String,
    now: Now,
    nap: Nap,
}

impl CodexFence {
    pub fn new(cluster: &Path, boot_id: &str) -> CodexFence {
        CodexFence {
            path: fence_path(cluster),
            boot_id: boot_id.to_string(),
            now: Box::new(real_now),
            nap: Box::new(real_sleep),
        }
    }

    pub fn real(cluster: &Path) -> CodexFence {
        let boot_id = notchling_platform::clock::boot_id().unwrap_or_default();
        CodexFence::new(cluster, &boot_id)
    }

    pub fn with_clock(mut self, now: Now, nap: Nap) -> CodexFence {
        self.now = now;
        self.nap = nap;
        self
    }

    fn keys<'a>(session: &'a str, row: &'a str) -> [&'a str; 2] {
        [session, row]
    }

    pub fn clear(&self, session: &str) -> Result<(), String> {
        let row = format!("codex:{session}");
        settle(
            &self.path,
            &Self::keys(session, &row),
            &self.boot_id,
            (&*self.now, &*self.nap),
            WAIT_MAX_MS,
        )
    }
}

impl CodexTurns for CodexOps {
    fn running(&self) -> Vec<String> {
        self.runs().running()
    }

    fn stop(&self, session: &str) {
        self.runs().stop(session);
    }
}
