use super::forms::machine_problems;
use super::registry::{Machine, MachineOs};
use super::runner::{Call, CommandResult};
use super::shell;
use super::step::{
    Cadence, CheckResult, Detected, FixContext, FixOutcome, FixPlan, Probe, RemoteOutput,
    SetupStep, StepContext, StepFix, StepId,
};
use super::steps_machine::{cut, is_newline, trim_ws};
use super::steps_node_binary::binary;
use super::steps_node_mac::app_closed;
use super::steps_node_reach::{is_tailscale_ip, reach};
use super::steps_node_service::service;
use super::steps_sd::first_line_300;
use super::steps_tailscale_linux::tagged;
use crate::cluster::persist::CLUSTER_DIR;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const MAC_BUILD: &str = concat!("notchling ", env!("CARGO_PKG_VERSION"));

pub(super) const BINARY: &str = "~/.local/bin/notchling";
pub(super) const BINARY_NEW: &str = "~/.local/bin/notchling.new";
pub(super) const APP_DIR: &str = "/Applications/Notchling.app";
pub(super) const UNIT: &str = "notchlingd.service";
pub(super) const RUNTIME_DIR: &str = r#"export XDG_RUNTIME_DIR="/run/user/$(id -u)""#;

const TAILSCALE_LINUX: &str = "tailscale status --json --peers=false";
const TAILSCALE_MAC: &str = r#"t=tailscale; command -v tailscale >/dev/null 2>&1 || t=/Applications/Tailscale.app/Contents/MacOS/Tailscale; "$t" status --json --peers=false"#;

pub fn dist_dir(data: &Path) -> PathBuf {
    data.join(CLUSTER_DIR).join("dist")
}

pub(super) fn steps(m: &Machine) -> Vec<SetupStep> {
    let mut out = vec![tailscale(m), binary(m), service(m)];
    if m.runs_app() {
        out.push(app_closed(m));
    }
    out.push(reach(m));
    out.push(joined(m));
    out
}

pub fn is_sd_join_step(id: &StepId) -> bool {
    *id != StepId::NODE_GIT
        && (*id == StepId::REACHABLE
            || *id == StepId::WORKER_USER
            || *id == StepId::STAYS_AWAKE
            || *id == StepId::FILEVAULT
            || id.raw().starts_with("node-")
            || super::steps_master::is_master_step(id))
}

fn tailscale(m: &Machine) -> SetupStep {
    SetupStep::new(
        StepId::NODE_TAILSCALE,
        "Tailscale connected",
        vec![StepId::REACHABLE],
        Cadence::EveryRefresh,
        Probe::remote(
            if m.is_linux() {
                TAILSCALE_LINUX
            } else {
                TAILSCALE_MAC
            },
            parse_tailscale,
        ),
        StepFix::HowTo(format!(
            "Install Tailscale on {} and run sudo tailscale up there, in your own terminal.",
            m.name
        )),
    )
}

fn joined(m: &Machine) -> SetupStep {
    let mut needs = vec![
        StepId::NODE_SERVICE,
        StepId::NODE_TAILSCALE,
        StepId::NODE_REACH,
    ];
    if m.runs_app() {
        needs.insert(1, StepId::NODE_APP_CLOSED);
    }
    SetupStep::new(
        StepId::NODE_JOINED,
        "Joined the cluster",
        needs,
        Cadence::EveryRefresh,
        Probe::remote(
            format!("{} cluster status --json", shell::path(BINARY)),
            parse_joined,
        ),
        StepFix::Run(FixPlan::new(join_display(), false, join_fix)),
    )
}

pub(super) fn json_from(text: &str) -> Option<Value> {
    let start = text.find('{')?;
    serde_json::Deserializer::from_str(&text[start..])
        .into_iter::<Value>()
        .next()?
        .ok()
}

pub(super) fn machine_refusal(m: &Machine) -> Option<String> {
    machine_problems(&m.name, &m.host, &m.user, &[])
        .into_iter()
        .next()
        .map(|why| format!("won't run on {}: {why}", m.name))
}

pub(super) fn first_text_line(text: &str) -> String {
    text.split(is_newline)
        .map(trim_ws)
        .find(|l| !l.is_empty())
        .map(cut)
        .unwrap_or_default()
}

