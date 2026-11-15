use super::registry::{Machine, MachineOs};
use super::runner::{Call, CommandResult};
use super::shell;
use super::step::{
    run, Cadence, CheckResult, Detected, FixContext, FixOutcome, FixPlan, Probe, RemoteOutput,
    SetupStep, StepContext, StepFix, StepId,
};
use super::steps_machine::{cut, is_newline, trim_ws};
use super::steps_node_binary::binary;
use super::steps_sd::first_line_300;
use crate::cluster::persist::CLUSTER_DIR;
use crate::lanes::quote;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const MAC_BUILD: &str = concat!("notchling ", env!("CARGO_PKG_VERSION"));

pub(super) const BINARY: &str = "~/.local/bin/notchling";
pub(super) const BINARY_NEW: &str = "~/.local/bin/notchling.new";
pub(super) const UNIT: &str = "notchlingd.service";
pub(super) const LABEL: &str = "dev.notchling.daemon";
const PLIST: &str = "~/Library/LaunchAgents/dev.notchling.daemon.plist";
pub(super) const RUNTIME_DIR: &str = r#"export XDG_RUNTIME_DIR="/run/user/$(id -u)""#;

const TAILSCALE_SCRIPT: &str = r#"t=tailscale; command -v tailscale >/dev/null 2>&1 || t=/Applications/Tailscale.app/Contents/MacOS/Tailscale; "$t" status --json --peers=false"#;

pub fn dist_dir(data: &Path) -> PathBuf {
    data.join(CLUSTER_DIR).join("dist")
}

pub(super) fn steps(m: &Machine) -> Vec<SetupStep> {
    vec![tailscale(m), binary(m), service(m), joined()]
}

fn tailscale(m: &Machine) -> SetupStep {
    SetupStep::new(
        StepId::NODE_TAILSCALE,
        "Tailscale connected",
        vec![StepId::REACHABLE],
        Cadence::EveryRefresh,
        Probe::remote(TAILSCALE_SCRIPT, parse_tailscale),
        StepFix::HowTo(format!(
            "Install Tailscale on {} and run sudo tailscale up there, in your own terminal.",
            m.name
        )),
    )
}

fn service(m: &Machine) -> SetupStep {
    let linux = m.is_linux();
    let (script, parse): (String, fn(&RemoteOutput, &StepContext) -> CheckResult) = if linux {
        (linux_probe(), parse_service_linux)
    } else {
        (mac_probe(), parse_service_mac)
    };
    SetupStep::new(
        StepId::NODE_SERVICE,
        "Notchling service",
        vec![StepId::NODE_BINARY],
        Cadence::EveryRefresh,
        Probe::remote(script, parse),
        StepFix::Run(FixPlan::new(service_display(m), linux, service_fix)),
    )
}

fn joined() -> SetupStep {
    SetupStep::new(
        StepId::NODE_JOINED,
        "Joined the cluster",
        vec![StepId::NODE_SERVICE, StepId::NODE_TAILSCALE],
        Cadence::EveryRefresh,
        Probe::remote(
            format!("{} cluster status --json", shell::path(BINARY)),
            parse_joined,
        ),
        StepFix::Run(FixPlan::new(join_display(), false, join_fix)),
    )
}

fn json_from(text: &str) -> Option<Value> {
    let start = text.find('{')?;
    serde_json::Deserializer::from_str(&text[start..])
        .into_iter::<Value>()
        .next()?
        .ok()
}

pub(super) fn first_text_line(text: &str) -> String {
    text.split(is_newline)
        .map(trim_ws)
        .find(|l| !l.is_empty())
        .map(cut)
        .unwrap_or_default()
}

fn tailscale_address(ip: &str) -> bool {
    let Ok(std::net::IpAddr::V4(v4)) = ip.parse::<std::net::IpAddr>() else {
        return false;
    };
    let o = v4.octets();
    o[0] == 100 && (o[1] & 0xC0) == 0x40
}

