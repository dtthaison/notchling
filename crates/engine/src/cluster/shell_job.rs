use std::path::{Path, PathBuf};

use super::persist::CLUSTER_DIR;

pub const MAX_COMMAND: usize = 64 * 1024;
pub const ENV_PREFIXES: [&str; 2] = ["NOTCHLING_", "CLAUDE_CODE_"];
pub const CODEX_LATER: &str = "Codex jobs on another machine come in a later version";
pub const AGY_LATER: &str = "Antigravity on another machine comes later.";
pub const JOBS_DIR: &str = "jobs";
const MAX_JOB: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellPlan {
    pub argv: Vec<String>,
    pub env_remove: Vec<String>,
    pub log: PathBuf,
}

fn job_name_ok(job: &str) -> bool {
    !job.is_empty()
        && job.len() <= MAX_JOB
        && job
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn shell_plan(
    job: &str,
    cwd: &str,
    command: &str,
    data_dir: &Path,
) -> Result<ShellPlan, String> {
    if !job_name_ok(job) {
        return Err("not a job name".to_string());
    }
    if cwd.contains('\0') {
        return Err("the folder has a NUL byte".to_string());
    }
    if !Path::new(cwd).is_absolute() {
        return Err("the folder must be an absolute path".to_string());
    }
    if command.trim().is_empty() {
        return Err("no command".to_string());
    }
    if command.contains('\0') {
        return Err("the command has a NUL byte".to_string());
    }
    if command.len() > MAX_COMMAND {
        return Err("the command is over 64 KB".to_string());
    }
    Ok(ShellPlan {
        argv: vec!["/bin/sh".to_string(), "-c".to_string(), command.to_string()],
        env_remove: ENV_PREFIXES.iter().map(|p| p.to_string()).collect(),
        log: data_dir
            .join(CLUSTER_DIR)
            .join(JOBS_DIR)
            .join(format!("{job}.log")),
    })
}

pub fn scrub(names: impl IntoIterator<Item = String>, prefixes: &[String]) -> Vec<String> {
    names
        .into_iter()
        .filter(|name| prefixes.iter().any(|p| name.starts_with(p.as_str())))
        .collect()
}
