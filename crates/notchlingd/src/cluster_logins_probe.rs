use crate::cluster_logins_notes::{base_of, notice, raise_once, Notes, VARIANTS};
use notchling_engine::cluster::logins::{
    argv_read_only, needed_text, probe, Flip, Sources, StatusRunner, Tracker, EVERY,
};
use notchling_engine::inbox::{Kind, State};
use notchling_engine::versions::Tool;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub const SIGNED_OUT: &str = "login-signed-out:";
const READ_MAX: u64 = 16 * 1024;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

pub type Files = Arc<dyn Fn(&Path) -> bool + Send + Sync>;
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

pub trait Installs: Send + Sync {
    fn codex(&self) -> Option<String>;
    fn agy(&self) -> bool;
    fn home(&self) -> PathBuf;
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
}

pub fn has_content(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() > 0)
}

pub struct SystemStatus;

impl StatusRunner for SystemStatus {
    fn run(&self, argv: &[String], timeout: Duration) -> Option<(i32, String)> {
        if !argv_read_only(argv) {
            return None;
        }
        let (program, args) = argv.split_first()?;
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .ok()?;
        let readers: Vec<_> = [
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        .map(|pipe| {
            std::thread::spawn(move || {
                let mut text = String::new();
                let _ = pipe.take(READ_MAX).read_to_string(&mut text);
                text
            })
        })
        .collect();
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
            }
        };
        let text: String = readers
            .into_iter()
            .map(|r| r.join().unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n");
        status.map(|s| (s.code().unwrap_or(-1), text))
    }
}

struct Inner {
    result: BTreeMap<Tool, bool>,
    at: Option<i64>,
    running: bool,
    tracker: Tracker,
}

pub struct LoginProbe {
    installs: Arc<dyn Installs>,
    runner: Arc<dyn StatusRunner>,
    files: Files,
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
            notes,
            clock,
            node,
            os,
            inner: Mutex::new(Inner {
                result: BTreeMap::new(),
                at: None,
                running: false,
                tracker: Tracker::default(),
            }),
        })
    }

    pub fn logins(self: &Arc<Self>) -> BTreeMap<Tool, bool> {
        let now = (self.clock)();
        let mut inner = lock(&self.inner);
        let stale = inner
            .at
            .is_none_or(|at| now.saturating_sub(at) >= EVERY.as_millis() as i64);
        if stale && !inner.running {
            inner.running = true;
            let me = self.clone();
            let spawned = std::thread::Builder::new()
                .name("login-probe".to_string())
                .spawn(move || me.refresh());
            if spawned.is_err() {
                inner.running = false;
            }
        }
        inner.result.clone()
    }

    pub fn refresh(&self) {
        let codex = self.installs.codex();
        let home = self.installs.home();
        let src = Sources {
            codex: codex.as_deref(),
            agy_installed: self.installs.agy(),
            os: &self.os,
            home: &home,
        };
        let found = probe(&src, self.runner.as_ref(), self.files.as_ref());
        let flips = {
            let mut inner = lock(&self.inner);
            let flips = inner.tracker.update(&found);
            inner.result = found;
            inner.at = Some((self.clock)());
            inner.running = false;
            flips
        };
        for flip in flips {
            self.apply(flip);
        }
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
