use notchling_engine::cluster::snapshot::repo_state::{
    filter_probe, parse, read_plan, without_filters, GitCall, GitOutput, RepoFacts,
};
use notchling_engine::control::Launcher;
use std::path::Path;
use std::time::Duration;

const CALL_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_OUTPUT: usize = 32 * 1024 * 1024;

fn run(launcher: &dyn Launcher, call: &GitCall) -> Result<GitOutput, String> {
    let (out, capped) = launcher
        .run_capped(&call.to_command(), CALL_TIMEOUT, MAX_OUTPUT)
        .map_err(|why| why.to_string())?;
    if capped {
        return Err("git answered more than can be read".to_owned());
    }
    Ok(GitOutput {
        code: out.code,
        stdout: out.output,
        stderr: out.stderr,
    })
}

pub fn read_repo(folder: &Path, launcher: &dyn Launcher) -> Result<Option<RepoFacts>, String> {
    let probe = run(launcher, &filter_probe(folder))?;
    if probe.code != 0 && probe.code != 1 {
        return Err(format!("git config exited {}", probe.code));
    }
    let calls = without_filters(read_plan(folder), &probe.stdout)?;
    let mut outputs = Vec::with_capacity(calls.len());
    for call in &calls {
        outputs.push(run(launcher, call)?);
    }
    parse(folder, &outputs)
}
