use super::hooks::{Want, Wanted};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const USER_HOOKS: &str = include_str!("fixtures/user-hooks.json");
pub const NO_HOOKS: &str = include_str!("fixtures/no-hooks.json");
pub const EMPTY_HOOKS: &str = include_str!("fixtures/empty-hooks.json");
pub const SWIFT_INSTALLED: &str = include_str!("fixtures/swift-installed.json");
pub const ODD: &str = include_str!("fixtures/odd.jsonc");
pub const COMPACT: &str = include_str!("fixtures/compact.json");
pub const UNKNOWN_KEYS: &str = include_str!("fixtures/unknown-keys.json");

pub const CMD: &str = "'/opt/notchling/bin/notchling' hook";

pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(tag: &str) -> Scratch {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "nl-install-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch folder");
        Scratch(dir)
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&self.0)
            .expect("read folder")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn wanted(prompt: Want, pages: Want) -> Wanted {
    Wanted {
        command: CMD.to_string(),
        prompt,
        pages,
    }
}

pub fn install_all() -> Wanted {
    wanted(Want::Install, Want::Install)
}

pub fn remove_all() -> Wanted {
    wanted(Want::Remove, Want::Remove)
}

pub fn value(text: &str) -> serde_json::Value {
    let node = super::jsonc::parse(text).expect("valid settings text");
    super::jsonc::to_value(&node, text)
}
