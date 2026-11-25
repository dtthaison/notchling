use super::registry::{Machine, SdWorkerSettings};
use super::runner::CommandResult;
use super::shell;
use super::step::{
    run, Cadence, CheckResult, FixContext, FixOutcome, FixPlan, Probe, RemoteOutput, SetupStep,
    StepContext, StepFix, StepId,
};
use super::steps_sd::first_line_300;
use serde_json::{json, Map, Value};

pub const READ_CAP: usize = 262_144;

fn is_valid_machine_name(name: &str) -> bool {
    let mut c = name.chars();
    c.next()
        .is_some_and(|f| f.is_ascii_lowercase() || f.is_ascii_digit())
        && c.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn owned(
    machine: &Machine,
    job: &SdWorkerSettings,
    all: &[Machine],
) -> Result<Map<String, Value>, String> {
    if !is_valid_machine_name(&machine.name) {
        return Err(format!(
            "the machine name {} must be lowercase letters, digits and hyphens",
            machine.name
        ));
    }
    if job.kinds.is_empty() {
        return Err(format!("no kinds chosen for {}", machine.name));
    }
    if let Some(bad) = job.kinds.iter().find(|k| *k != "ios" && *k != "backend") {
        return Err(format!("kind {bad} must be ios or backend"));
    }
    if job.repos.is_empty() {
        return Err(format!("no repos chosen for {}", machine.name));
    }
    if let Some(repo) = job
        .repos
        .iter()
        .find(|r| r.check_command.trim_matches([' ', '\t']).is_empty())
    {
        return Err(format!("no check command for {}", repo.github));
    }
    if !job.python.starts_with('/') || !job.sd_path.starts_with('/') {
        return Err("python or sd.py is not an absolute path yet".into());
    }
    let mut others: Vec<(&str, Option<&SdWorkerSettings>)> = all
        .iter()
        .filter(|m| m.name != machine.name)
        .map(|m| (m.name.as_str(), m.sd_worker()))
        .collect();
    others.push((machine.name.as_str(), Some(job)));
    let mut repos: Vec<Value> = Vec::new();
    for repo in &job.repos {
        let mut claimed: std::collections::BTreeMap<&str, Vec<&str>> = Default::default();
        for (name, settings) in &others {
            let Some(settings) = settings else { continue };
            if !settings.repos.iter().any(|r| r.github == repo.github) {
                continue;
            }
            for kind in &settings.kinds {
                let names = claimed.entry(kind.as_str()).or_default();
                if !names.contains(name) {
                    names.push(name);
                }
            }
        }
        let mut kinds = Map::new();
        for (kind, names) in &claimed {
            if names.len() > 1 {
                let mut sorted = names.clone();
                sorted.sort_unstable();
                return Err(format!(
                    "kind {kind} of {} is claimed by {} and {}",
                    repo.github, sorted[0], sorted[1]
                ));
            }
            kinds.insert((*kind).into(), json!(names[0]));
        }
        let name = repo
            .github
            .rsplit('/')
            .find(|p| !p.is_empty())
            .unwrap_or(&repo.github);
        let path = repo
            .machine_path
            .clone()
            .unwrap_or_else(|| format!("~/src/{name}"));
        let mut entry = Map::new();
        entry.insert("github".into(), json!(repo.github));
        entry.insert("path".into(), json!(path));
        entry.insert("kinds".into(), Value::Object(kinds));
        entry.insert("check_cmd".into(), json!(repo.check_command));
        if let Some(skill) = repo.verify_skill.as_deref().filter(|s| !s.is_empty()) {
            entry.insert("verify_skill".into(), json!(format!("{path}/{skill}")));
        }
        repos.push(Value::Object(entry));
    }
    let mut out = Map::new();
    out.insert("machine".into(), json!(machine.name));
    out.insert("token_file".into(), json!(shell::GITHUB_TOKEN));
    out.insert("claude_env".into(), json!(shell::CLAUDE_ENV));
    out.insert("token_expires".into(), json!(job.token_expires));
    out.insert("health_url".into(), json!(job.health_url));
    out.insert("repos".into(), Value::Array(repos));
    Ok(out)
}

pub fn parse(text: &str) -> Option<Map<String, Value>> {
    if text.is_empty() {
        return None;
    }
    match serde_json::from_str::<Value>(text).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

pub fn merged(remote: Option<&str>, owned: &Map<String, Value>) -> Vec<u8> {
    let mut object = remote.and_then(parse).unwrap_or_default();
    let remote_repos: Vec<Value> = match object.get("repos") {
        Some(Value::Array(list)) => list.clone(),
        _ => Vec::new(),
    };
    for (key, value) in owned {
        if key != "repos" {
            object.insert(key.clone(), value.clone());
        }
    }
    let owned_repos = match owned.get("repos") {
        Some(Value::Array(list)) => list.clone(),
        _ => Vec::new(),
    };
    let repos = owned_repos
        .into_iter()
        .map(|repo| {
            let mut entry = remote_repos
                .iter()
                .find(|r| r.get("github") == repo.get("github"))
                .and_then(|r| r.as_object().cloned())
                .unwrap_or_default();
            if let Value::Object(fields) = repo {
                for (k, v) in fields {
                    entry.insert(k, v);
                }
            }
            Value::Object(entry)
        })
        .collect();
    object.insert("repos".into(), Value::Array(repos));
    super::registry::pretty_sorted(Value::Object(object))
}

pub fn differences(remote: Option<&str>, owned: &Map<String, Value>) -> Vec<String> {
    let object = remote.and_then(parse).unwrap_or_default();
    let mut keys: Vec<&String> = owned.keys().collect();
    keys.sort();
    keys.into_iter()
        .filter(|key| {
            if *key == "repos" {
                return !repos_match(object.get("repos"), owned.get("repos"));
            }
            let null = Value::Null;
            object.get(*key).unwrap_or(&null) != owned.get(*key).unwrap_or(&null)
        })
        .cloned()
        .collect()
}

fn repos_match(remote: Option<&Value>, owned: Option<&Value>) -> bool {
    let (Some(Value::Array(remote)), Some(Value::Array(owned))) = (remote, owned) else {
        return false;
    };
    if remote.len() != owned.len() || !remote.iter().all(Value::is_object) {
        return false;
    }
    owned.iter().all(|repo| {
        let Some(fields) = repo.as_object() else {
            return false;
        };
        let Some(there) = remote
            .iter()
            .find(|r| r.get("github") == repo.get("github"))
        else {
            return false;
        };
        fields.iter().all(|(k, v)| there.get(k) == Some(v))
    })
}

const PROBE: &str = r#"head -c 262144 "$HOME/.config/sd/worker.json" 2>/dev/null"#;

const READ: &str = r#"[ -f "$HOME/.config/sd/worker.json" ] || exit 0; head -c 262144 "$HOME/.config/sd/worker.json""#;

pub(super) fn step(machine: &Machine, job: &SdWorkerSettings, all: &[Machine]) -> SetupStep {
    let mut needs = vec![StepId::PLUGINS, StepId::ISSUES_TOKEN, StepId::CLAUDE_LOGIN];
    needs.extend(job.repos.iter().map(|r| StepId::clone_repo(&r.github)));
    SetupStep::new(
        StepId::WORKER_JSON,
        "worker.json written",
        needs,
        Cadence::EveryRefresh,
        Probe::remote(PROBE, check),
        StepFix::Run(FixPlan::new(display(machine, job, all), false, write)),
    )
}

fn check(out: &RemoteOutput, ctx: &StepContext) -> CheckResult {
    let machine = &ctx.machine;
    let Some(job) = machine.sd_worker() else {
        return CheckResult::cant_tell("no sd worker job");
    };
    let owned = match owned(machine, job, &ctx.all) {
        Ok(o) => o,
        Err(why) => return CheckResult::fail(why),
    };
    if out.text.trim().is_empty() {
        return CheckResult::fail("missing");
    }
    if parse(&out.text).is_none() {
        return CheckResult::fail("not valid JSON");
    }
    let differ = differences(Some(&out.text), &owned);
    if differ.is_empty() {
        CheckResult::pass("current")
    } else {
        CheckResult::fail(format!("differs: {}", differ.join(", ")))
    }
}

fn display(machine: &Machine, job: &SdWorkerSettings, all: &[Machine]) -> String {
    let head = format!(
        "Read {} (head -c {READ_CAP}), keep the keys Notchling doesn't own, then write:\n{}",
        shell::WORKER_JSON,
        shell::write_file(shell::WORKER_JSON, "600")
    );
    match owned(machine, job, all) {
        Err(why) => format!("{head}\n(can't be written yet: {why})"),
        Ok(owned) => {
            let data = super::registry::pretty_sorted(Value::Object(owned));
            format!(
                "{head}\nwith these keys on stdin:\n{}",
                String::from_utf8_lossy(&data).trim_end_matches('\n')
            )
        }
    }
}

fn why(r: &CommandResult, ctx: &FixContext) -> String {
    first_line_300(&r.err, &|s| ctx.mask(s))
}

fn write(ctx: &FixContext) -> FixOutcome {
    let machine = &ctx.step.machine;
    let Some(job) = machine.sd_worker() else {
        return FixOutcome::fail("this machine has no sd worker job");
    };
    let owned = match owned(machine, job, &ctx.step.all) {
        Ok(o) => o,
        Err(why) => return FixOutcome::fail(why),
    };
    let read = run::script(ctx, READ, None, 60);
    let missing = read.out.is_empty() && read.err.is_empty() && (read.code == 0 || read.code == 1);
    if !(missing || (read.code == 0 && read.err.is_empty())) {
        let said = why(&read, ctx);
        let tail = if said.is_empty() {
            String::new()
        } else {
            format!(": {said}")
        };
        return FixOutcome::fail(format!(
            "couldn't read worker.json (exit {}){tail}",
            read.code
        ));
    }
    if !read.out.trim().is_empty() && parse(&read.out).is_none() {
        return FixOutcome::fail(
            "worker.json on the machine isn't a JSON object; fix or remove it first",
        );
    }
    let data = merged(Some(&read.out), &owned);
    let wrote = run::script(
        ctx,
        &shell::write_file(shell::WORKER_JSON, "600"),
        Some(data),
        60,
    );
    if wrote.code == 0 {
        FixOutcome::ok("worker.json written")
    } else {
        FixOutcome::fail(format!("couldn't write worker.json: {}", why(&wrote, ctx)))
    }
}
