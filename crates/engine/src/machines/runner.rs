use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const TIMED_OUT: i32 = 124;
pub const NOT_FOUND: i32 = 127;
pub const REFUSED: i32 = 126;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandResult {
    pub code: i32,
    pub out: String,
    pub err: String,
}

impl CommandResult {
    pub fn new(code: i32, out: &str, err: &str) -> CommandResult {
        CommandResult {
            code,
            out: out.into(),
            err: err.into(),
        }
    }

    pub fn ok(out: &str) -> CommandResult {
        CommandResult::new(0, out, "")
    }

    pub fn refused(why: &str) -> CommandResult {
        CommandResult::new(REFUSED, "", why)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Channel {
    Stdout,
    Stderr,
}

pub type OutputSink = dyn Fn(Channel, &str) + Send + Sync;

#[derive(Clone)]
pub struct Call {
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
    pub on_output: Option<Arc<OutputSink>>,
}

impl fmt::Debug for Call {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Call")
            .field("argv", &self.argv)
            .field("cwd", &self.cwd)
            .field("env", &self.env.iter().map(|(k, _)| k).collect::<Vec<_>>())
            .field("stdin_bytes", &self.stdin.as_ref().map(Vec::len))
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl Call {
    pub fn new(argv: Vec<String>, timeout: Duration) -> Call {
        Call {
            argv,
            cwd: None,
            env: Vec::new(),
            stdin: None,
            timeout,
            on_output: None,
        }
    }

    pub fn secs(argv: Vec<String>, secs: u64) -> Call {
        Call::new(argv, Duration::from_secs(secs))
    }

    pub fn stdin(mut self, data: Vec<u8>) -> Call {
        self.stdin = Some(data);
        self
    }

    pub fn env(mut self, key: &str, value: &str) -> Call {
        self.env.push((key.into(), value.into()));
        self
    }

    pub fn cwd(mut self, dir: PathBuf) -> Call {
        self.cwd = Some(dir);
        self
    }

    pub fn streaming(mut self, sink: Option<Arc<OutputSink>>) -> Call {
        self.on_output = sink;
        self
    }
}

pub trait Runner: Send + Sync {
    fn run(&self, call: &Call) -> CommandResult;
}

pub struct Refusing(pub &'static str);

pub const OFF: &str =
    "Machines is off: the Swift app sets up machines until the maintainer turns this on";

impl Runner for Refusing {
    fn run(&self, _call: &Call) -> CommandResult {
        CommandResult::refused(self.0)
    }
}

pub fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

#[derive(Clone, PartialEq, Eq)]
pub struct Recorded {
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub stdin: Option<Vec<u8>>,
    pub cwd: Option<PathBuf>,
}

impl fmt::Debug for Recorded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Recorded")
            .field("argv", &self.argv)
            .field("stdin_bytes", &self.stdin.as_ref().map(Vec::len))
            .finish()
    }
}

impl Recorded {
    pub fn text(&self) -> String {
        self.argv.join(" ")
    }

    pub fn stdin_text(&self) -> String {
        self.stdin
            .as_ref()
            .map(|d| String::from_utf8_lossy(d).into_owned())
            .unwrap_or_default()
    }
}

type Matcher = Box<dyn Fn(&[String]) -> bool + Send + Sync>;
type Answer = Box<dyn Fn(&Call) -> CommandResult + Send + Sync>;

struct Rule {
    matches: Matcher,
    answer: Answer,
    chunks: Vec<(Channel, String)>,
}

#[derive(Default)]
pub struct ScriptedRunner {
    rules: Mutex<Vec<Rule>>,
    seen: Mutex<Vec<Recorded>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl ScriptedRunner {
    pub fn new() -> ScriptedRunner {
        ScriptedRunner::default()
    }

    pub fn on(
        &self,
        matches: impl Fn(&[String]) -> bool + Send + Sync + 'static,
        result: CommandResult,
    ) -> &Self {
        self.on_with(matches, result, Vec::new())
    }

    pub fn on_with(
        &self,
        matches: impl Fn(&[String]) -> bool + Send + Sync + 'static,
        result: CommandResult,
        chunks: Vec<(Channel, String)>,
    ) -> &Self {
        lock(&self.rules).push(Rule {
            matches: Box::new(matches),
            answer: Box::new(move |_| result.clone()),
            chunks,
        });
        self
    }

    pub fn on_call(
        &self,
        matches: impl Fn(&[String]) -> bool + Send + Sync + 'static,
        answer: impl Fn(&Call) -> CommandResult + Send + Sync + 'static,
    ) -> &Self {
        lock(&self.rules).push(Rule {
            matches: Box::new(matches),
            answer: Box::new(answer),
            chunks: Vec::new(),
        });
        self
    }

    pub fn on_script(&self, needle: &'static str, result: CommandResult) -> &Self {
        self.on(
            move |a| a.last().is_some_and(|s| s.contains(needle)),
            result,
        )
    }

    pub fn calls(&self) -> Vec<Recorded> {
        lock(&self.seen).clone()
    }
}

impl Runner for ScriptedRunner {
    fn run(&self, call: &Call) -> CommandResult {
        lock(&self.seen).push(Recorded {
            argv: call.argv.clone(),
            env: call.env.clone(),
            stdin: call.stdin.clone(),
            cwd: call.cwd.clone(),
        });
        let rules = lock(&self.rules);
        let Some(rule) = rules.iter().find(|r| (r.matches)(&call.argv)) else {
            return CommandResult::ok("");
        };
        if let Some(sink) = &call.on_output {
            for (channel, text) in &rule.chunks {
                sink(*channel, text);
            }
        }
        (rule.answer)(call)
    }
}

impl<R: Runner + ?Sized> Runner for Arc<R> {
    fn run(&self, call: &Call) -> CommandResult {
        (**self).run(call)
    }
}
