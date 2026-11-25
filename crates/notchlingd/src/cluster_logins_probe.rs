use crate::cluster_logins_notes::{base_of, notice, raise_once, Notes, VARIANTS};
use notchling_engine::cluster::logins::{
    claude_profile_signed_in, needed_text, probe, Flip, Sources, StatusRunner, Tracker,
    CLAUDE_PROFILE_FILE, EVERY,
};
use notchling_engine::inbox::{Kind, State};
use notchling_engine::versions::Tool;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

pub use crate::cluster_logins_run::SystemStatus;

pub const SIGNED_OUT: &str = "login-signed-out:";
const SCAN_WAIT: Duration = Duration::from_secs(30);
const SCAN_POLL: Duration = Duration::from_millis(200);
const BUSY_MS: i64 = 90_000;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

pub type Files = Arc<dyn Fn(&Path) -> bool + Send + Sync>;
pub type OwnCheck = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

const PROFILE_MAX_BYTES: u64 = 64 * 1024 * 1024;
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

pub trait Installs: Send + Sync {
    fn codex(&self) -> Option<String>;
    fn agy(&self) -> bool;
    fn home(&self) -> PathBuf;
    fn scanned(&self) -> bool;
}

impl Installs for crate::versions_ops::VersionsOps {
    fn codex(&self) -> Option<String> {
        self.tool_paths().remove(&Tool::Codex)
    }

    fn agy(&self) -> bool {
        self.tool_paths().contains_key(&Tool::Agy)
    }

    fn home(&self) -> PathBuf {
        crate::versions_ops::VersionsOps::home(self)
    }

    fn scanned(&self) -> bool {
        crate::versions_ops::VersionsOps::scanned(self)
    }
}

pub fn has_content(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() > 0)
}

pub fn has_claude_profile(home: &Path) -> bool {
    let Ok(file) = std::fs::File::open(home.join(CLAUDE_PROFILE_FILE)) else {
        return false;
    };
    let mut data = Vec::new();
    if file.take(PROFILE_MAX_BYTES).read_to_end(&mut data).is_err() {
        return false;
    }
    std::str::from_utf8(&data).is_ok_and(claude_profile_signed_in)
}

struct Inner {
    result: BTreeMap<Tool, bool>,
    own: bool,
    at: Option<i64>,
    running: Option<i64>,
    tracker: Tracker,
    key: Option<(Option<String>, bool)>,
}

pub struct LoginProbe {
    installs: Arc<dyn Installs>,
    runner: Arc<dyn StatusRunner>,
    files: Files,
    own_check: OnceLock<OwnCheck>,
    notes: Arc<dyn Notes>,
    clock: Clock,
    node: String,
    os: String,
    inner: Mutex<Inner>,
}

fn safe(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

impl LoginProbe {
    pub fn new(
        (installs, runner, files): (Arc<dyn Installs>, Arc<dyn StatusRunner>, Files),
        (notes, clock): (Arc<dyn Notes>, Clock),
        (node, os): (String, String),
    ) -> Arc<LoginProbe> {
        Arc::new(LoginProbe {
            installs,
            runner,
            files,
            own_check: OnceLock::new(),
            notes,
            clock,
            node,
            os,
            inner: Mutex::new(Inner {
                result: BTreeMap::new(),
                own: false,
                at: None,
                running: None,
                tracker: Tracker::default(),
                key: None,
            }),
        })
    }

    pub fn set_own(&self, check: OwnCheck) {
        let _ = self.own_check.set(check);
    }

    pub fn own_logins(&self) -> BTreeMap<Tool, bool> {
        let mut out = BTreeMap::new();
        if lock(&self.inner).own {
            out.insert(Tool::Claude, true);
        }
        out
    }

    pub fn logins(self: &Arc<Self>) -> BTreeMap<Tool, bool> {
        let now = (self.clock)();
        let key = (self.installs.codex(), self.installs.agy());
        let mut inner = lock(&self.inner);
        let due = match inner.at {
            None => true,
            Some(at) => {
                now.saturating_sub(at) >= EVERY.as_millis() as i64
                    || inner.key.as_ref() != Some(&key)
            }
        };
        let busy = inner
            .running
            .is_some_and(|since| now.saturating_sub(since) < BUSY_MS);
        if due && !busy {
            inner.running = Some(now);
            let me = self.clone();
            let spawned = std::thread::Builder::new()
                .name("login-probe".to_string())
                .spawn(move || me.refresh_when_ready(SCAN_WAIT, SCAN_POLL));
            if spawned.is_err() {
                inner.running = None;
            }
        }
        inner.result.clone()
    }

    pub fn refresh_when_ready(&self, wait: Duration, poll: Duration) {
        let until = Instant::now() + wait;
        while !self.installs.scanned() && Instant::now() < until {
            std::thread::sleep(poll);
        }
        if !self.refresh() {
            lock(&self.inner).running = None;
        }
    }

    pub fn refresh(&self) -> bool {
        if !self.installs.scanned() {
            return false;
        }
        let codex = self.installs.codex();
        let home = self.installs.home();
        let agy = self.installs.agy();
        let src = Sources {
            codex: codex.as_deref(),
            agy_installed: agy,
            os: &self.os,
            home: &home,
        };
        let found = probe(&src, self.runner.as_ref(), self.files.as_ref());
        let own = self.own_check.get().is_some_and(|check| check(&home));
        let flips = {
            let mut inner = lock(&self.inner);
            let flips = inner.tracker.update(&found);
            inner.result = found;
            inner.own = own;
            inner.at = Some((self.clock)());
            inner.key = Some((codex, agy));
            inner.running = None;
            flips
        };
        for flip in flips {
            self.apply(flip);
        }
        true
    }

    fn base(&self, tool: Tool) -> String {
        format!("{SIGNED_OUT}{}:{}", safe(&self.node), tool.name())
    }

    fn apply(&self, flip: Flip) {
        let base = self.base(flip.tool);
        if flip.signed_in {
            for id in self.notes.open_ids() {
                if base_of(&id) == base {
                    self.notes.withdraw(&id);
                }
            }
            return;
        }
        let text = needed_text(flip.tool, &self.node);
        let item = notice(base.clone(), Kind::LoginNeeded, text, None, (self.clock)());
        if flip.seen_before {
            self.raise_fresh(&base, item);
        } else {
            raise_once(self.notes.as_ref(), &base, item);
        }
    }

    fn raise_fresh(&self, base: &str, mut item: notchling_engine::inbox::Item) {
        for n in 1..=VARIANTS {
            let id = if n == 1 {
                base.to_string()
            } else {
                format!("{base}#{n}")
            };
            match self.notes.state(&id) {
                Some(State::Open) => return,
                Some(_) => continue,
                None => {
                    item.id = id;
                    self.notes.raise(item);
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "cluster_logins_probe_tests.rs"]
mod tests;
