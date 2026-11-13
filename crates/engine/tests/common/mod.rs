#![allow(dead_code)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub fn notchling_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../legacy/macos/Tests/NotchlingTests/Fixtures")
        .join(name)
}

pub fn console_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../legacy/macos/Tests/ConsoleCoreTests/Fixtures")
        .join(name)
}

pub fn s(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub struct Scratch {
    pub dir: PathBuf,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

impl Scratch {
    pub fn new(tag: &str) -> Scratch {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("notchling-{tag}-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        Scratch { dir }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    pub fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.path(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent");
        }
        fs::write(&path, text).expect("write");
        path
    }

    pub fn append(&self, name: &str, text: &str) {
        let mut file = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(self.path(name))
            .expect("open");
        file.write_all(text.as_bytes()).expect("append");
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

pub fn lines_of(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .expect("read fixture")
        .lines()
        .map(String::from)
        .collect()
}
