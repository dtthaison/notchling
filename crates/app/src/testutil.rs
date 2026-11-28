use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

pub(crate) struct Scratch(PathBuf);

#[cfg(not(windows))]
fn join_rel(base: &Path, rel: &str) -> PathBuf {
    base.join(rel)
}

#[cfg(windows)]
fn join_rel(base: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .filter(|part| !part.is_empty())
        .fold(base.to_path_buf(), |path, part| path.join(part))
}

impl Scratch {
    pub(crate) fn new(tag: &str) -> Scratch {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("notchling-app-{tag}-{}-{n}", std::process::id()));
        #[cfg(windows)]
        let dir: PathBuf = dir.components().collect();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("make the scratch folder");
        Scratch(dir)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    #[cfg(unix)]
    pub(crate) fn dir(&self, rel: &str) -> PathBuf {
        let path = self.0.join(rel);
        std::fs::create_dir_all(&path).expect("make the folder");
        path
    }

    pub(crate) fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = join_rel(&self.0, rel);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("make the folder");
        std::fs::write(&path, text).expect("write the file");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
