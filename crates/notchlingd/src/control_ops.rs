//! Session control (E11) as daemon ops: `engine::control`'s planners, run through the daemon's
//! [`notchling_engine::control::Launcher`]. App tier and behind the acting switch like every acting
//! op ([`crate::acting_ops`] checks both before these run).
//!
//! | Op | Args | Reply |
//! | --- | --- | --- |
//! | `session.stop` | `{id}` | `{"stopped": true}` |
//! | `session.remove` | `{id}` | `{"removed": true}` |
//! | `session.new` | `{folder, config_dir?, flags?: ["--model", <an allowed model>]}` | `{"jobId": "6ce4c19c" or null}` |
//! | `folder.trust` | `{folder, config_dir?}` | `{"trusted": true}` |
//! | `session.move` | `{id, to_config_dir}` | `{"jobId": ...}` |
//! | `session.command` | `{id, command}` | `{"sent": true}` |
//!
//! `config_dir` and `to_config_dir` name a lane by its folder (a `lanes.json` entry), or the main
//! login by `~/.claude` (or `null`, or left out); any other folder is refused. `session.new`
//! refused by Claude Code for an untrusted folder fails with Claude Code's words ("Workspace not
//! trusted"); `folder.trust` then a second `session.new` is the way on. `session.command` types a
//! slash command like a reply; one that opens a menu (`/plugin`, `/mcp`…) needs a visible terminal,
//! so it is refused here.
//!
//! What a caller may change is narrow, since whoever holds the app tier for a moment shouldn't be
//! able to widen what a session may do:
//!
//! - `flags` is nothing, or exactly `--model <id>` with a model the catalog allows: the built-in
//!   ones plus what the tool lists live (`Catalog::is_allowed`, which sees through a `[1m]` or a
//!   date suffix). The user's `catalog.json` renames and prices models but never adds one.
//!   Anything else is refused, naming the allowed ids.
//! - `folder.trust` takes an absolute path to a folder that exists, isn't `/` or the home folder,
//!   and that `session.new` was refused as untrusted within the last [`TRUST_WINDOW`] (compared
//!   canonical). It trusts only what the user just tried to start a session in.

use crate::acting_ops::{bad, text_arg, ActingOps, NoLog};
use crate::catalog_ops::CatalogService;
use notchling_engine::catalog::Tool;
use notchling_engine::control::session::home_of;
use notchling_engine::control::slash::{route, Route};
use notchling_engine::control::{
    execute, plan_move, plan_new_session, plan_remove, plan_stop, plan_trust_folder, ControlError,
    NewSession, Outcome, Plan, PlanError, Step,
};
use notchling_engine::reply::plan as keys_plan;
use notchling_engine::sessions::lanes::lane_at_path;
use notchling_local_api::{ops, ApiError, ErrorCode};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long after `session.new` was refused a folder may be trusted.
pub const TRUST_WINDOW: Duration = Duration::from_secs(10 * 60);

fn failed(e: ControlError) -> ApiError {
    ApiError::new(ErrorCode::Failed, e.to_string())
}

fn refused(e: PlanError) -> ApiError {
    bad(e.to_string())
}

impl ActingOps {
    /// Runs one control op; the switch and the tier were checked already.
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
                let plan = plan_move(&agent, to.as_deref(), &self.roots).map_err(refused)?;
                Ok(json!({"jobId": self.run(&plan)?}))
            }
            ops::SESSION_COMMAND => self.command(args),
            _ => Err(ApiError::internal(format!("{op} has no handler"))),
        }
    }

    fn run(&self, plan: &Plan) -> Result<Option<String>, ApiError> {
        self.run_raw(plan).map_err(failed)
    }

    /// Runs a control plan through the daemon's launcher (Move All's stops and resumes too).
    pub(crate) fn run_raw(&self, plan: &Plan) -> Result<Option<String>, ControlError> {
        let pause = |ms: u64| (self.pause)(ms);
        execute(plan, self.launcher.as_ref(), &pause).map(|report| report.job_id)
    }

    fn refusals(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<PathBuf, Instant>> {
        self.refused.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn new_session(&self, args: &Value) -> Result<Value, ApiError> {
        let folder = folder_arg(args)?;
        let dir = self.config_dir_arg(args, "config_dir")?;
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
                    // What `folder.trust` may then trust, for a while.
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

    /// The canonical folder `folder.trust` may trust: see the module's rules.
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
        let home = Path::new(&home_of(&self.roots)).to_path_buf();
        let home = std::fs::canonicalize(&home).unwrap_or(home);
        if real.parent().is_none() || real == home {
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

    /// A lane's folder as `lanes.json` spells it, or `None` for the main login (named by its folder,
    /// `null`, or left out). Anything else is refused: a command must not run in a stray folder.
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

/// `flags`: nothing, or `["--model", <a model the catalog allows>]`.
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