fn parse_tailscale(out: &RemoteOutput, _ctx: &StepContext) -> CheckResult {
    let Some(v) = json_from(&out.text) else {
        let why = first_text_line(&out.text);
        return CheckResult::fail(if why.is_empty() {
            "Tailscale isn't running there".to_string()
        } else {
            format!("Tailscale isn't running there: {why}")
        });
    };
    let state = v["BackendState"].as_str().unwrap_or("");
    if state != "Running" {
        return CheckResult::fail(format!(
            "Tailscale is {}",
            if state.is_empty() {
                "not running".to_string()
            } else {
                cut(state)
            }
        ));
    }
    let address = v["Self"]["TailscaleIPs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find(|ip| tailscale_address(ip));
    match address {
        Some(ip) => CheckResult::pass(format!("running, {}", cut(ip))),
        None => CheckResult::fail("Tailscale is running but has no 100.x address"),
    }
}

fn linux_probe() -> String {
    format!(
        "{RUNTIME_DIR}\necho \"active=$(systemctl --user is-active {UNIT} 2>&1 | head -1)\"\necho \"linger=$(loginctl show-user \"$(id -un)\" -p Linger 2>&1 | head -1)\""
    )
}

fn mac_probe() -> String {
    format!("launchctl print \"gui/$(id -u)/{LABEL}\"")
}

pub(super) fn value_of<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.split(is_newline)
        .find_map(|l| l.strip_prefix(key))
        .map(trim_ws)
}

fn parse_service_linux(out: &RemoteOutput, _ctx: &StepContext) -> CheckResult {
    let active = value_of(&out.text, "active=").unwrap_or("");
    if active.is_empty() {
        return CheckResult::cant_tell("the machine didn't say whether the service runs");
    }
    if active != "active" {
        return CheckResult::fail(format!("the service is {}", cut(active)));
    }
    let linger = value_of(&out.text, "linger=").unwrap_or("");
    if linger != "Linger=yes" {
        return CheckResult::fail(
            "running, but it stops when the last login ends: Linger is off for the user",
        );
    }
    CheckResult::pass("running, and it stays up after logout")
}

fn parse_service_mac(out: &RemoteOutput, _ctx: &StepContext) -> CheckResult {
    if out.code != 0 {
        return CheckResult::fail("the LaunchAgent isn't loaded");
    }
    let state = out
        .text
        .split(is_newline)
        .map(trim_ws)
        .find_map(|l| l.strip_prefix("state = "))
        .unwrap_or("");
    if state == "running" {
        return CheckResult::pass("running");
    }
    CheckResult::fail(if state.is_empty() {
        "loaded, but not running".to_string()
    } else {
        format!("loaded, but {}", cut(state))
    })
}

