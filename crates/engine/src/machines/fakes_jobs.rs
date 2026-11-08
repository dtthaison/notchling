//! Test-only fakes for the Jobs side (what the Swift tests keep as private `FakeRunner`,
//! `MergeFakeRunner`, `MorningFakeRunner`, `HoldingRunner`, `Gate` and `Counter`): runners that
//! start nothing and record every call. No test using them runs ssh, `gh`, `git` or `sudo`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;

use super::registry::{Machine, MachineRepo, SdWorkerSettings};
use super::runner::{Call, CommandResult, Runner};

pub fn ok(out: &str) -> CommandResult {
    CommandResult::new(0, out, "")
}

pub fn bad(err: &str) -> CommandResult {
    CommandResult::new(1, "", err)
}

pub fn bad_code(err: &str, code: i32, out: &str) -> CommandResult {
    CommandResult::new(code, out, err)
}

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A machine with an sd worker job whose sd path has a space in it (a quoting trap), serving the
/// repo `acme/app`.
pub fn machine(name: &str) -> Machine {
    let mut m = Machine::new(name, &format!("{name}.tail"), "agent");
    let mut job = SdWorkerSettings::new(name);
    job.sd_path = "/opt/sd dir/sd.py".into();
    job.repos = vec![repo()];
    m.set_sd_worker(Some(job));
    m
}

pub fn repo() -> MachineRepo {
    MachineRepo {
        github: "acme/app".into(),
        local_path: "/Users/me/code/app".into(),
        check_command: "just check".into(),
        machine_path: None,
        verify_skill: None,
    }
}

/// One call a fake saw.
#[derive(Clone, Debug)]
pub struct Rec {
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
}

impl Rec {
    fn of(call: &Call) -> Rec {
        Rec {
            argv: call.argv.clone(),
            cwd: call.cwd.clone(),
            env: call.env.clone(),
            timeout: call.timeout,
        }
    }

    /// The executable by name, then its arguments: `git -C /t status --porcelain`.
    pub fn line(&self) -> String {
        line(&self.argv)
    }

    /// The executable's name only.
    pub fn exe(&self) -> &str {
        self.argv
            .first()
            .and_then(|a| a.rsplit('/').next())
            .unwrap_or("")
    }
}

pub fn line(argv: &[String]) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if let Some(first) = argv.first() {
        parts.push(first.rsplit('/').next().unwrap_or(""));
    }
    parts.extend(argv.iter().skip(1).map(String::as_str));
    parts.join(" ")
}

type Answer = Box<dyn Fn(&[String]) -> CommandResult + Send + Sync>;

/// Answers from a closure over the argv and records every call (Swift `FakeRunner` and
/// `MorningFakeRunner`).
pub struct AnswerRunner {
    answer: Answer,
    seen: Mutex<Vec<Rec>>,
}

impl AnswerRunner {
    pub fn new(
        answer: impl Fn(&[String]) -> CommandResult + Send + Sync + 'static,
    ) -> AnswerRunner {
        AnswerRunner {
            answer: Box::new(answer),
            seen: Mutex::new(Vec::new()),
        }
    }

    /// Succeeds with `{}` for everything.
    pub fn quiet() -> AnswerRunner {
        AnswerRunner::new(|_| ok("{}"))
    }

    /// Succeeds with no output for everything.
    pub fn silent() -> AnswerRunner {
        AnswerRunner::new(|_| ok(""))
    }

    pub fn recs(&self) -> Vec<Rec> {
        lock(&self.seen).clone()
    }

    pub fn calls(&self) -> Vec<Vec<String>> {
        self.recs().into_iter().map(|r| r.argv).collect()
    }

    /// The calls to `gh`, without the path of the executable.
    pub fn gh_calls(&self) -> Vec<Vec<String>> {
        self.recs()
            .into_iter()
            .filter(|r| r.exe() == "gh")
            .map(|r| r.argv[1..].to_vec())
            .collect()
    }
}

