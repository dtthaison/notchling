use crate::acting_ops::Room;
use crate::inbox_ops::InboxOps;
use crate::versions_ops::Clock;
use notchling_engine::control::launcher::Launcher;
use notchling_engine::control::plan::Command;
use notchling_engine::inbox::{Action, ActionId, Item, Kind, Source, State};
use notchling_engine::transcript::strict::Skips;
use notchling_engine::transcript::strict::{
    claude_lines, codex_lines, strict_claude, strict_codex,
};
use notchling_engine::vault::random_id;
use notchling_engine::versions::check::{self, CheckPart, CheckResult, CheckTier, Outcome, Output};
use notchling_engine::versions::mac::Installed;
use notchling_engine::versions::record::{Fact, Recorder};
use notchling_engine::versions::reference::{self, Book};
use notchling_engine::versions::soak::Soaks;
use notchling_engine::versions::{Tool, Version};
use notchling_local_api::topics;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

pub const RETRY_AFTER_MS: i64 = 120_000;
pub const NEWEST_FILES: usize = 20;
pub const TAIL_BYTES: u64 = 4_000_000;
const WALK_LIMIT: usize = 20_000;
const SELF_CLAUDE: &str =
    include_str!("../../engine/tests/fixtures/versions/self-test-claude.jsonl");
const SELF_CODEX: &str = include_str!("../../engine/tests/fixtures/versions/self-test-codex.jsonl");

pub type SelfTest = Arc<dyn Fn() -> bool + Send + Sync>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Checker {
    launcher: Arc<dyn Launcher>,
    home: PathBuf,
    self_test: Mutex<SelfTest>,
}

impl Checker {
    pub fn new(launcher: Arc<dyn Launcher>, home: PathBuf) -> Checker {
        Checker {
            launcher,
            home,
            self_test: Mutex::new(Arc::new(|| true)),
        }
    }

    pub fn set_self_test(&self, test: SelfTest) {
        *lock(&self.self_test) = test;
    }

    pub fn run(&self, tool: Tool, installed: &Installed) -> Outcome {
        let started = Instant::now();
        let Some(version) = installed.version.clone() else {
            return cant_tell();
        };
        let mut outputs: Vec<Output> = Vec::new();
        for command in check::commands(tool, Path::new(&installed.path)) {
            let argv: Vec<&str> = command.argv.iter().map(String::as_str).collect();
            let answer = self
                .launcher
                .run(
                    &Command::new(&argv),
                    Duration::from_secs(command.timeout_secs),
                )
                .ok()
                .map(|o| (o.code, o.combined()));
            outputs.push((command.part, answer));
        }
        if tool == Tool::Notchling {
            outputs.push((CheckPart::SelfTest, Some(self.own_check())));
        }
        let skips = self.strict(tool);
        let mut outcome = check::judge(tool, &version, &outputs, &skips);
        outcome.duration_ms = started.elapsed().as_millis() as u64;
        outcome
    }

    fn own_check(&self) -> (i32, String) {
        let test = lock(&self.self_test).clone();
        let mut skips = Skips::default();
        claude_lines(&mut skips, SELF_CLAUDE.lines().map(str::as_bytes));
        codex_lines(&mut skips, None, SELF_CODEX.lines().map(str::as_bytes));
        let clean = skips.unknown_total() == 0 && skips.turns_in("self-test") >= 2;
        (if clean && test() { 0 } else { 1 }, String::new())
    }

    fn strict(&self, tool: Tool) -> Skips {
        self.strict_since(tool, None)
    }

