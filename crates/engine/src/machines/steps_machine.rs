use super::process::CUT_NOTICE;
use super::registry::{DockerState, Machine, MachineOs};
use super::runner::CommandResult;
use super::shell;
use super::step::{
    run, Cadence, CheckResult, Detected, FixContext, FixOutcome, FixPlan, Probe, RemoteOutput,
    SetupStep, StepContext, StepFix, StepId, SubRow,
};
use super::steps_xcode::{parse_xcode, XCODE_SCRIPT};
use std::collections::BTreeMap;

pub fn all(m: &Machine) -> Vec<SetupStep> {
    vec![
        SetupStep::new(
            StepId::REACHABLE,
            "Reachable",
            vec![],
            Cadence::EveryRefresh,
            Probe::remote(REACHABLE_SCRIPT, parse_reachable),
            StepFix::HowTo(format!("Turn the machine on and check it from this Mac with `tailscale ping {}`. For a new Linux user, add the user to the Tailscale ACL's ssh users. Then check again.", m.host)),
        ),
        SetupStep::new(
            StepId::WORKER_USER,
            "Worker user",
            vec![StepId::REACHABLE],
            Cadence::Slow,
            Probe::remote(WORKER_USER_SCRIPT, parse_worker_user),
            StepFix::HowTo("Use the worker account (agent on vps-sing-1). Root is only for steps marked needs admin. On Linux the worker account must not be able to use sudo: remove it from the sudo group (admin steps run as root over SSH). Then check again.".into()),
        ),
        SetupStep::new(
            StepId::STAYS_AWAKE,
            "Stays awake",
            vec![StepId::REACHABLE],
            Cadence::EveryRefresh,
            Probe::remote("pmset -g", parse_stays_awake),
            StepFix::Run(stays_awake_fix()),
        ),
        SetupStep::new(
            StepId::FILEVAULT,
            "FileVault",
            vec![StepId::REACHABLE],
            Cadence::EveryRefresh,
            Probe::remote("fdesetup status", parse_file_vault),
            StepFix::HowTo("Notchling only reads this. Turn FileVault on or off in System Settings on the machine. With it on, the machine waits for an unlock after a restart. Then check again.".into()),
        ),
        SetupStep::new(
            StepId::BASE_TOOLS,
            "Base tools",
            vec![StepId::REACHABLE],
            Cadence::EveryRefresh,
            Probe::remote(BASE_TOOLS_SCRIPT, parse_base_tools),
            StepFix::Run(base_tools_fix(m.os)),
        ),
        SetupStep::new(
            StepId::CLAUDE_CODE,
            "Claude Code",
            vec![StepId::REACHABLE],
            Cadence::EveryRefresh,
            Probe::remote(CLAUDE_SCRIPT, parse_claude_code),
            StepFix::Run(claude_code_fix()),
        ),
        SetupStep::new(
            StepId::XCODE,
            "Xcode",
            vec![StepId::REACHABLE],
            Cadence::EveryRefresh,
            Probe::remote(XCODE_SCRIPT, parse_xcode),
            StepFix::HowTo("Notchling doesn't install Xcode, which needs your Apple ID. On the machine: App Store, open it once, Xcode > Settings > Components for the iOS runtime, then Check again.".into()),
        ),
    ]
}

const CUT_LENGTH: usize = 200;

pub(super) fn cut(s: &str) -> String {
    s.chars().take(CUT_LENGTH).collect()
}

pub(super) fn shown(s: &str, mask: &dyn Fn(&str) -> String) -> String {
    cut(&mask(s))
}

pub(super) fn is_newline(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

pub(super) fn trim_ws(s: &str) -> &str {
    s.trim_matches(|c: char| c == '\t' || (c.is_whitespace() && !is_newline(c)))
}

fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split(is_newline).filter(|l| !l.is_empty())
}