fn is_node_id(text: &str) -> bool {
    (16..=64).contains(&text.len())
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

fn parse_joined(out: &RemoteOutput, ctx: &StepContext) -> CheckResult {
    let Some(want) = ctx.tokens.cluster_id() else {
        return CheckResult::fail(
            "This Mac has no cluster yet. Make one first, then join this machine to it.",
        );
    };
    let Some(v) = json_from(&out.text) else {
        let why = first_text_line(&out.text);
        return CheckResult::fail(if why.is_empty() {
            "the Notchling service there didn't answer".to_string()
        } else {
            format!("the Notchling service there didn't answer: {why}")
        });
    };
    let Some(theirs) = v["cluster"]["id"].as_str() else {
        return CheckResult::fail("not in a cluster yet");
    };
    if theirs != want {
        let name = v["cluster"]["name"].as_str().unwrap_or("");
        return CheckResult::fail(if name.is_empty() {
            "in another cluster".to_string()
        } else {
            format!("in another cluster, {}", cut(name))
        });
    }
    let node = v["me"]
        .as_str()
        .filter(|id| is_node_id(id))
        .map(str::to_string);
    let name = v["cluster"]["name"].as_str().unwrap_or("");
    CheckResult::pass(if name.is_empty() {
        "in this Mac's cluster".to_string()
    } else {
        format!("in {}", cut(name))
    })
    .detected(Detected {
        node,
        ..Detected::default()
    })
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

pub(super) fn load_command(linux: bool) -> String {
    if linux {
        format!(
            "systemctl --user daemon-reload && systemctl --user enable {UNIT} && systemctl --user restart {UNIT}"
        )
    } else {
        format!(
            "u=\"gui/$(id -u)\"; launchctl enable \"$u/{LABEL}\"; if launchctl print \"$u/{LABEL}\" >/dev/null 2>&1; then launchctl kickstart -k \"$u/{LABEL}\"; else launchctl bootstrap \"$u\" {}; fi",
            shell::path(PLIST)
        )
    }
}

pub(super) fn restart_command(linux: bool) -> String {
    if linux {
        format!(
            "{RUNTIME_DIR}\nif systemctl --user is-active --quiet {UNIT} 2>/dev/null; then systemctl --user restart {UNIT}; fi"
        )
    } else {
        format!(
            "u=\"gui/$(id -u)\"; if launchctl print \"$u/{LABEL}\" >/dev/null 2>&1; then launchctl kickstart -k \"$u/{LABEL}\"; fi"
        )
    }
}

fn load_script(linux: bool) -> String {
    if linux {
        format!(
            "{RUNTIME_DIR}\nfor i in 1 2 3 4 5 6 7 8 9 10; do [ -S \"$XDG_RUNTIME_DIR/bus\" ] && break; sleep 1; done\n{}",
            load_command(true)
        )
    } else {
        load_command(false)
    }
}

fn linger_query() -> &'static str {
    r#"loginctl show-user "$(id -un)" -p Linger"#
}

fn service_display(m: &Machine) -> String {
    let install = format!("{} service install", shell::path(BINARY));
    if m.is_linux() {
        format!(
            "Write the service file as the worker user:\n{install}\nThen, as admin, keep the service running after logout:\nloginctl enable-linger {}\n(skipped when {} already says Linger=yes)\nThen load it:\n{}",
            quote(&m.user),
            linger_query(),
            load_command(true)
        )
    } else {
        format!(
            "Write the LaunchAgent:\n{install}\nThen load it, or restart it when it already runs:\n{}",
            load_command(false)
        )
    }
}

fn service_fix(ctx: &FixContext) -> FixOutcome {
    let m = &ctx.step.machine;
    let linux = m.is_linux();
    let installed = run::script(
        ctx,
        &format!("{} service install", shell::path(BINARY)),
        None,
        120,
    );
    if installed.code != 0 {
        return FixOutcome::fail(format!(
            "couldn't write the service file (exit {}): {}",
            installed.code,
            said(ctx, &installed)
        ));
    }
    if linux {
        let asked = run::script(ctx, linger_query(), None, 60);
        if !asked.out.lines().any(|l| trim_ws(l) == "Linger=yes") {
            let enabled = run::admin(
                ctx,
                &format!("loginctl enable-linger {}", quote(&m.user)),
                120,
            );
            if enabled.code != 0 {
                return FixOutcome::fail(format!(
                    "couldn't keep the service running after logout (exit {}): {}",
                    enabled.code,
                    said(ctx, &enabled)
                ));
            }
        }
    }
    let loaded = run::script(ctx, &load_script(linux), None, 180);
    if loaded.code != 0 {
        return FixOutcome::fail(format!(
            "couldn't load the service (exit {}): {}",
            loaded.code,
            said(ctx, &loaded)
        ));
    }
    FixOutcome::ok("service written and loaded")
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

fn join_fix(ctx: &FixContext) -> FixOutcome {
    let token = match ctx.step.tokens.join_token() {
        Ok(token) if !token.trim().is_empty() => token.trim().to_string(),
        Ok(_) => return FixOutcome::fail("this Mac's cluster gave an empty join token"),
        Err(why) => return FixOutcome::fail(ctx.mask(&why)),
    };
    let mut masked = ctx.clone();
    masked.masks.hide("join-token", &token);
    masked.step.masks.hide("join-token", &token);
    let call = Call::secs(shell::ssh_script(&ctx.step.machine, &join_script()), 90)
        .stdin(format!("{token}\n").into_bytes());
    if masked.masks.leaks_into(&call.argv.join("\n")) {
        return FixOutcome::fail("refused: the token would have gone on a command line");
    }
    let ran = ctx.step.runner.run(&call);
    if ran.code != 0 {
        return FixOutcome::fail(format!(
            "couldn't join (exit {}): {}",
            ran.code,
            said(&masked, &ran)
        ));
    }
    let line = first_line_300(&ran.out, &|s| masked.mask(s));
    FixOutcome::ok(if line.is_empty() {
        "asked to join".to_string()
    } else {
        line
    })
}