fn parse_tailscale(out: &RemoteOutput, ctx: &StepContext) -> CheckResult {
    let Some(v) = json_from(&out.text) else {
        let linux = ctx.machine.is_linux();
        if out.code == 126 || out.code == 127 {
            let missing = CheckResult::fail("Tailscale isn't installed there");
            return tagged(missing, linux, out.code, None);
        }
        let why = first_text_line(&out.text);
        return tagged(
            CheckResult::fail(if why.is_empty() {
                "Tailscale isn't running there".to_string()
            } else {
                format!("Tailscale isn't running there: {why}")
            }),
            linux,
            out.code,
            None,
        );
    };
    let state = v["BackendState"].as_str().unwrap_or("");
    if state != "Running" {
        let stopped = CheckResult::fail(format!(
            "Tailscale is {}",
            if state.is_empty() {
                "not running".to_string()
            } else {
                cut(state)
            }
        ));
        return tagged(stopped, ctx.machine.is_linux(), out.code, Some(state));
    }
    let address = v["Self"]["TailscaleIPs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find(|ip| is_tailscale_ip(ip));
    let Some(ip) = address else {
        return CheckResult::fail("Tailscale is running but has no 100.x address");
    };
    let local = ctx.tokens.tailnet();
    let theirs = v["CurrentTailnet"]["Name"].as_str();
    if let (Some(local), Some(theirs)) = (local, theirs) {
        if !theirs.trim().eq_ignore_ascii_case(local.trim()) {
            return CheckResult::fail(format!(
                "in the tailnet {}, but this Mac is in {}",
                plain(theirs),
                plain(&local)
            ));
        }
    }
    CheckResult::pass(format!("running, {}", cut(ip)))
}

fn plain(text: &str) -> String {
    cut(&text.chars().filter(|c| !c.is_control()).collect::<String>())
}

pub(super) fn value_of<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.split(is_newline)
        .find_map(|l| l.strip_prefix(key))
        .map(trim_ws)
}

fn is_node_id(text: &str) -> bool {
    (16..=64).contains(&text.len())
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

fn parse_joined(out: &RemoteOutput, ctx: &StepContext) -> CheckResult {
    let want = match ctx.tokens.cluster_id() {
        Err(_) => return CheckResult::cant_tell("Notchling isn't running on this Mac"),
        Ok(None) => {
            return CheckResult::fail(
                "This Mac has no cluster yet. Make one first, then join this machine to it.",
            )
        }
        Ok(Some(id)) => id,
    };
    let Some(v) = json_from(&out.text) else {
        let why = first_text_line(&out.text);
        return CheckResult::fail(if why.is_empty() {
            "the Notchling service there didn't answer".to_string()
        } else {
            format!("the Notchling service there didn't answer: {why}")
        });
    };
    let node = v["me"]
        .as_str()
        .filter(|id| is_node_id(id))
        .map(str::to_string);
    let seen = |result: CheckResult| {
        result.detected(Detected {
            node: node.clone(),
            ..Detected::default()
        })
    };
    let Some(theirs) = v["cluster"]["id"].as_str() else {
        return seen(match v["joining"].as_object() {
            Some(joining) if joining.get("answered").and_then(Value::as_bool) == Some(true) => {
                CheckResult::cant_tell("waiting for approval on this Mac")
            }
            Some(_) => CheckResult::cant_tell("joining; this Mac hasn't answered yet"),
            None => CheckResult::fail("not in a cluster yet"),
        });
    };
    if theirs != want {
        let name = v["cluster"]["name"].as_str().unwrap_or("");
        return seen(CheckResult::fail(if name.is_empty() {
            "in another cluster".to_string()
        } else {
            format!("in another cluster, {}", cut(name))
        }));
    }
    let name = v["cluster"]["name"].as_str().unwrap_or("");
    seen(CheckResult::pass(if name.is_empty() {
        "in this Mac's cluster".to_string()
    } else {
        format!("in {}", cut(name))
    }))
}

pub(super) fn os_word(m: &Machine) -> Option<&'static str> {
    match m.os? {
        MachineOs::MacOs => Some("macos"),
        MachineOs::Linux => Some("linux"),
    }
}