fn keyed(text: &str, keys: &[&str]) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    for raw in lines(text) {
        let Some((key, value)) = raw.split_once('=') else {
            continue;
        };
        if keys.contains(&key) && !found.contains_key(key) {
            found.insert(key.to_string(), trim_ws(value).to_string());
        }
    }
    found
}

fn first_line(text: &str) -> String {
    lines(text)
        .map(trim_ws)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

fn outcome(r: &CommandResult, ok: &str, ctx: &FixContext) -> FixOutcome {
    if r.code == 0 {
        return FixOutcome::ok(ok);
    }
    let last_line = |output: &str| -> Option<String> {
        let masked = ctx.mask(output);
        let mut found: Vec<String> = lines(&masked)
            .map(|l| trim_ws(l).to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if output.starts_with(CUT_NOTICE) && found.len() > 1 {
            found.remove(1);
        }
        found.pop()
    };
    let last = last_line(&r.err)
        .or_else(|| last_line(&r.out))
        .unwrap_or_else(|| "no output".into());
    FixOutcome::fail(format!("failed (code {}): {}", r.code, cut(&last)))
}

const REACHABLE_SCRIPT: &str = r#"echo "os=$(uname -s)"
echo "arch=$(uname -m)"
echo "uid=$(id -u)"
echo "user=$(id -un)"
echo "home=$HOME"
if [ "$(uname -s)" = Darwin ]; then v=$(sw_vers -productVersion 2>/dev/null); else v=$(sed -n 's/^PRETTY_NAME=//p' /etc/os-release 2>/dev/null | head -1 | tr -d '"'); fi
echo "version=$v"
echo "disk_kb=$(df -Pk "$HOME" 2>/dev/null | awk 'NR==2 {print $4}')"
if command -v docker >/dev/null 2>&1; then if o=$(docker info --format '{{json .SecurityOptions}}' 2>/dev/null); then case "$o" in *rootless*) echo docker=rootless;; *) echo docker=rootful;; esac; else echo docker=stopped; fi; else echo docker=none; fi"#;

fn docker_state(text: &str) -> Option<DockerState> {
    match text {
        "none" => Some(DockerState::None),
        "stopped" => Some(DockerState::Stopped),
        "rootful" => Some(DockerState::Rootful),
        "rootless" => Some(DockerState::Rootless),
        _ => None,
    }
}

fn parse_reachable(out: &RemoteOutput, ctx: &StepContext) -> CheckResult {
    let text = ctx.mask(&out.text);
    let kv = keyed(
        &text,
        &["os", "arch", "home", "version", "disk_kb", "docker"],
    );
    let os_name = match kv.get("os") {
        Some(name) if !name.is_empty() => name.as_str(),
        _ => {
            if out.code != 0 {
                let why = first_line(&text);
                return CheckResult::fail(if why.is_empty() {
                    "can't reach the machine".to_string()
                } else {
                    format!("can't reach the machine: {}", cut(&why))
                });
            }
            return CheckResult::cant_tell("the machine gave no answer");
        }
    };
    let os = match os_name {
        "Darwin" => MachineOs::MacOs,
        "Linux" => MachineOs::Linux,
        other => return CheckResult::fail(format!("unsupported OS {}", cut(other))),
    };
    let mut detected = Detected {
        os: Some(os),
        ..Detected::default()
    };
    let home = match kv.get("home") {
        Some(home) if shell::is_safe_machine_path(home) => home.clone(),
        _ => {
            return CheckResult::fail(
                "the machine's home folder is missing or has characters Notchling won't use",
            )
            .detected(detected)
        }
    };
    detected.home = Some(home.clone());

    let mut abilities = ctx.machine.abilities.clone().unwrap_or_default();
    abilities.home = Some(home);
    let arch = kv.get("arch").filter(|s| !s.is_empty()).map(|s| cut(s));
    let version = kv.get("version").filter(|s| !s.is_empty()).map(|s| cut(s));
    if let Some(arch) = &arch {
        abilities.arch = Some(arch.clone());
    }
    if let Some(version) = &version {
        abilities.os_version = Some(version.clone());
    }
    if let Some(docker) = kv.get("docker").and_then(|d| docker_state(d)) {
        abilities.docker = Some(docker);
    }
    if let Some(kb) = kv.get("disk_kb").and_then(|k| k.parse::<i64>().ok()) {
        if kb >= 0 {
            abilities.free_disk_gb = Some(kb / 1_048_576);
        }
    }
    detected.abilities = Some(abilities);

    let label = if os == MachineOs::MacOs {
        version
            .as_ref()
            .map_or("macOS".to_string(), |v| format!("macOS {v}"))
    } else {
        version.clone().unwrap_or_else(|| "Linux".into())
    };
    CheckResult::pass(arch.map_or(label.clone(), |a| format!("{label}, {a}"))).detected(detected)
}

