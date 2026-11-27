use crate::cluster::grant_files::GrantValue;
use std::collections::BTreeMap;

pub const CONFIG_DIR_VAR: &str = "CLAUDE_CONFIG_DIR";
pub const LOGIN_TOKEN_VAR: &str = "CLAUDE_CODE_OAUTH_TOKEN";

#[derive(Clone, Debug, Default)]
pub struct Command {
    pub argv: Vec<String>,
    pub env: BTreeMap<String, Option<String>>,
    pub cwd: Option<String>,
    pub secret_env: BTreeMap<String, GrantValue>,
}

impl PartialEq for Command {
    fn eq(&self, other: &Command) -> bool {
        self.argv == other.argv
            && self.env == other.env
            && self.cwd == other.cwd
            && self.secret_env.len() == other.secret_env.len()
            && self
                .secret_env
                .iter()
                .zip(&other.secret_env)
                .all(|((ka, va), (kb, vb))| ka == kb && va.expose() == vb.expose())
    }
}

impl Eq for Command {}

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

    pub fn with_secret_env(mut self, key: &str, value: GrantValue) -> Self {
        self.secret_env.insert(key.to_string(), value);
        self
    }

    pub fn keys_only(&self) -> Command {
        let mut shown = self.clone();
        shown.secret_env = self
            .secret_env
            .keys()
            .map(|key| (key.clone(), GrantValue::new(String::new())))
            .collect();
        shown
    }
}

impl Plan {
    pub fn with_claude_secret_env(&self, key: &str, value: &GrantValue) -> Plan {
        let steps = self
            .steps
            .iter()
            .map(|step| match step {
                Step::Run(run) if run.command.program() == "claude" => {
                    let mut run = run.clone();
                    run.command = run.command.with_secret_env(key, value.clone());
                    Step::Run(run)
                }
                other => other.clone(),
            })
            .collect();
        Plan {
            steps,
            outcome: self.outcome,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    ExitZero,
    JobId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunStep {
    pub command: Command,
    pub timeout_secs: u64,
    pub expect: Expect,
    pub required: bool,
    pub keep_going: bool,
    pub pause_after_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quiet {
    pub quiet_ms: u64,
    pub timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyStep {
    pub keys: String,
    pub pause_ms: u64,
    pub then_quiet: Option<Quiet>,
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done,
    NewJob,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<Step>,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    NotAJob,
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