pub(super) fn arch_word(m: &Machine) -> Option<String> {
    let raw = m.abilities.as_ref()?.arch.as_deref()?;
    let arch = if raw == "arm64" { "aarch64" } else { raw };
    let plain = !arch.is_empty()
        && arch.len() <= 16
        && arch
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    plain.then(|| arch.to_string())
}

pub(super) fn said(ctx: &FixContext, r: &CommandResult) -> String {
    let text = if r.err.trim().is_empty() {
        &r.out
    } else {
        &r.err
    };
    first_line_300(text, &|s| ctx.mask(s))
}

fn join_script() -> String {
    format!("{} cluster join --token-stdin", shell::path(BINARY))
}

fn join_display() -> String {
    format!(
        "Send this Mac's join token on stdin; it is never shown or put on a command line:\n{}",
        join_script()
    )
}

const ASK_APP: &str = "Use Add Machine or Join Cluster in the app's Cluster sheet.";

fn is_me(text: &str) -> bool {
    text.len() == 16
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

fn read_me(ctx: &FixContext) -> Result<String, String> {
    let script = format!("{} cluster status --json", shell::path(BINARY));
    let ran = ctx.step.runner.run(&Call::secs(
        shell::ssh_script(&ctx.step.machine, &script),
        30,
    ));
    if ran.code != 0 {
        return Err(format!(
            "couldn't read the node id there (exit {}): {}",
            ran.code,
            said(ctx, &ran)
        ));
    }
    json_from(&ran.out)
        .and_then(|v| v["me"].as_str().map(str::to_string))
        .filter(|me| is_me(me))
        .ok_or_else(|| "refused: the machine gave no usable node id".to_string())
}

fn token_for(ctx: &FixContext, me: &str) -> Result<(String, bool), String> {
    let m = &ctx.step.machine;
    let choices = m.node.as_ref().map(|n| n.choices).unwrap_or_default();
    let tokens = &ctx.step.tokens;
    match tokens.invite(me, &m.name, choices.can_be_master, choices.acting) {
        Ok(Some(token)) if !token.trim().is_empty() => Ok((token.trim().to_string(), true)),
        Ok(Some(_)) => Err("this Mac's cluster gave an empty invite".to_string()),
        Err(why) => Err(ctx.mask(&why)),
        Ok(None) => match tokens.join_token() {
            Ok(token) if !token.trim().is_empty() => Ok((token.trim().to_string(), false)),
            Ok(_) => Err("this Mac's cluster gave an empty join token".to_string()),
            Err(why) => Err(format!(
                "{}. {ASK_APP}",
                ctx.mask(&why).trim().trim_end_matches('.')
            )),
        },
    }
}

fn token_expired(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("expired") || text.contains("bad token")
}

fn join_fix(ctx: &FixContext) -> FixOutcome {
    if let Some(why) = machine_refusal(&ctx.step.machine) {
        return FixOutcome::fail(why);
    }
    let me = match read_me(ctx) {
        Ok(me) => me,
        Err(why) => return FixOutcome::fail(why),
    };
    let mut masked = ctx.clone();
    let mut retried = false;
    loop {
        let (token, invited) = match token_for(&masked, &me) {
            Ok(got) => got,
            Err(why) => return FixOutcome::fail(why),
        };
        masked.masks.hide("join-token", &token);
        masked.step.masks.hide("join-token", &token);
        let call = Call::secs(shell::ssh_script(&ctx.step.machine, &join_script()), 90)
            .stdin(format!("{token}\n").into_bytes());
        if masked.masks.leaks_into(&call.argv.join("\n")) {
            return FixOutcome::fail("refused: the token would have gone on a command line");
        }
        let ran = ctx.step.runner.run(&call);
        if ran.code == 0 {
            let line = first_line_300(&ran.out, &|s| masked.mask(s));
            return FixOutcome::ok(if line.is_empty() {
                "asked to join".to_string()
            } else {
                line
            });
        }
        if invited && !retried && token_expired(&format!("{}\n{}", ran.out, ran.err)) {
            retried = true;
            continue;
        }
        return FixOutcome::fail(format!(
            "couldn't join (exit {}): {}",
            ran.code,
            said(&masked, &ran)
        ));
    }
}
