use super::registry::{DockerState, MachineAbilities, MachineOs, MachineRepo, SdWorkerSettings};
use super::shell::is_safe_machine_path;
use super::step::CheckResult;
use super::usage::SecretUsage;
use crate::vault::{expiry, VaultEntry};

pub const NAME_PROBLEM: &str = "Name: lowercase letters, digits and hyphens";
pub const HOST_PROBLEM: &str = "Host: a Tailscale name or an address";
pub const USER_PROBLEM: &str = "User: lowercase letters, digits, hyphens and underscores";
pub const DATED_PROBLEM: &str =
    "A machine name can't end in -<8 digits>; that's how replaced keys are titled";

pub fn machine_problems(name: &str, host: &str, user: &str, existing: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    if !is_name(name) {
        out.push(NAME_PROBLEM.to_string());
    } else if existing.iter().any(|e| e == name) {
        out.push(format!("{name} is already a machine"));
    } else if ends_in_replaced_key_date(name) {
        out.push(DATED_PROBLEM.to_string());
    }
    if !is_host(host) {
        out.push(HOST_PROBLEM.to_string());
    }
    if !is_user(user) {
        out.push(USER_PROBLEM.to_string());
    }
    out
}

pub fn ends_in_replaced_key_date(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() > 9 && b[b.len() - 9] == b'-' && b[b.len() - 8..].iter().all(u8::is_ascii_digit)
}

fn lower_or_digit(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}

fn is_name(s: &str) -> bool {
    let Some(first) = s.chars().next() else {
        return false;
    };
    (1..=40).contains(&s.len())
        && lower_or_digit(first)
        && s.chars().all(|c| lower_or_digit(c) || c == '-')
}

fn is_host(s: &str) -> bool {
    let Some(first) = s.chars().next() else {
        return false;
    };
    if s.len() > 253 {
        return false;
    }
    if first.is_ascii_alphanumeric()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return true;
    }
    s.contains(':')
        && s.chars()
            .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
        && s.parse::<std::net::Ipv6Addr>().is_ok()
}

fn is_user(s: &str) -> bool {
    let Some(first) = s.chars().next() else {
        return false;
    };
    s.len() <= 32
        && (first.is_ascii_lowercase() || first == '_')
        && s.chars().all(|c| lower_or_digit(c) || c == '_' || c == '-')
}

pub fn abilities_line(a: Option<&MachineAbilities>, os: Option<MachineOs>) -> String {
    let mut parts = Vec::new();
    let version = a
        .and_then(|a| a.os_version.clone())
        .filter(|v| !v.is_empty());
    match os {
        Some(MachineOs::MacOs) => {
            parts.push(version.map_or("macOS".into(), |v| format!("macOS {v}")))
        }
        Some(MachineOs::Linux) => parts.push(version.unwrap_or_else(|| "Linux".into())),
        None => parts.extend(version),
    }
    if let Some(a) = a {
        if let Some(arch) = a.arch.as_ref().filter(|x| !x.is_empty()) {
            parts.push(arch.clone());
        }
        if let Some(x) = a.xcode.as_ref().filter(|x| !x.is_empty()) {
            parts.push(format!("Xcode {x}"));
        }
        match a.simulators.len() {
            0 => {}
            1 => parts.push("1 simulator".into()),
            n => parts.push(format!("{n} simulators")),
        }
        if let Some(d) = a.docker {
            parts.push(
                match d {
                    DockerState::None => "no Docker",
                    DockerState::Stopped => "Docker stopped",
                    DockerState::Rootful => "Docker rootful",
                    DockerState::Rootless => "Docker rootless",
                }
                .into(),
            );
        }
        if let Some(gb) = a.free_disk_gb {
            parts.push(format!("{gb} GB free"));
        }
    }
    if parts.is_empty() {
        "not checked yet".into()
    } else {
        parts.join(" · ")
    }
}

pub fn update_offer_title(count: usize) -> String {
    format!(
        "Update {count} machine{}",
        if count == 1 { "" } else { "s" }
    )
}

pub fn secret_row(
    usage: &SecretUsage,
    entry: Option<&VaultEntry>,
    facts: Option<&CheckResult>,
    today: i64,
) -> String {
    let mut parts = vec![format!("{} → {}", usage.name, usage.remote_path)];
    if let Some(f) = facts {
        parts.push(f.state.text().to_string());
    }
    match entry {
        Some(e) => {
            if let Some(day) = e.expires.as_ref().filter(|d| !d.is_empty()) {
                parts.push(match expiry::status(day, today) {
                    Some(expiry::Status::Expired) => format!("expired {day}"),
                    Some(expiry::Status::Today) => "expires today".into(),
                    Some(expiry::Status::InDays(_)) => {
                        format!(
                            "expires {}",
                            expiry::phrase(day, today).unwrap_or_else(|| day.clone())
                        )
                    }
                    None => format!("expires {day}"),
                });
            }
        }
        None => parts.push("not in the vault".into()),
    }
    parts.join(" · ")
}

pub fn unreadable_note(reason: &str) -> String {
    format!("machines.json can't be read: {reason}. Notchling won't change it. Fix or move it, then Check again.")
}