    pub fn strict_since(&self, tool: Tool, since_ms: Option<i64>) -> Skips {
        let mut skips = Skips::default();
        let recent = |(modified, _): &(SystemTime, PathBuf)| match since_ms {
            None => true,
            Some(ms) => modified
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64 >= ms)
                .unwrap_or(false),
        };
        match tool {
            Tool::Claude => {
                let root = self.home.join(".claude/projects");
                for (_, path) in newest(&root, 2, |_| true).iter().filter(|f| recent(f)) {
                    skips.merge(&strict_claude(path, TAIL_BYTES));
                }
            }
            Tool::Codex => {
                let root = self.home.join(".codex/sessions");
                let rollout = |p: &Path| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("rollout-"))
                };
                for (_, path) in newest(&root, 4, rollout).iter().filter(|f| recent(f)) {
                    skips.merge(&strict_codex(path, TAIL_BYTES));
                }
            }
            Tool::Agy | Tool::Notchling => {}
        }
        skips
    }
}

fn cant_tell() -> Outcome {
    Outcome {
        result: CheckResult::CantTell,
        failed_part: None,
        why: None,
        unknown_count: 0,
        unchecked: check::UNCHECKED.to_vec(),
        duration_ms: 0,
    }
}

fn newest(root: &Path, depth: usize, keep: impl Fn(&Path) -> bool) -> Vec<(SystemTime, PathBuf)> {
    let mut found: Vec<(SystemTime, PathBuf)> = Vec::new();
    let mut seen = 0usize;
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, level)) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.filter_map(|e| e.ok()) {
            seen += 1;
            if seen > WALK_LIMIT {
                break;
            }
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if level + 1 < depth {
                    stack.push((path, level + 1));
                }
            } else if kind.is_file()
                && path.extension().and_then(|e| e.to_str()) == Some("jsonl")
                && keep(&path)
            {
                if let Some(modified) = entry.metadata().ok().and_then(|m| m.modified().ok()) {
                    found.push((modified, path));
                }
            }
        }
    }
    found.sort_by_key(|a| std::cmp::Reverse(a.0));
    found.truncate(NEWEST_FILES);
    found
}

pub struct Parts {
    pub reference: Value,
    pub usable: bool,
    pub failed: Value,
    pub check: Value,
    pub soak: Value,
}

pub fn outcome_json(tool: Tool, version: &Version, outcome: &Outcome, at: i64) -> Value {
    json!({
        "tool": tool.name(),
        "version": version.as_str(),
        "result": outcome.result.word(),
        "failedPart": outcome.failed_part.map(|p| p.word()),
        "why": outcome.why,
        "unknownCount": outcome.unknown_count,
        "unchecked": outcome.unchecked,
        "at": at,
    })
}

pub struct Checks {
    checker: Checker,
    data: PathBuf,
    room: Arc<dyn Room>,
    recorder: Arc<dyn Recorder>,
    clock: Clock,
    book: Mutex<Book>,
    soaks: Mutex<Soaks>,
    inbox: Mutex<Option<Arc<InboxOps>>>,
    notices: Mutex<BTreeMap<Tool, (String, Version)>>,
    running: Mutex<BTreeSet<Tool>>,
    retry: Mutex<BTreeMap<Tool, i64>>,
    last: Mutex<BTreeMap<Tool, Value>>,
    threads: Mutex<Vec<JoinHandle<()>>>,
    enabled: AtomicBool,
}

impl Checks {
    pub fn new(
        launcher: Arc<dyn Launcher>,
        home: PathBuf,
        data: PathBuf,
        room: Arc<dyn Room>,
        recorder: Arc<dyn Recorder>,
        clock: Clock,
    ) -> Arc<Checks> {
        let book = reference::load(&data, recorder.as_ref());
        let data_for_soak = data.clone();
        Arc::new(Checks {
            checker: Checker::new(launcher, home),
            data,
            room,
            recorder,
            clock,
            book: Mutex::new(book),
            soaks: Mutex::new(Soaks::load(&data_for_soak)),
            inbox: Mutex::new(None),
            notices: Mutex::new(BTreeMap::new()),
            running: Mutex::new(BTreeSet::new()),
            retry: Mutex::new(BTreeMap::new()),
            last: Mutex::new(BTreeMap::new()),
            threads: Mutex::new(Vec::new()),
            enabled: AtomicBool::new(false),
        })
    }

