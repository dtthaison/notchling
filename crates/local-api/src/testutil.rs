//! Scratch folders for tests. Under `/tmp` on Unix, since a socket path must stay short (104 bytes
//! on macOS) and the macOS temp dir is long. Nothing here touches the real data folder.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// A fresh folder, mode 0700 on Unix, removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let base = if cfg!(unix) {
            PathBuf::from("/tmp")
        } else {
            std::env::temp_dir()
        };
        let n = N.fetch_add(1, Ordering::SeqCst);
        let path = base.join(format!("nl-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        notchling_platform::files::ensure_private_dir(&path).expect("make temp dir");
        TempDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