const WORKER_USER_SCRIPT: &str = r#"id -u
echo "os=$(uname -s)"
echo "groups=$(id -nG 2>/dev/null)"
if [ "$(uname -s)" = Linux ]; then sudo -n -l >/dev/null 2>&1; echo "sudo_l=$?"; fi"#;

const SUDO_GROUPS: [&str; 3] = ["sudo", "wheel", "admin"];

fn parse_worker_user(out: &RemoteOutput, _ctx: &StepContext) -> CheckResult {
    let uid = lines(&out.text)
        .map(trim_ws)
        .find(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_digit()));
    let Some(number) = uid.and_then(|u| u.parse::<i64>().ok()) else {
        return CheckResult::cant_tell("the machine did not say who this is");
    };
    if number == 0 {
        return CheckResult::fail(
            "signed in as root; use the worker account. Root is only for steps marked needs admin",
        );
    }
    let kv = keyed(&out.text, &["os", "groups", "sudo_l"]);
    if kv.get("os").map(String::as_str) == Some("Linux") {
        let groups = kv.get("groups").map(String::as_str).unwrap_or("");
        let in_sudo_group = groups.split([' ', '\t']).any(|g| SUDO_GROUPS.contains(&g));
        if in_sudo_group || kv.get("sudo_l").map(String::as_str) == Some("0") {
            return CheckResult::fail(
                "the worker account can use sudo; remove it from the sudo group",
            );
        }
    }
    CheckResult::pass(format!("user id {number}, not root"))
}