impl Runner for AnswerRunner {
    fn run(&self, call: &Call) -> CommandResult {
        lock(&self.seen).push(Rec::of(call));
        (self.answer)(&call.argv)
    }
}

/// Scripted answers by text (Swift `MergeFakeRunner`): a rule matches when its text is in the call
/// written as `git -C /t status --porcelain` (the executable shown by name only); the first match
/// wins and anything else succeeds with no output. A queue answers one call at a time and repeats
/// its last answer; queues win over rules.
pub struct RuleRunner {
    rules: Vec<(String, CommandResult)>,
    queues: Vec<(String, Vec<CommandResult>)>,
    used: Mutex<HashMap<usize, usize>>,
    seen: Mutex<Vec<Rec>>,
}

impl RuleRunner {
    pub fn new(
        rules: Vec<(&str, CommandResult)>,
        queues: Vec<(&str, Vec<CommandResult>)>,
    ) -> RuleRunner {
        RuleRunner {
            rules: rules.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            queues: queues
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
            used: Mutex::new(HashMap::new()),
            seen: Mutex::new(Vec::new()),
        }
    }

    pub fn recs(&self) -> Vec<Rec> {
        lock(&self.seen).clone()
    }

    pub fn calls(&self) -> Vec<Vec<String>> {
        self.recs().into_iter().map(|r| r.argv).collect()
    }

    /// Each call as one line, executable by name.
    pub fn lines(&self) -> Vec<String> {
        self.recs().iter().map(Rec::line).collect()
    }
}

impl Runner for RuleRunner {
    fn run(&self, call: &Call) -> CommandResult {
        let line = line(&call.argv);
        lock(&self.seen).push(Rec::of(call));
        if let Some(q) = self
            .queues
            .iter()
            .position(|(k, _)| line.contains(k.as_str()))
        {
            let mut used = lock(&self.used);
            let n = *used.get(&q).unwrap_or(&0);
            used.insert(q, n + 1);
            let answers = &self.queues[q].1;
            return answers[n.min(answers.len() - 1)].clone();
        }
        self.rules
            .iter()
            .find(|(k, _)| line.contains(k.as_str()))
            .map(|(_, r)| r.clone())
            .unwrap_or_else(|| ok(""))
    }
}

/// Holds a thread until the test lets it go.
#[derive(Default)]
pub struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    pub fn wait(&self) {
        let mut open = lock(&self.open);
        while !*open {
            open = self.changed.wait(open).unwrap_or_else(|e| e.into_inner());
        }
    }

    pub fn open(&self) {
        *lock(&self.open) = true;
        self.changed.notify_all();
    }
}

#[derive(Default)]
pub struct Counter(AtomicUsize);

impl Counter {
    pub fn bump(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }

    pub fn value(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }

    /// Waits (a few seconds at most) until the count is above zero.
    pub fn wait_for_first(&self) {
        for _ in 0..1000 {
            if self.value() > 0 {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// Answers a status at once, except the first call, which waits for [`HoldingRunner::release`]
/// (Swift `HoldingRunner`).
pub struct HoldingRunner {
    answer: String,
    calls: Counter,
    gate: Gate,
}

impl HoldingRunner {
    pub fn new(answer: &str) -> HoldingRunner {
        HoldingRunner {
            answer: answer.into(),
            calls: Counter::default(),
            gate: Gate::default(),
        }
    }

    pub fn calls(&self) -> usize {
        self.calls.value()
    }

    pub fn wait_for_first_call(&self) {
        self.calls.wait_for_first();
    }

    pub fn release(&self) {
        self.gate.open();
    }
}

impl Runner for HoldingRunner {
    fn run(&self, _call: &Call) -> CommandResult {
        self.calls.bump();
        if self.calls.value() == 1 {
            self.gate.wait();
        }
        ok(&self.answer)
    }
}
