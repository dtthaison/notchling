use crate::op_group::OpGroup;
use crate::pages_ops::Titles;
use notchling_engine::control::Launcher;
use notchling_engine::git::guard::allowed_folder;
use notchling_engine::git::{read_diff, read_status, Failure};
use notchling_engine::tools::codex::list as codex_list;
use notchling_local_api::{ops, ApiError, Caller, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const GIT_OPS: [&str; 2] = [ops::GIT_STATUS, ops::GIT_DIFF];

const KNOWN_FOR: Duration = Duration::from_secs(5);
const CODEX_ROLLOUTS: usize = 30;
const CODEX_DAYS: i64 = 14;

pub type Folders = Arc<dyn Fn() -> Vec<PathBuf> + Send + Sync>;

pub struct GitOps {
    launcher: Arc<dyn Launcher>,
    folders: Folders,
    known: Mutex<Option<(Instant, Vec<PathBuf>)>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn live_folders(
    sessions: Arc<Mutex<Value>>,
    antigravity: Titles,
    codex_sessions: PathBuf,
    fixture: bool,
) -> Folders {
    Arc::new(move || {
        let mut out: Vec<PathBuf> = lock(&sessions)
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| row["cwd"].as_str())
                    .filter(|cwd| !cwd.is_empty())
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default();
        out.extend(
            antigravity()
                .into_iter()
                .filter(|s| !s.folder.is_empty())
                .map(|s| PathBuf::from(s.folder)),
        );
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let days = (!fixture).then_some(CODEX_DAYS);
        for path in codex_list::rollouts(&codex_sessions, days, now_ms, CODEX_ROLLOUTS) {
            if let Some(agent) = codex_list::session(&path) {
                if !agent.cwd.is_empty() {
                    out.push(PathBuf::from(agent.cwd));
                }
            }
        }
        out.sort();
        out.dedup();
        out
    })
}

fn failed(failure: Failure) -> ApiError {
    match failure {
        Failure::Refused(why) => ApiError::new(ErrorCode::Forbidden, why),
        Failure::NotRepo => ApiError::new(ErrorCode::Failed, "not a git repository"),
        Failure::Failed(why) => ApiError::new(ErrorCode::Failed, why),
    }
}

fn text_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::bad_request(format!("{key} is required")))
}

impl GitOps {
    pub fn new(launcher: Arc<dyn Launcher>, folders: Folders) -> GitOps {
        GitOps {
            launcher,
            folders,
            known: Mutex::new(None),
        }
    }

    fn known(&self) -> Vec<PathBuf> {
        let cached = lock(&self.known)
            .as_ref()
            .filter(|(at, _)| at.elapsed() < KNOWN_FOR)
            .map(|(_, list)| list.clone());
        if let Some(list) = cached {
            return list;
        }
        let fresh = (self.folders)();
        *lock(&self.known) = Some((Instant::now(), fresh.clone()));
        fresh
    }

    fn allowed(&self, cwd: &str) -> Option<String> {
        allowed_folder(Path::new(cwd), &self.known()).map(|p| p.to_string_lossy().into_owned())
    }

    fn status(&self, cwd: &str) -> Result<Value, ApiError> {
        match read_status(cwd, self.launcher.as_ref()) {
            Ok(status) => {
                let mut value = serde_json::to_value(status)
                    .map_err(|e| ApiError::internal(format!("status: {e}")))?;
                value["repo"] = json!(true);
                Ok(value)
            }
            Err(Failure::NotRepo) => Ok(json!({"repo": false})),
            Err(other) => Err(failed(other)),
        }
    }

    fn diff(&self, cwd: &str, path: &str) -> Result<Value, ApiError> {
        let diff = read_diff(cwd, path, self.launcher.as_ref()).map_err(failed)?;
        serde_json::to_value(diff).map_err(|e| ApiError::internal(format!("diff: {e}")))
    }
}

impl OpGroup for GitOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        GIT_OPS.contains(&op).then_some(OpSpec::APP_QUIET)
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        let cwd = text_arg(args, "cwd")?;
        let Some(real) = self.allowed(cwd) else {
            return Err(ApiError::new(
                ErrorCode::Forbidden,
                "cwd is not a session folder",
            ));
        };
        match op {
            ops::GIT_STATUS => self.status(&real),
            ops::GIT_DIFF => self.diff(&real, text_arg(args, "path")?),
            other => Err(ApiError::internal(format!("{other} has no handler"))),
        }
    }
}