fn parse_stays_awake(out: &RemoteOutput, _ctx: &StepContext) -> CheckResult {
    const WANTED: [(&str, i64); 3] = [("sleep", 0), ("disksleep", 0), ("autorestart", 1)];
    let mut seen: BTreeMap<&str, i64> = BTreeMap::new();
    for raw in lines(&out.text) {
        let words: Vec<&str> = raw.split([' ', '\t']).filter(|w| !w.is_empty()).collect();
        if words.len() < 2 {
            continue;
        }
        let Some((key, _)) = WANTED.iter().find(|(k, _)| *k == words[0]) else {
            continue;
        };
        if seen.contains_key(key) {
            continue;
        }
        if let Ok(value) = words[1].parse::<i64>() {
            seen.insert(key, value);
        }
    }
    let missing: Vec<&str> = WANTED
        .iter()
        .filter(|(k, _)| !seen.contains_key(k))
        .map(|(k, _)| *k)
        .collect();
    if !missing.is_empty() {
        return CheckResult::cant_tell(format!("pmset did not report {}", missing.join(", ")));
    }
    let wrong: Vec<String> = WANTED
        .iter()
        .filter(|(k, v)| seen.get(k) != Some(v))
        .map(|(k, v)| format!("{k} {} (should be {v})", seen[k]))
        .collect();
    if wrong.is_empty() {
        return CheckResult::pass(
            WANTED
                .iter()
                .map(|(k, v)| format!("{k} {v}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    CheckResult::fail(wrong.join(", "))
}

const STAYS_AWAKE_SCRIPT: &str = "pmset -a sleep 0 disksleep 0 && pmset -a autorestart 1";

fn stays_awake_fix() -> FixPlan {
    FixPlan::new(format!("as admin: {STAYS_AWAKE_SCRIPT}"), true, |ctx| {
        let r = run::admin(ctx, STAYS_AWAKE_SCRIPT, 900);
        outcome(&r, "sleep 0, disksleep 0, autorestart 1 set", ctx)
    })
}

fn parse_file_vault(out: &RemoteOutput, ctx: &StepContext) -> CheckResult {
    if out.text.contains("FileVault is On") {
        return CheckResult::pass("on: after a restart it waits for an unlock").detected(
            Detected {
                file_vault: Some(true),
                ..Detected::default()
            },
        );
    }
    if out.text.contains("FileVault is Off") {
        return CheckResult::pass("off").detected(Detected {
            file_vault: Some(false),
            ..Detected::default()
        });
    }
    let why = first_line(&ctx.mask(&out.text));
    CheckResult::cant_tell(if why.is_empty() {
        "fdesetup said nothing".to_string()
    } else {
        cut(&why)
    })
}

const BASE_TOOL_NAMES: [&str; 4] = ["git", "python3", "jq", "gh"];

const BASE_TOOLS_SCRIPT: &str = r#"echo "git=$(command -v git)"
echo "python3=$(command -v python3)"
echo "jq=$(command -v jq)"
echo "gh=$(command -v gh)"
if [ "$(uname -s)" = Darwin ] && ! xcode-select -p >/dev/null 2>&1; then echo clt=missing; pv=; else pv=$(python3 -c 'import sys; print("%d.%d" % sys.version_info[:2])' 2>/dev/null); fi
echo "pyver=$pv"
if command -v brew >/dev/null 2>&1; then echo brew=yes; else echo brew=no; fi"#;

fn major_minor(s: &str) -> Option<(i64, i64)> {
    let parts: Vec<&str> = s.split('.').collect();
    let [major, minor] = parts.as_slice() else {
        return None;
    };
    Some((major.parse().ok()?, minor.parse().ok()?))
}

fn base_tool_rows(kv: &BTreeMap<String, String>, ctx: &StepContext) -> Vec<SubRow> {
    BASE_TOOL_NAMES
        .iter()
        .map(|name| {
            let path = kv.get(*name).map(String::as_str).unwrap_or("");
            SubRow {
                name: name.to_string(),
                ok: !path.is_empty(),
                detail: if path.is_empty() {
                    "missing".into()
                } else {
                    shown(path, &|s| ctx.mask(s))
                },
            }
        })
        .collect()
}

fn parse_base_tools(out: &RemoteOutput, ctx: &StepContext) -> CheckResult {
    let kv = keyed(
        &out.text,
        &["git", "python3", "jq", "gh", "pyver", "brew", "clt"],
    );
    let has = |name: &str| kv.get(name).is_some_and(|v| !v.is_empty());
    if kv.get("clt").map(String::as_str) == Some("missing") {
        return CheckResult::fail("Command Line Tools missing").rows(base_tool_rows(&kv, ctx));
    }
    let mut problems: Vec<String> = Vec::new();
    let missing: Vec<&str> = BASE_TOOL_NAMES
        .iter()
        .filter(|n| !has(n))
        .copied()
        .collect();
    if !missing.is_empty() {
        problems.push(format!("missing: {}", missing.join(", ")));
    }

    let version = kv.get("pyver").and_then(|v| major_minor(v));
    let mut python_ok = false;
    if has("python3") {
        match version {
            Some(v) if v >= (3, 10) => python_ok = true,
            Some((major, minor)) => problems.push(format!(
                "python3 is {major}.{minor}, sd needs 3.10 or newer"
            )),
            None => problems.push("python3 does not run".into()),
        }
    }

    let mut detected = Detected::default();
    if python_ok {
        if let Some(path) = kv.get("python3") {
            if shell::is_safe_machine_path(path) {
                detected.python = Some(path.clone());
            }
        }
    }

    let rows = base_tool_rows(&kv, ctx);
    if !problems.is_empty() {
        return CheckResult::fail(problems.join("; "))
            .rows(rows)
            .detected(detected);
    }
    let python = version.map_or("python3".to_string(), |(a, b)| format!("python3 {a}.{b}"));
    CheckResult::pass(format!("git, {python}, jq, gh"))
        .rows(rows)
        .detected(detected)
}

const MAC_BASE_TOOLS_SCRIPT: &str = r##"xcode-select -p >/dev/null 2>&1 || { echo 'Command Line Tools are missing: run xcode-select --install in the machine'"'"'s own Terminal, then Check again.'; exit 1; }; command -v brew >/dev/null 2>&1 || { echo 'Homebrew is missing: install it from https://brew.sh in the machine'"'"'s own Terminal, then Check again.'; exit 1; }; brew install git python jq gh"##;

const LINUX_BASE_TOOLS_SCRIPT: &str = r#"set -e
export DEBIAN_FRONTEND=noninteractive NEEDRESTART_SUSPEND=1 NEEDRESTART_MODE=l
apt-get update
apt-get install -y git python3 jq wget --no-upgrade
mkdir -p -m 755 /etc/apt/keyrings
wget -nv -O /etc/apt/keyrings/githubcli-archive-keyring.gpg https://cli.github.com/packages/githubcli-archive-keyring.gpg
chmod go+r /etc/apt/keyrings/githubcli-archive-keyring.gpg
echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" > /etc/apt/sources.list.d/github-cli.list
apt-get update
apt-get install -y gh --no-upgrade"#;

const NOT_DETECTED: &str = "OS not detected yet; run Reachable first";

fn not_settled(now: Option<MachineOs>) -> String {
    if now.is_none() {
        NOT_DETECTED.into()
    } else {
        "this machine's OS changed since this step was built; check again first".into()
    }
}

fn base_tools_fix(os: Option<MachineOs>) -> FixPlan {
    match os {
        Some(MachineOs::Linux) => FixPlan::new(
            format!("as admin:\n{LINUX_BASE_TOOLS_SCRIPT}"),
            true,
            |ctx| {
                if ctx.step.machine.os != Some(MachineOs::Linux) {
                    return FixOutcome::fail(not_settled(ctx.step.machine.os));
                }
                let r = run::admin(ctx, LINUX_BASE_TOOLS_SCRIPT, 900);
                outcome(&r, "git, python3, jq, wget and gh installed", ctx)
            },
        ),
        Some(MachineOs::MacOs) => FixPlan::new(MAC_BASE_TOOLS_SCRIPT, false, |ctx| {
            if ctx.step.machine.os != Some(MachineOs::MacOs) {
                return FixOutcome::fail(not_settled(ctx.step.machine.os));
            }
            let r = run::script(ctx, MAC_BASE_TOOLS_SCRIPT, None, 600);
            outcome(&r, "git, python, jq and gh installed with Homebrew", ctx)
        }),
        None => FixPlan::new(NOT_DETECTED, false, |_| FixOutcome::fail(NOT_DETECTED)),
    }
}

const CLAUDE_SCRIPT: &str = "command -v claude >/dev/null 2>&1 && claude --version 2>/dev/null";

fn parse_claude_code(out: &RemoteOutput, ctx: &StepContext) -> CheckResult {
    let line = first_line(&ctx.mask(&out.text));
    if line.is_empty() || !line.chars().any(|c| c.is_numeric()) {
        return CheckResult::fail("not installed");
    }
    CheckResult::pass(cut(&line))
}

pub const CLAUDE_INSTALL: &str =
    r#"s=$(curl -fsSL https://claude.ai/install.sh) && printf '%s\n' "$s" | bash"#;

fn claude_code_fix() -> FixPlan {
    FixPlan::new(CLAUDE_INSTALL, false, |ctx| {
        let r = run::script(ctx, CLAUDE_INSTALL, None, 600);
        outcome(&r, "Claude Code installed", ctx)
    })
}
