//! The data a planner produces: commands to run, key scripts for a hidden pty, and what a good run
//! means. Nothing here starts a process; [`super::launcher::Launcher`] does, [`super::run::execute`]
//! walks a [`Plan`] through one.

use std::collections::BTreeMap;

/// `CLAUDE_CONFIG_DIR`, the variable that points Claude Code at a lane's config folder.
pub const CONFIG_DIR_VAR: &str = "CLAUDE_CONFIG_DIR";

/// One process to run. `argv[0]` is a program *name* (`claude` or `git`); the launcher finds the
/// executable, so a plan reads the same on every machine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Command {
    pub argv: Vec<String>,
    /// Changes to the inherited environment: `Some` sets a variable, `None` removes it (the main
    /// login must not inherit a lane's `CLAUDE_CONFIG_DIR`).
    pub env: BTreeMap<String, Option<String>>,
    /// The working folder; `None` keeps the launcher's.
    pub cwd: Option<String>,
}

impl Command {
    pub fn new(argv: &[&str]) -> Self {
        Command {
            argv: argv.iter().map(|a| (*a).to_string()).collect(),
            ..Command::default()
        }
    }

    pub fn from_args(program: &str, args: &[String]) -> Self {
        let mut argv = vec![program.to_string()];
        argv.extend(args.iter().cloned());
        Command {
            argv,
            ..Command::default()
        }
    }

    /// Runs for the lane at `dir` (`CLAUDE_CONFIG_DIR=<dir>`), or for the main login (variable removed).
    pub fn for_config_dir(mut self, dir: Option<&str>) -> Self {
        self.env
            .insert(CONFIG_DIR_VAR.to_string(), dir.map(String::from));
        self
    }

    pub fn in_dir(mut self, cwd: &str) -> Self {
        self.cwd = Some(cwd.to_string());
        self
    }

    pub fn with_env(mut self, key: &str, value: Option<&str>) -> Self {
        self.env.insert(key.to_string(), value.map(String::from));
        self
    }

    pub fn program(&self) -> &str {
        self.argv.first().map_or("", String::as_str)
    }
}

/// What a finished step has to show for the plan to go on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    /// Exit status 0; the output is the message on failure.
    ExitZero,
    /// The output names the new job (`backgrounded · 6ce4c19c …`); an untrusted folder is told apart.
    JobId,
}

/// One process step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunStep {
    pub command: Command,
    pub timeout_secs: u64,
    pub expect: Expect,
    /// False: a failure is reported but the plan goes on (the extra copies a Remove cleans up).
    pub required: bool,
    /// Wait this long after the step succeeds (the host lets go of the transcript after `claude stop`).
    pub pause_after_ms: u64,
}

/// "Wait until the screen has been quiet this long, but give up after the timeout."
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quiet {
    pub quiet_ms: u64,
    pub timeout_ms: u64,
}

/// Keys to type into a pty, then a pause and optionally a wait for quiet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyStep {
    pub keys: String,
    pub pause_ms: u64,
    pub then_quiet: Option<Quiet>,
}

/// A hidden terminal session: start `command` in a pty of this size, wait for a line containing
/// `wait_for`, then play `keys`. The reply agent's pty executor plays it; this is only data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PtyScript {
    pub command: Command,
    pub rows: u16,
    pub cols: u16,
    pub wait_for: String,
    pub wait_timeout_secs: u64,
    pub keys: Vec<KeyStep>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Run(RunStep),
    Pty(PtyScript),
}

/// What a successful plan yields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing to return; the session list shows the effect.
    Done,
    /// A new background job; its short id is in the last step's output.
    NewJob,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<Step>,
    pub outcome: Outcome,
}

/// Why a plan could not be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// Only background jobs can be stopped, removed or moved.
    NotAJob,
    /// Moving resumes the conversation, which needs its transcript.
    NoTranscript,
    NoFolder,
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PlanError::NotAJob => "only a background session can be controlled this way",
            PlanError::NoTranscript => "this session has no transcript to resume",
            PlanError::NoFolder => "no folder to start the session in",
        })
    }
}

impl std::error::Error for PlanError {}
