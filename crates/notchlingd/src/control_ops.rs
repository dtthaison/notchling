use crate::acting::ACTING_OFF;
use crate::acting_ops::{bad, switch_off, text_arg, ActingOps, NoLog};
use crate::catalog_ops::CatalogService;
use crate::outbox_ops::{SendArgs, NO_SESSION};
use notchling_engine::catalog::Tool;
use notchling_engine::cluster::grant_files::GrantValue;
use notchling_engine::control::session::home_of;
use notchling_engine::control::slash::{route, Route};
use notchling_engine::control::{
    claude_json_for, execute, plan_move, plan_new_session, plan_remove, plan_resume, plan_stop,
    plan_trust_folder, ControlError, NewSession, Outcome, Plan, PlanError, Step, LOGIN_TOKEN_VAR,
};
use notchling_engine::delegate::agy_trust::claude_trusts;
use notchling_engine::limits::renew::renew_command;
use notchling_engine::outbox::Via;
use notchling_engine::reply::plan as keys_plan;
use notchling_engine::sessions::history::{self, Item};
use notchling_engine::sessions::lanes::lane_at_path;
use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_local_api::{ops, ApiError, Caller, ClientKind, ErrorCode};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const TRUST_WINDOW: Duration = Duration::from_secs(10 * 60);
pub const SIGNED_OUT: &str = "worker_claude_signed_out";
const SIGNED_OUT_TEXT: &str =
    "Claude Code isn't signed in on this machine. Sign in there, then dispatch again.";
const STATUS_WAIT: Duration = Duration::from_secs(15);
const FIRST_MESSAGE_TRIES: u32 = 20;
const FIRST_MESSAGE_GAP_MS: u64 = 250;

#[cfg(test)]
const _: () = assert!(
    notchling_engine::cluster::core::START_CALL_MS
        > notchling_engine::control::session::START_STOP_TIMEOUT_SECS * 1000
            + STATUS_WAIT.as_millis() as u64
            + FIRST_MESSAGE_TRIES as u64 * FIRST_MESSAGE_GAP_MS
);

const FIRST_MESSAGE_LOST: &str =
    "The session started but its first message wasn't queued. Send it with notchling session reply.";

fn first_message<'a>(caller: &Caller, args: &'a Value) -> Option<&'a str> {
    if caller.client != ClientKind::Node {
        return None;
    }
    args.get("text")
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
}

fn failed(e: ControlError) -> ApiError {
    ApiError::new(ErrorCode::Failed, e.to_string())
}

fn refused(e: PlanError) -> ApiError {
    bad(e.to_string())
}

pub(crate) fn agent_from_history(item: Item) -> Agent {
    let project = Path::new(&item.folder)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Agent {
        id: item.id.clone(),
        name: if item.title.is_empty() {
            project.clone()
        } else {
            item.title
        },
        project,
        cwd: item.folder.clone(),
        group: AgentGroup::Completed,
        label: "Exited".to_string(),
        detail: None,
        since_ms: item.modified_ms,
        target: AgentTarget::Resume {
            session_id: item.id,
            cwd: item.folder,
        },
        suggested_reply: None,
        transcript_path: item.path,
        host: String::new(),
        headless: false,
        config_dir: item.config_dir,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
        worktree_path: None,
    }
}

impl ActingOps {
    pub(crate) fn resume_session(&self, args: &Value) -> Result<Value, ApiError> {
        if !self.acting.is_on() {
            return Err(switch_off("acting", ACTING_OFF));
        }
        let id = text_arg(args, "id")?;
        let agent = match self.agent(args) {
            Ok(agent) => agent,
            Err(_) => {
                let sources = history::sources(&self.roots);
                history::History::new()
                    .find(&sources, id)
                    .map(agent_from_history)
                    .ok_or_else(|| bad(NO_SESSION).with_reason("no_session"))?
            }
        };
        let plan =
            plan_resume(&agent, &self.roots).map_err(|e| bad(e.message()).with_reason("usage"))?;
        self.onboard_lane(agent.config_dir.as_deref());
        match self.run_raw(&plan) {
            Ok(job) => Ok(json!({"jobId": job})),
            Err(e) => {
                if matches!(e, ControlError::Untrusted(_)) {
                    if let Ok(real) = std::fs::canonicalize(&agent.cwd) {
                        let mut refusals = self.refusals();
                        refusals.retain(|_, at| at.elapsed() < TRUST_WINDOW);
                        refusals.insert(real, Instant::now());
                    }
                }
                Err(failed(e))
            }
        }
    }

