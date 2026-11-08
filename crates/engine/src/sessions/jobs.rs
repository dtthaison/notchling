//! Background jobs and session files: `jobs/<id>/state.json` and `sessions/<pid>.json`.

use super::agent::{Agent, AgentGroup, AgentTarget};
use super::lanes::Roots;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// How current a copy of a job is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Rank {
    pub live: bool,
    pub updated_ms: i64,
}

pub fn rank(_job: &Value) -> Rank {
    todo!()
}

pub fn newest_first(_copies: &[(Option<String>, Value)]) -> Vec<Option<String>> {
    todo!()
}

pub fn agent_from_job(
    _job: &Value,
    _short_id: &str,
    _live: Option<&Value>,
    _modified: &dyn Fn(&str) -> Option<i64>,
) -> Agent {
    let _ = (AgentGroup::Working, AgentTarget::Process(0));
    todo!()
}

pub fn agent_from_session(
    _s: &Value,
    _host_app: &dyn Fn(u32) -> Option<String>,
) -> Option<Agent> {
    todo!()
}

pub fn read_jobs(_base: &Path) -> BTreeMap<String, Value> {
    todo!()
}

pub fn read_sessions(_base: &Path, _alive: &dyn Fn(u32) -> bool) -> Vec<Value> {
    todo!()
}

pub fn job_dirs(_roots: &Roots, _id: &str) -> Vec<Option<String>> {
    todo!()
}

pub fn config_dir_for_job(_roots: &Roots, _id: &str) -> Option<String> {
    todo!()
}

pub fn launch_flags(_job: &Value, _name: &str) -> Vec<String> {
    todo!()
}

pub fn resume_arguments(
    _conversation: &str,
    _existing: bool,
    _flags: &[String],
) -> Vec<String> {
    todo!()
}
