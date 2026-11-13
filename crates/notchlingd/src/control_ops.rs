use crate::acting_ops::{bad, text_arg, ActingOps, NoLog};
use crate::catalog_ops::CatalogService;
use notchling_engine::catalog::Tool;
use notchling_engine::control::session::home_of;
use notchling_engine::control::slash::{route, Route};
use notchling_engine::control::{
    claude_json_for, execute, plan_move, plan_new_session, plan_remove, plan_stop,
    plan_trust_folder, ControlError, NewSession, Outcome, Plan, PlanError, Step,
};
use notchling_engine::delegate::agy_trust::claude_trusts;
use notchling_engine::reply::plan as keys_plan;
use notchling_engine::sessions::lanes::lane_at_path;
use notchling_local_api::{ops, ApiError, ErrorCode};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const TRUST_WINDOW: Duration = Duration::from_secs(10 * 60);

fn failed(e: ControlError) -> ApiError {
    ApiError::new(ErrorCode::Failed, e.to_string())
}

fn refused(e: PlanError) -> ApiError {
    bad(e.to_string())
}

impl ActingOps {
    pub(crate) fn control(&self, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::SESSION_STOP => {
                let plan = plan_stop(&self.agent(args)?, &self.roots).map_err(refused)?;
                self.run(&plan)?;
                Ok(json!({"stopped": true}))
            }
            ops::SESSION_REMOVE => {
                let plan = plan_remove(&self.agent(args)?, &self.roots).map_err(refused)?;
                self.run(&plan)?;
                Ok(json!({"removed": true}))
            }
            ops::SESSION_NEW => self.new_session(args),
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

    fn new_session(&self, args: &Value) -> Result<Value, ApiError> {
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
        match self.run_raw(&plan) {
            Ok(job) => Ok(json!({"jobId": job})),
            Err(e) => {
                if matches!(e, ControlError::Untrusted(_)) {
                    if let Ok(real) = std::fs::canonicalize(folder) {
                        let mut refusals = self.refusals();
                        refusals.retain(|_, at| at.elapsed() < TRUST_WINDOW);
                        refusals.insert(real, Instant::now());
                    }
                }
                Err(failed(e))
            }
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
    match list.as_slice() {
        [] => Ok(list),
        [flag, model] if flag == "--model" && catalog.is_allowed(Tool::Claude, model) => Ok(list),
        _ => Err(bad(format!(
            "flags may only be --model with one of {}",
            catalog.allowed(Tool::Claude).join(", ")
        ))),
    }
}

fn folder_arg(args: &Value) -> Result<&str, ApiError> {
    let folder = text_arg(args, "folder")?;
    if folder.is_empty() {
        return Err(bad("folder is needed"));
    }
    Ok(folder)
}