    pub fn set_inbox(&self, inbox: Arc<InboxOps>) {
        *lock(&self.inbox) = Some(inbox);
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::SeqCst);
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    pub fn set_self_test(&self, test: SelfTest) {
        self.checker.set_self_test(test);
    }

    pub fn is_running(&self, tool: Tool) -> bool {
        lock(&self.running).contains(&tool)
    }

    pub fn book(&self) -> Book {
        lock(&self.book).clone()
    }

    pub fn wanted(&self, tool: Tool, installed: &Installed) -> bool {
        let (Some(version), Some(identity)) = (&installed.version, &installed.identity) else {
            return false;
        };
        if lock(&self.retry).contains_key(&tool) {
            return false;
        }
        let book = lock(&self.book);
        !book.usable(tool, version, &identity.sha256) && !book.is_failed(tool, version)
    }

    pub fn request(self: &Arc<Self>, tool: Tool, installed: &Installed, again: bool) -> bool {
        if !self.enabled() || installed.version.is_none() {
            return false;
        }
        if !lock(&self.running).insert(tool) {
            return false;
        }
        let me = self.clone();
        let installed = installed.clone();
        let handle = std::thread::spawn(move || {
            me.attempt(tool, &installed, again);
            lock(&me.running).remove(&tool);
        });
        let mut threads = lock(&self.threads);
        threads.retain(|h| !h.is_finished());
        threads.push(handle);
        true
    }

    pub fn wait(&self) {
        let handles: Vec<JoinHandle<()>> = std::mem::take(&mut *lock(&self.threads));
        for handle in handles {
            let _ = handle.join();
        }
    }

