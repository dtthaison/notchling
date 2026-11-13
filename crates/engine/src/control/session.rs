use super::plan::{Command, Expect, Outcome, Plan, PlanError, RunStep, Step};
use super::trust::plan_trust_folder;
use crate::sessions::jobs::read_json;
use crate::sessions::{
    config_dir_for_job, job_dirs, launch_flags, resume_arguments, Agent, AgentTarget, Roots,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

const START_STOP_TIMEOUT_SECS: u64 = 60;
const REMOVE_TIMEOUT_SECS: u64 = 120;
pub const STOP_SETTLE_MS: u64 = 1500;

pub fn home_of(roots: &Roots) -> String {
    roots
        .main_dir
        .parent()
        .unwrap_or(&roots.main_dir)
        .to_string_lossy()
        .into_owned()
}

fn job_id(agent: &Agent) -> Result<&str, PlanError> {
    match &agent.target {
        AgentTarget::Job(id) => Ok(id),
        _ => Err(PlanError::NotAJob),
    }
}

fn run(command: Command, timeout_secs: u64, expect: Expect) -> RunStep {
    RunStep {
        command,
        timeout_secs,
        expect,
        required: true,
        keep_going: false,
        pause_after_ms: 0,
    }
}

fn stop_step(id: &str, config_dir: Option<&str>) -> RunStep {
    RunStep {
        pause_after_ms: STOP_SETTLE_MS,
        ..run(
            Command::new(&["claude", "stop", id]).for_config_dir(config_dir),
            START_STOP_TIMEOUT_SECS,
            Expect::ExitZero,
        )
    }
}

fn start_step(cwd: &str, config_dir: Option<&str>, args: &[String]) -> RunStep {
    run(
        Command::from_args("claude", args)
            .in_dir(cwd)
            .for_config_dir(config_dir),
        START_STOP_TIMEOUT_SECS,
        Expect::JobId,
    )
}

#[derive(Clone, Debug, Default)]
pub struct NewSession<'a> {
    pub folder: &'a str,
    pub config_dir: Option<&'a str>,
    pub flags: &'a [String],
    pub trust: bool,
}

pub fn plan_new_session(request: &NewSession) -> Result<Plan, PlanError> {
    if request.folder.is_empty() {
        return Err(PlanError::NoFolder);
    }
    let mut steps = Vec::new();
    if request.trust {
        steps.push(Step::Pty(plan_trust_folder(
            request.folder,
            request.config_dir,
        )));
    }
    let mut args = request.flags.to_vec();
    args.push("--bg".to_string());
    steps.push(Step::Run(start_step(
        request.folder,
        request.config_dir,
        &args,
    )));
    Ok(Plan {
        steps,
        outcome: Outcome::NewJob,
    })
}

pub fn plan_stop(agent: &Agent, roots: &Roots) -> Result<Plan, PlanError> {
    let id = job_id(agent)?;
    Ok(Plan {
        steps: vec![Step::Run(stop_step(
            id,
            config_dir_for_job(roots, id).as_deref(),
        ))],
        outcome: Outcome::Done,
    })
}

pub fn plan_respawn(id: &str, roots: &Roots) -> Plan {
    Plan {
        steps: vec![Step::Run(run(
            Command::new(&["claude", "respawn", id])
                .for_config_dir(config_dir_for_job(roots, id).as_deref()),
            START_STOP_TIMEOUT_SECS,
            Expect::ExitZero,
        ))],
        outcome: Outcome::Done,
    }
}

pub fn job_state_path(roots: &Roots, id: &str) -> PathBuf {
    job_dir(roots, config_dir_for_job(roots, id).as_deref())
        .join("jobs")
        .join(id)
        .join("state.json")
}

pub fn plan_remove(agent: &Agent, roots: &Roots) -> Result<Plan, PlanError> {
    let id = job_id(agent)?;
    let mut dirs = job_dirs(roots, id);
    if dirs.is_empty() {
        dirs.push(None);
    }
    let steps = dirs
        .iter()
        .enumerate()
        .map(|(index, dir)| {
            Step::Run(RunStep {
                required: index == 0,
                keep_going: index == 0,
                ..run(
                    Command::new(&["claude", "rm", id]).for_config_dir(dir.as_deref()),
                    REMOVE_TIMEOUT_SECS,
                    Expect::ExitZero,
                )
            })
        })
        .collect();
    Ok(Plan {
        steps,
        outcome: Outcome::Done,
    })
}

fn job_dir(roots: &Roots, config_dir: Option<&str>) -> PathBuf {
    config_dir.map_or_else(|| roots.main_dir.clone(), PathBuf::from)
}

fn state_of(dir: &Path, id: &str) -> Option<Value> {
    read_json(&dir.join("jobs").join(id).join("state.json"))
}

pub fn flags_for_job(roots: &Roots, id: &str, name: &str) -> Vec<String> {
    let dir = job_dir(roots, config_dir_for_job(roots, id).as_deref());
    launch_flags(&state_of(&dir, id).unwrap_or(Value::Null), name)
}

pub fn conversation_of(transcript: &str) -> Option<String> {
    let stem = Path::new(transcript).file_stem()?.to_str()?;
    (!stem.is_empty()).then(|| stem.to_string())
}

pub fn resume_plan_step(
    roots: &Roots,
    id: &str,
    conversation: &str,
    cwd: &str,
    to: Option<&str>,
    flags: &[String],
) -> RunStep {
    let existing = state_of(&job_dir(roots, to), id).is_some();
    let cwd = if cwd.is_empty() {
        home_of(roots)
    } else {
        cwd.to_string()
    };
    start_step(&cwd, to, &resume_arguments(conversation, existing, flags))
}

pub fn plan_move(agent: &Agent, to: Option<&str>, roots: &Roots) -> Result<Plan, PlanError> {
    let id = job_id(agent)?;
    let transcript = agent
        .transcript_path
        .as_deref()
        .ok_or(PlanError::NoTranscript)?;
    let conversation = conversation_of(transcript).ok_or(PlanError::NoTranscript)?;
    let flags = flags_for_job(roots, id, &agent.name);
    Ok(Plan {
        steps: vec![
            Step::Run(stop_step(id, config_dir_for_job(roots, id).as_deref())),
            Step::Run(resume_plan_step(
                roots,
                id,
                &conversation,
                &agent.cwd,
                to,
                &flags,
            )),
        ],
        outcome: Outcome::NewJob,
    })
}

pub(crate) fn stop_for(id: &str, config_dir: Option<&str>) -> Plan {
    Plan {
        steps: vec![Step::Run(stop_step(id, config_dir))],
        outcome: Outcome::Done,
    }
}