pub fn migrated_note(backup: &str) -> String {
    format!("machines.json was upgraded to the new format. The old file is kept at {backup}.")
}

pub fn rename_note(old_names: &[String]) -> Option<String> {
    let (last, rest) = old_names.split_last()?;
    let list = if rest.is_empty() {
        last.clone()
    } else {
        format!("{} and {last}", rest.join(", "))
    };
    Some(format!(
        "Rename {list} to the new name{}",
        if old_names.len() == 1 { "" } else { "s" }
    ))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobRepoDraft {
    pub github: String,
    pub local_path: String,
    pub check_command: String,
    pub machine_path: String,
    pub verify_skill: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobDraft {
    pub machine: String,
    pub repos: Vec<JobRepoDraft>,
    pub kinds: Vec<String>,
    pub health_url: String,
    pub token_expires: Option<String>,
    pub token_all_repos: bool,
    pub is_new: bool,
    pub base: Option<SdWorkerSettings>,
}

pub const KIND_CHOICES: [&str; 2] = ["ios", "backend"];

impl JobDraft {
    pub fn new(machine: &str) -> JobDraft {
        JobDraft {
            machine: machine.into(),
            is_new: true,
            ..JobDraft::default()
        }
    }

    pub fn of(machine: &super::registry::Machine) -> JobDraft {
        let mut d = JobDraft::new(&machine.name);
        let Some(job) = machine.sd_worker() else {
            return d;
        };
        d.is_new = false;
        d.base = Some(job.clone());
        d.repos = job
            .repos
            .iter()
            .map(|r| JobRepoDraft {
                github: r.github.clone(),
                local_path: r.local_path.clone(),
                check_command: r.check_command.clone(),
                machine_path: r.machine_path.clone().unwrap_or_default(),
                verify_skill: r.verify_skill.clone().unwrap_or_default(),
            })
            .collect();
        d.kinds = job.kinds.clone();
        d.health_url = job.health_url.clone().unwrap_or_default();
        d.token_expires = job.token_expires.clone();
        d.token_all_repos = job.token_all_repos == Some(true);
        d
    }
}

pub fn job_problems(d: &JobDraft) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |s: String| {
        if !out.contains(&s) {
            out.push(s);
        }
    };
    if d.repos.is_empty() {
        add("Add at least one repository".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for r in &d.repos {
        let github = r.github.trim();
        if !is_repo(github) {
            add("Repository: owner/name, like me/app".into());
        } else if !seen.insert(github.to_lowercase()) {
            add(format!("{github} is listed twice"));
        }
        if !r.local_path.trim().starts_with('/') {
            add("Checkout on this Mac: an absolute path".into());
        }
        if r.check_command.trim().is_empty() {
            add("Check command: required".into());
        }
        let mp = r.machine_path.trim();
        if !mp.is_empty() && !is_machine_path(mp) {
            add("Machine path: ~/ or an absolute path with plain characters".into());
        }
        let skill = r.verify_skill.trim();
        if !skill.is_empty()
            && (skill.starts_with('/') || !is_safe_machine_path(&format!("/{skill}")))
        {
            add("Verify skill: a path inside the checkout".into());
        }
    }
    let health = d.health_url.trim();
    if !health.is_empty() && !is_health_url(health) {
        add("Health URL: http or https".into());
    }
    out
}

pub fn job_settings(d: &JobDraft, base: Option<&SdWorkerSettings>) -> SdWorkerSettings {
    let repos = d
        .repos
        .iter()
        .map(|r| {
            let mp = r.machine_path.trim();
            let skill = r.verify_skill.trim();
            MachineRepo {
                github: r.github.trim().into(),
                local_path: r.local_path.trim().into(),
                check_command: r.check_command.trim().into(),
                machine_path: (!mp.is_empty()).then(|| mp.into()),
                verify_skill: (!skill.is_empty()).then(|| skill.into()),
            }
        })
        .collect();
    let health = d.health_url.trim();
    let mut s = SdWorkerSettings::new(&d.machine);
    if let Some(b) = base {
        s.python = b.python.clone();
        s.sd_path = b.sd_path.clone();
        s.token_expires = b.token_expires.clone();
        if !b.issues_secret.is_empty() {
            s.issues_secret = b.issues_secret.clone();
        }
    }
    s.repos = repos;
    s.kinds = d.kinds.clone();
    s.health_url = (!health.is_empty()).then(|| health.into());
    s.token_all_repos = d.token_all_repos.then_some(true);
    s
}

pub fn is_repo(s: &str) -> bool {
    let parts: Vec<&str> = s.split('/').collect();
    parts.len() == 2
        && s.len() <= 140
        && parts.iter().all(|p| {
            p.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c))
        })
}

fn is_machine_path(p: &str) -> bool {
    match p.strip_prefix('~') {
        Some(rest) if rest.starts_with('/') => is_safe_machine_path(rest),
        _ => is_safe_machine_path(p),
    }
}

fn is_health_url(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    let Some(rest) = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
    else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or("");
    !host.is_empty() && s.len() <= 300 && !s.chars().any(|c| " \t\n\r\"'`$\\;<>|&(){}".contains(c))
}
