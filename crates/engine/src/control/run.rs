use super::launcher::{LaunchError, Launcher};
use super::plan::{Expect, Plan, RunStep, Step};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlError {
    Untrusted(String),
    TrustFailed,
    Failed(String),
}

impl std::fmt::Display for ControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ControlError::Untrusted(message) | ControlError::Failed(message) => {
                f.write_str(message)
            }
            ControlError::TrustFailed => f.write_str(
                "Couldn't trust the folder. Run claude there once and accept its prompt.",
            ),
        }
    }
}

impl std::error::Error for ControlError {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub job_id: Option<String>,
    pub ignored_failures: Vec<String>,
}

pub fn is_untrusted(message: &str) -> bool {
    message.contains("not trusted")
}

pub fn job_id_in_launch_output(text: &str) -> Option<String> {
    for line in text.split('\n').filter(|l| !l.is_empty()) {
        let parts: Vec<&str> = line.split(" · ").collect();
        if parts.len() < 2 {
            continue;
        }
        if let Some(id) = parts[1].split_whitespace().next() {
            if id.chars().count() == 8 && id.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(id.to_string());
            }
        }
    }
    None
}

pub fn execute(
    plan: &Plan,
    launcher: &dyn Launcher,
    pause: &dyn Fn(u64),
) -> Result<Report, ControlError> {
    let mut report = Report::default();
    let mut deferred: Option<ControlError> = None;
    for step in &plan.steps {
        match step {
            Step::Run(run) => match run_step(run, launcher) {
                Ok(job) => {
                    report.job_id = job.or(report.job_id.take());
                    if run.pause_after_ms > 0 {
                        pause(run.pause_after_ms);
                    }
                }
                Err(error) if run.required && run.keep_going => {
                    deferred.get_or_insert(error);
                }
                Err(error) if run.required => return Err(error),
                Err(error) => report.ignored_failures.push(error.to_string()),
            },
            Step::Pty(script) => match launcher.run_pty(script) {
                Ok(true) => {}
                Ok(false) | Err(_) => return Err(ControlError::TrustFailed),
            },
        }
    }
    match deferred {
        Some(error) => Err(error),
        None => Ok(report),
    }
}

fn run_step(run: &RunStep, launcher: &dyn Launcher) -> Result<Option<String>, ControlError> {
    let program = run.command.argv.get(1).map_or("", String::as_str);
    let out = launcher
        .run(&run.command, Duration::from_secs(run.timeout_secs))
        .map_err(|e| match e {
            LaunchError::Timeout(_) => ControlError::Failed(format!(
                "claude {program} didn't finish in {} s.",
                run.timeout_secs
            )),
            other => ControlError::Failed(other.to_string()),
        })?;
    let combined = out.combined();
    let text = combined.trim();
    match run.expect {
        Expect::ExitZero if out.code == 0 => Ok(None),
        Expect::ExitZero if text.is_empty() => Err(ControlError::Failed(format!(
            "claude {program} exited with code {}.",
            out.code
        ))),
        Expect::ExitZero => Err(ControlError::Failed(text.to_string())),
        Expect::JobId => match job_id_in_launch_output(&combined) {
            Some(id) => Ok(Some(id)),
            None if is_untrusted(&combined) => Err(ControlError::Untrusted(text.to_string())),
            None if text.is_empty() => Err(ControlError::Failed(format!(
                "claude --bg exited with code {}.",
                out.code
            ))),
            None => Err(ControlError::Failed(text.to_string())),
        },
    }
}