    pub fn due_retries(&self) -> Vec<Tool> {
        let now = (self.clock)();
        lock(&self.retry)
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(tool, _)| *tool)
            .collect()
    }

    fn attempt(&self, tool: Tool, installed: &Installed, again: bool) {
        let Some(version) = installed.version.clone() else {
            return;
        };
        let sha = installed
            .identity
            .as_ref()
            .map(|i| i.sha256.clone())
            .unwrap_or_default();
        if again {
            lock(&self.book).check_again(tool, &version);
            lock(&self.retry).remove(&tool);
        }
        let mut outcome = self.checker.run(tool, installed);
        let now = (self.clock)();
        if outcome.result == CheckResult::CantTell {
            let retried = lock(&self.retry).remove(&tool).is_some();
            if !retried {
                lock(&self.retry).insert(tool, now + RETRY_AFTER_MS);
                self.finish(tool, &version, &outcome, now, false);
                return;
            }
            outcome.result = CheckResult::Failed;
            outcome.failed_part = Some(CheckPart::Version);
            outcome.why = Some("it didn't start".to_string());
        } else {
            lock(&self.retry).remove(&tool);
        }
        let held = {
            let mut book = lock(&self.book);
            let held = book.reference(tool).map(|r| r.version.clone());
            book.apply(tool, &version, &sha, &outcome, now);
            let _ = reference::save(&self.data, &book);
            held
        };
        self.finish(tool, &version, &outcome, now, true);
        self.notify(tool, &version, &outcome, held.as_ref(), now);
    }

    fn notify(
        &self,
        tool: Tool,
        version: &Version,
        outcome: &Outcome,
        held: Option<&Version>,
        now: i64,
    ) {
        match outcome.result {
            CheckResult::Failed => {
                self.withdraw(tool);
                let Some(inbox) = lock(&self.inbox).clone() else {
                    return;
                };
                let Some(id) = random_id("version-check:") else {
                    return;
                };
                let mut detail = outcome.why.clone().unwrap_or_default();
                if let Some(held) = held {
                    if !detail.is_empty() {
                        detail.push(' ');
                    }
                    detail.push_str(&format!("Workers stay on {held}."));
                }
                inbox.raise(
                    Item {
                        id: id.clone(),
                        kind: Kind::VersionCheckFailed,
                        title: format!("{} {} Didn't Pass", tool.title(), version),
                        detail,
                        source: Source::default(),
                        created_ms: now,
                        expires_ms: None,
                        actions: vec![Action {
                            id: ActionId::Open,
                            label: "Open Settings".to_string(),
                            primary: true,
                        }],
                        state: State::Open,
                        answer: None,
                        answered_by: None,
                        seen: false,
                        open: Some(json!({ "settings": "agents", "tool": tool.name() })),
                        closed_ms: None,
                    },
                    None,
                );
                lock(&self.notices).insert(tool, (id, version.clone()));
            }
            CheckResult::Passed | CheckResult::PartlyChecked => self.withdraw(tool),
            CheckResult::CantTell => {}
        }
    }

    fn withdraw(&self, tool: Tool) {
        let held = lock(&self.notices).remove(&tool);
        let inbox = lock(&self.inbox).clone();
        if let (Some((id, _)), Some(inbox)) = (held, inbox) {
            inbox.resolve(&id, State::Withdrawn, None, None);
        }
    }

    pub fn moved_on(&self, found: &BTreeMap<Tool, Installed>) {
        let stale: Vec<Tool> = lock(&self.notices)
            .iter()
            .filter(|(tool, (_, version))| {
                found.get(tool).and_then(|i| i.version.as_ref()) != Some(version)
            })
            .map(|(tool, _)| *tool)
            .collect();
        for tool in stale {
            self.withdraw(tool);
        }
    }

    fn finish(&self, tool: Tool, version: &Version, outcome: &Outcome, now: i64, record: bool) {
        let event = outcome_json(tool, version, outcome, now);
        lock(&self.last).insert(tool, event.clone());
        self.room
            .publish(topics::VERSIONS, topics::VERSIONS_CHECKED, &event);
        if record {
            self.recorder.record(Fact::Checked {
                tool,
                version: version.clone(),
                tier: CheckTier::FilesFlags,
                result: outcome.result,
                failed_part: outcome.failed_part,
                unknown_count: outcome.unknown_count,
                duration_ms: outcome.duration_ms,
            });
        }
    }

    pub fn soak_pass(&self) {
        if !self.enabled() {
            return;
        }
        for tool in [Tool::Claude, Tool::Codex] {
            let reference = lock(&self.book)
                .reference(tool)
                .map(|r| (r.version.clone(), r.at_ms));
            let Some((version, at_ms)) = reference else {
                lock(&self.soaks).follow(tool, None);
                continue;
            };
            let skips = self.checker.strict_since(tool, Some(at_ms));
            let mut soaks = lock(&self.soaks);
            if let Some(soak) = soaks.follow(tool, Some(&version)) {
                soak.observe(&skips);
            }
            let _ = soaks.save(&self.data);
        }
    }

    pub fn parts(&self, tool: Tool, installed: Option<&Installed>) -> Parts {
        let book = lock(&self.book);
        let reference = book.reference(tool).map_or(Value::Null, |r| {
            json!({
                "version": r.version.as_str(),
                "sha256": r.sha256,
                "result": r.result.word(),
                "partly": r.result == CheckResult::PartlyChecked,
                "at": r.at_ms,
                "unchecked": r.unchecked,
            })
        });
        let usable = installed.is_some_and(|i| match (&i.version, &i.identity) {
            (Some(v), Some(id)) => book.usable(tool, v, &id.sha256),
            _ => false,
        });
        let failed: Vec<Value> = book
            .failed_versions(tool)
            .into_iter()
            .map(|(version, f)| {
                json!({
                    "version": version,
                    "part": f.part.word(),
                    "at": f.at_ms,
                    "why": f.why,
                })
            })
            .collect();
        drop(book);
        let check = if self.is_running(tool) {
            json!({ "running": true })
        } else {
            lock(&self.last).get(&tool).cloned().unwrap_or(Value::Null)
        };
        let soak = lock(&self.soaks).get(tool).map_or(Value::Null, |s| {
            json!({
                "since": s.since.as_str(),
                "turns": s.turns,
                "unknown": s.unknown,
                "line": s.line(),
            })
        });
        Parts {
            reference,
            usable,
            failed: Value::Array(failed),
            check,
            soak,
        }
    }
}
