//! Planners for one session: New Session, Stop, Remove and Move. Each answers with a [`Plan`] and
//! starts nothing. A job's lane comes from [`crate::sessions::job_dirs`], the way
//! `AgentMonitor.configDir(forJob:)` finds it.

use super::plan::{Command, Expect, Outcome, Plan, PlanError, RunStep, Step};
use super::trust::plan_trust_folder;
use crate::sessions::jobs::read_json;
use crate::sessions::{
    config_dir_for_job, job_dirs, launch_flags, resume_arguments, Agent, AgentTarget, Roots,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// `claude stop`/`--bg` give up after this long.
const START_STOP_TIMEOUT_SECS: u64 = 60;
const REMOVE_TIMEOUT_SECS: u64 = 120;
/// The host lets go of the transcript this long after `claude stop`; resuming sooner opens it too early.
pub const STOP_SETTLE_MS: u64 = 1500;

/// The user's home folder: the parent of `~/.claude`.
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

/// What a New Session asks for.
#[derive(Clone, Debug, Default)]
pub struct NewSession<'a> {
    pub folder: &'a str,
    /// The lane's folder; `None` is the main login (the variable is removed).
    pub config_dir: Option<&'a str>,
    /// Extra `claude` flags, put before `--bg`.
    pub flags: &'a [String],
    /// Accept Claude Code's trust prompt for the folder first (after a refused start).
    pub trust: bool,
}

/// `claude --bg` in the folder. A folder Claude Code has not trusted is refused with "Workspace not
/// trusted": [`super::run::execute`] reports that as `Untrusted`, and the caller plans again with
/// `trust: true`, which puts the hidden-pty trust script first.
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

/// `claude stop <id>` for a background job, in the lane that holds it. The conversation is kept.
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

/// `claude respawn <id>` in the folder holding the job: restarts it on that folder's current login,
/// after a limit switch (`LimitWatcher.respawn`). The conversation is kept.
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

/// The job's `state.json`, in the folder holding its current copy: it changes when the job runs
/// again after a respawn.
pub fn job_state_path(roots: &Roots, id: &str) -> PathBuf {
    job_dir(roots, config_dir_for_job(roots, id).as_deref())
        .join("jobs")
        .join(id)
        .join("state.json")
}

/// `claude rm <id>` in every folder holding a copy of the job. A session moved to another account
/// left a stopped copy behind; remove that too, or it shows up again. Only the current copy's
/// result counts: the others are best effort, and none of them waits on another (a current copy
/// that fails still lets the stale ones go, as Swift's loop does).
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

/// The flags the job was started with (name, model, permission mode), read from the folder holding
/// its current copy, so the moved session keeps them.
pub fn flags_for_job(roots: &Roots, id: &str, name: &str) -> Vec<String> {
    let dir = job_dir(roots, config_dir_for_job(roots, id).as_deref());
    launch_flags(&state_of(&dir, id).unwrap_or(Value::Null), name)
}

/// The conversation id a transcript path names (its file stem).
pub fn conversation_of(transcript: &str) -> Option<String> {
    let stem = Path::new(transcript).file_stem()?.to_str()?;
    (!stem.is_empty()).then(|| stem.to_string())
}

/// `claude --bg --resume <conversation>` for a job moving into `to` (`None`: the main login), with
/// the flags passed along only when that folder has no copy of the job to continue.
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

/// Moves one background session to another account: `claude stop` (keeps the conversation), then
/// `claude --bg --resume <transcript id>` with the target's config folder and the job's
/// `respawnFlags`. `to` is the lane's folder, `None` the main login. The prompt cache stays with the
/// old account, so the first reply re-reads the whole conversation.
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