    pub(crate) fn control(
        &self,
        caller: &Caller,
        op: &str,
        args: &Value,
    ) -> Result<Value, ApiError> {
        match op {
            ops::SESSION_STOP => {
                let plan = plan_stop(&self.agent(args)?, &self.roots).map_err(refused)?;
                self.run(&plan)?;
                Ok(json!({"stopped": true}))
            }
            ops::SESSION_REMOVE => {
                let agent = self.agent(args)?;
                let plan = plan_remove(&agent, &self.roots).map_err(refused)?;
                self.run(&plan)?;
                if let Some(outbox) = self.outbox() {
                    outbox.remove_session(&agent.outbox_key());
                }
                Ok(json!({"removed": true}))
            }
            ops::SESSION_NEW => self.new_session(caller, args),
            ops::FOLDER_TRUST => {
                let folder = folder_arg(args)?;
                let dir = self.config_dir_arg(args, "config_dir")?;
                let real = self.trustable(folder)?;
                self.onboard_lane(dir.as_deref());
                let plan = Plan {
                    steps: vec![Step::Pty(plan_trust_folder(folder, dir.as_deref()))],
                    outcome: Outcome::Done,
                };
                self.run(&plan)?;
                self.refusals().remove(&real);
                Ok(json!({"trusted": true}))
            }
            ops::SESSION_MOVE => {
                let agent = self.agent(args)?;
                if args.get("to_config_dir").is_none() {
                    return Err(bad(
                        "to_config_dir is needed (the main login's folder or a lane's)",
                    ));
                }
                let to = self.config_dir_arg(args, "to_config_dir")?;
                self.onboard_lane(to.as_deref());
                let trust = self.needs_trust(&agent.cwd, to.as_deref());
                let plan = plan_move(&agent, to.as_deref(), &self.roots, trust).map_err(refused)?;
                Ok(json!({"jobId": self.run(&plan)?}))
            }
            ops::SESSION_COMMAND => self.command(args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    fn run(&self, plan: &Plan) -> Result<Option<String>, ApiError> {
        self.run_raw(plan).map_err(failed)
    }

    pub(crate) fn respawn_guarded<R>(&self, job: &str, then: impl FnOnce() -> R) -> R {
        let lock = self.respawns.for_job(job);
        let _one = lock.lock().unwrap_or_else(|e| e.into_inner());
        then()
    }

    pub(crate) fn respawn(&self, job: &str) -> Result<Option<String>, ControlError> {
        self.respawn_guarded(job, || {
            self.run_raw(&notchling_engine::control::plan_respawn(job, &self.roots))
        })
    }

    pub(crate) fn run_raw(&self, plan: &Plan) -> Result<Option<String>, ControlError> {
        let pause = |ms: u64| (self.pause)(ms);
        execute(plan, self.launcher.as_ref(), &pause).map(|report| report.job_id)
    }

    fn refusals(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<PathBuf, Instant>> {
        self.refused.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn onboard_lane(&self, dir: Option<&str>) {
        let Some(lane) = dir.and_then(|d| lane_at_path(&self.roots, d)) else {
            return;
        };
        let home = PathBuf::from(home_of(&self.roots));
        let store = notchling_engine::lanes::LaneStore {
            main_dir: self.roots.main_dir.clone(),
            main_config: home.join(".claude.json"),
            lanes_dir: Path::new(&lane.path)
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
            lanes_file: self.roots.lanes_file.clone(),
        };
        let _ = store.finish_onboarding(&lane);
    }

    fn new_session(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let folder = folder_arg(args)?;
        let dir = self.config_dir_arg(args, "config_dir")?;
        self.onboard_lane(dir.as_deref());
        let flags = flags_arg(args, self.catalog())?;
        let plan = plan_new_session(&NewSession {
            folder,
            config_dir: dir.as_deref(),
            flags: &flags,
            trust: false,
        })
        .map_err(refused)?;
        let (plan, token) = self.with_login(caller, plan);
        match self.run_raw(&plan) {
            Ok(job) => {
                if let (Some(token), Some(job), Some(login)) = (&token, &job, self.login.get()) {
                    let _ = login.launched(job, token);
                }
                let mut reply = json!({"jobId": job});
                if let (Some(text), Some(job)) = (first_message(caller, args), &job) {
                    let queued = self.queue_first_message(job, text);
                    reply["messageQueued"] = json!(queued);
                    if !queued {
                        reply["warning"] = json!(FIRST_MESSAGE_LOST);
                    }
                }
                Ok(reply)
            }
            Err(e) => {
                if matches!(e, ControlError::Untrusted(_)) {
                    if let Ok(real) = std::fs::canonicalize(folder) {
                        let mut refusals = self.refusals();
                        refusals.retain(|_, at| at.elapsed() < TRUST_WINDOW);
                        refusals.insert(real, Instant::now());
                    }
                }
                if caller.client == ClientKind::Node
                    && token.is_none()
                    && matches!(e, ControlError::Failed(_))
                    && self.claude_signed_out(dir.as_deref())
                {
                    return Err(ApiError::new(ErrorCode::Failed, SIGNED_OUT_TEXT)
                        .with_reason(SIGNED_OUT)
                        .with_field("wire", json!(SIGNED_OUT)));
                }
                Err(failed(e))
            }
        }
    }

    fn queue_first_message(&self, job: &str, text: &str) -> bool {
        let Some(outbox) = self.outbox() else {
            return false;
        };
        let send = SendArgs {
            id: job.to_string(),
            text: text.to_string(),
            clear_box: false,
            options: None,
            via: Via::Agent,
        };
        for _ in 0..FIRST_MESSAGE_TRIES {
            match outbox.send(&send) {
                Ok(_) => return true,
                Err(e) if e.message == NO_SESSION => (self.pause)(FIRST_MESSAGE_GAP_MS),
                Err(_) => return false,
            }
        }
        false
    }

    fn claude_signed_out(&self, config_dir: Option<&str>) -> bool {
        let Ok(out) = self.launcher.run(&renew_command(config_dir), STATUS_WAIT) else {
            return false;
        };
        serde_json::from_str::<Value>(out.output.trim())
            .ok()
            .is_some_and(|status| status["loggedIn"] == json!(false))
    }

    fn with_login(&self, caller: &Caller, plan: Plan) -> (Plan, Option<GrantValue>) {
        if caller.client != ClientKind::Node {
            return (plan, None);
        }
        match self.login.get().and_then(|login| login.token()) {
            Some(token) => (
                plan.with_claude_secret_env(LOGIN_TOKEN_VAR, &token),
                Some(token),
            ),
            None => (plan, None),
        }
    }

    fn trustable(&self, folder: &str) -> Result<PathBuf, ApiError> {
        let path = Path::new(folder);
        if !path.is_absolute() {
            return Err(bad("folder is an absolute path"));
        }
        let real =
            std::fs::canonicalize(path).map_err(|_| bad(format!("{folder} doesn't exist")))?;
        if !real.is_dir() {
            return Err(bad(format!("{folder} isn't a folder")));
        }
        if self.never_trusted(&real) {
            return Err(bad(
                "the root and the home folder are never trusted from here",
            ));
        }
        let fresh = self
            .refusals()
            .get(&real)
            .is_some_and(|at| at.elapsed() < TRUST_WINDOW);
        if !fresh {
            return Err(bad(
                "only a folder a new session was just refused in (\"Workspace not trusted\") can be trusted",
            ));
        }
        Ok(real)
    }

    fn never_trusted(&self, real: &Path) -> bool {
        let home = Path::new(&home_of(&self.roots)).to_path_buf();
        let home = std::fs::canonicalize(&home).unwrap_or(home);
        real.parent().is_none() || real == home
    }

    fn needs_trust(&self, folder: &str, config_dir: Option<&str>) -> bool {
        let Ok(real) = std::fs::canonicalize(folder) else {
            return false;
        };
        if !real.is_dir() || self.never_trusted(&real) {
            return false;
        }
        let login =
            std::fs::read_to_string(claude_json_for(&self.roots, config_dir)).unwrap_or_default();
        !claude_trusts(&login, Path::new(folder)) && !claude_trusts(&login, &real)
    }

    fn command(&self, args: &Value) -> Result<Value, ApiError> {
        let command = text_arg(args, "command")?.trim();
        if !command.starts_with('/') || command.len() < 2 {
            return Err(bad("a command starts with /"));
        }
        if let Some(which) = notchling_engine::chrome::command::parse(command) {
            let agent = self.agent(args)?;
            let chrome = self
                .chrome()
                .ok_or_else(|| bad("Chrome can't be turned on here"))?;
            if !crate::chrome_ops::offers(&agent) {
                return Err(bad(
                    "Chrome can only be changed for sessions Notchling can restart",
                ));
            }
            let force = args.get("force").and_then(Value::as_bool).unwrap_or(false);
            let on = which == notchling_engine::chrome::command::Command::On;
            return Ok(json!({"sent": false, "chrome": chrome.start(&agent, on, force)}));
        }
        if route(command) == Route::Terminal {
            return Err(bad(format!(
                "{command} opens a menu, which needs a terminal to show it"
            )));
        }
        let agent = self.agent(args)?;
        self.play(&agent, &keys_plan::reply(command), &mut NoLog)?;
        Ok(json!({"sent": true}))
    }

    fn config_dir_arg(&self, args: &Value, key: &str) -> Result<Option<String>, ApiError> {
        let path = match args.get(key) {
            None | Some(Value::Null) => return Ok(None),
            Some(Value::String(path)) => path,
            Some(_) => return Err(bad(format!("{key} is a folder"))),
        };
        if Path::new(path)
            .components()
            .eq(self.roots.main_dir.components())
        {
            return Ok(None);
        }
        lane_at_path(&self.roots, path)
            .map(|lane| Some(lane.path))
            .ok_or_else(|| {
                bad(format!(
                    "{path} is neither the main login's folder nor a lane's"
                ))
            })
    }
}

fn flags_arg(args: &Value, catalog: &CatalogService) -> Result<Vec<String>, ApiError> {
    let list = match args.get("flags") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(list)) => list
            .iter()
            .map(|f| f.as_str().map(String::from))
            .collect::<Option<Vec<String>>>()
            .ok_or_else(|| bad("flags is a list of strings"))?,
        Some(_) => return Err(bad("flags is a list of strings")),
    };
    let (model, rest) = match list.as_slice() {
        [flag, model, rest @ ..] if flag == "--model" => (Some(model), rest),
        rest => (None, rest),
    };
    let model_ok = model.is_none_or(|m| catalog.is_allowed(Tool::Claude, m));
    let rest_ok =
        matches!(rest, [] | [_]) && rest.iter().all(|f| CHROME_FLAGS.contains(&f.as_str()));
    if model_ok && rest_ok {
        return Ok(list);
    }
    Err(bad(format!(
        "flags may only be --model with one of {}, then --chrome or --no-chrome",
        catalog.allowed(Tool::Claude).join(", ")
    )))
}

const CHROME_FLAGS: [&str; 2] = ["--chrome", "--no-chrome"];

fn folder_arg(args: &Value) -> Result<&str, ApiError> {
    let folder = text_arg(args, "folder")?;
    if folder.is_empty() {
        return Err(bad("folder is needed"));
    }
    Ok(folder)
}
