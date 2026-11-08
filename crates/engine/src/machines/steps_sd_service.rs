//! The "Service installed" step (Swift `SDWorkerSteps` service part): the sd worker's units on
//! Linux (admin, written through root) and its LaunchAgent on macOS (the worker user writes it).
//! Nothing here enables or starts anything: that is the Running switch.

use super::service_files::{
    render, templates, ServiceFile, LINUX_NAMES, PLIST_NAME, PLIST_PATH, UNIT_DIRECTORY,
};
use super::shell;
use super::step::{
    run, Cadence, CheckResult, FixContext, FixOutcome, FixPlan, Probe, RemoteOutput, SetupStep,
    StepContext, StepFix, StepId,
};
use super::steps_sd::{first_line_300, running_probe, SD_PLUGIN};
use crate::lanes::quote;
use sha2::{Digest, Sha256};
use std::path::Path;

/// After the plist is written: the label stays disabled (launchd keeps that across logins) until
/// Running turns on.
pub const MAC_DISABLE_SCRIPT: &str = r#"launchctl disable "gui/$(id -u)/com.sd.worker""#;
const MAC_LABEL: &str = "com.sd.worker";

pub(super) fn step(linux: bool) -> SetupStep {
    SetupStep::new(
        StepId::SERVICE,
        "Service installed",
        vec![StepId::WORKER_JSON, StepId::BASE_TOOLS],
        Cadence::EveryRefresh,
        Probe::remote(probe(linux), check),
        StepFix::Run(FixPlan::new(display(linux), linux, install)),
    )
}

/// The `launchctl print-disabled` line for the label says disabled (`disabled`, or `true` on
/// older macOS).
pub(super) fn label_disabled(lines: &[&str]) -> bool {
    lines.iter().any(|l| {
        l.contains(r#""com.sd.worker" =>"#) && (l.ends_with("disabled") || l.ends_with("true"))
    })
}

fn service_target(name: &str, linux: bool) -> String {
    if linux {
        format!("{UNIT_DIRECTORY}/{name}")
    } else {
        PLIST_PATH.into()
    }
}

fn probe(linux: bool) -> String {
    let digest = if linux { "sha256sum" } else { "shasum -a 256" };
    let names: Vec<&str> = if linux {
        LINUX_NAMES.to_vec()
    } else {
        vec![PLIST_NAME]
    };
    let mut parts: Vec<String> = names
        .iter()
        .map(|name| {
            let path = shell::path(&service_target(name, linux));
            format!(
                "if [ -f {path} ]; then {digest} {path}; else echo {}; fi",
                quote(&format!("missing {name}"))
            )
        })
        .collect();
    // On a Mac, then whether the label is loaded and disabled: a plist that is neither would
    // start at login.
    if !linux {
        parts.push(running_probe(false));
    }
    parts.join("; ")
}

fn hex(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// The files this Mac's sd templates give for `ctx.machine`.
fn rendered(ctx: &StepContext) -> Result<Vec<ServiceFile>, String> {
    let machine = &ctx.machine;
    let job = machine.sd_worker().ok_or("no sd worker job")?;
    let sd = ctx
        .mac
        .plugins
        .get(SD_PLUGIN)
        .ok_or("this Mac has no sd plugin to take the templates from")?;
    let found = templates(&Path::new(&sd.path).join("templates/worker"))
        .ok_or("sd on this Mac has no worker templates")?;
    let home = machine
        .abilities
        .as_ref()
        .and_then(|a| a.home.as_deref())
        .ok_or("the machine's home folder isn't known yet")?;
    render(
        &found,
        machine.is_linux(),
        &machine.user,
        home,
        &job.python,
        &job.sd_path,
    )
}

fn check(out: &RemoteOutput, ctx: &StepContext) -> CheckResult {
    let files = match rendered(ctx) {
        Ok(f) => f,
        Err(why) => return CheckResult::fail(why),
    };
    let lines: Vec<&str> = out.text.split('\n').collect();
    let mut missing: Vec<&str> = Vec::new();
    let mut differ: Vec<&str> = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let line = lines.get(index).copied().unwrap_or("");
        if line.starts_with("missing") {
            missing.push(&file.name);
            continue;
        }
        let word = line.split(' ').find(|w| !w.is_empty()).unwrap_or("");
        if word != hex(&file.content) {
            differ.push(&file.name);
        }
    }
    if differ.is_empty() && missing.is_empty() && files.len() == 1 && files[0].name == PLIST_NAME {
        // line 0 is the digest; then `loaded`/`notloaded` and the print-disabled line. Running on
        // (loaded, enabled) is fine.
        let rest: Vec<&str> = lines
            .iter()
            .skip(1)
            .map(|l| l.trim_matches([' ', '\t']))
            .collect();
        if rest.first() != Some(&"loaded") && !label_disabled(&rest) {
            return CheckResult::fail("installed but not disabled; a login would start it");
        }
    }
    if differ.is_empty() && missing.is_empty() {
        return CheckResult::pass(if files.len() == 1 {
            format!("{} matches", files[0].name)
        } else {
            format!("{} units match", files.len())
        });
    }
    let mut parts: Vec<String> = Vec::new();
    if !differ.is_empty() {
        parts.push(format!("differs: {}", differ.join(", ")));
    }
    if !missing.is_empty() {
        parts.push(format!("missing: {}", missing.join(", ")));
    }
    CheckResult::fail(parts.join("; "))
}

/// The admin script that writes the units: each file's bytes as base64, then `daemon-reload`. It
/// enables and starts nothing, and it reads no stdin.
pub fn install_script(files: &[ServiceFile]) -> String {
    use base64::Engine;
    let mut lines = vec!["set -e".to_string(), "umask 022".to_string()];
    for file in files {
        // a temp file, then a move, on its own line so `set -e` stops on a failed decode
        lines.push(format!(
            "printf '%s' '{}' | base64 -d > {}.new",
            base64::engine::general_purpose::STANDARD.encode(file.content.as_bytes()),
            file.remote_path
        ));
        lines.push(format!("mv -f {0}.new {0}", file.remote_path));
    }
    lines.push("systemctl daemon-reload".into());
    lines.join("\n")
}

fn display(linux: bool) -> String {
    if !linux {
        return format!(
            "First, keep it from starting at login until you turn Running on (skipped when Running is already on):\n{MAC_DISABLE_SCRIPT}\nThen write the LaunchAgent (mode 644) with the plist on stdin:\n{}",
            shell::write_file(PLIST_PATH, "644")
        );
    }
    let mut lines = vec!["set -e".to_string(), "umask 022".to_string()];
    for name in LINUX_NAMES {
        let path = format!("{UNIT_DIRECTORY}/{name}");
        lines.push(format!(
            "printf '%s' '<base64 of {name}>' | base64 -d > {path}.new"
        ));
        lines.push(format!("mv -f {path}.new {path}"));
    }
    lines.push("systemctl daemon-reload".into());
    lines.push("(nothing is enabled or started)".into());
    lines.join("\n")
}

fn install(ctx: &FixContext) -> FixOutcome {
    let files = match rendered(&ctx.step) {
        Ok(f) => f,
        Err(why) => return FixOutcome::fail(why),
    };
    let said = |r: &super::runner::CommandResult| first_line_300(&r.err, &|s| ctx.mask(s));
    if ctx.step.machine.is_linux() {
        let ran = run::admin(ctx, &install_script(&files), 900);
        return if ran.code == 0 {
            FixOutcome::ok(format!("{} units written", files.len()))
        } else {
            FixOutcome::fail(format!(
                "couldn't write the units (exit {}): {}",
                ran.code,
                said(&ran)
            ))
        };
    }
    // The plist has RunAtLoad, so launchd would start it at the next login. Disable the label
    // first (launchd keeps that without a plist or a loaded job), so a failure leaves no plist
    // behind. This never loads it: no bootstrap, no enable.
    //
    // One state skips that: Running is on (the label is loaded and enabled, the Running check's
    // own test). The plist is then rewritten under the running job and the label stays enabled, or
    // this Fix would turn Running off. A state that can't be read counts as not running: the
    // label is disabled.
    let state = run::script(ctx, &running_probe(false), None, 60);
    let state_lines: Vec<&str> = state
        .out
        .split('\n')
        .map(|l| l.trim_matches([' ', '\t']))
        .filter(|l| !l.is_empty())
        .collect();
    let running_is_on = state_lines.first() == Some(&"loaded") && !label_disabled(&state_lines);
    if !running_is_on {
        let off = run::script(ctx, MAC_DISABLE_SCRIPT, None, 60);
        if off.code != 0 {
            return FixOutcome::fail(format!(
                "couldn't disable {MAC_LABEL} (exit {}): {}; no service file was written",
                off.code,
                said(&off)
            ));
        }
    }
    let wrote = run::script(
        ctx,
        &shell::write_file(PLIST_PATH, "644"),
        Some(files[0].content.clone().into_bytes()),
        60,
    );
    if wrote.code == 0 {
        FixOutcome::ok(if running_is_on {
            format!("{PLIST_NAME} written; Running stays on")
        } else {
            format!("{PLIST_NAME} written; {MAC_LABEL} stays disabled until Running is turned on")
        })
    } else {
        FixOutcome::fail(format!("couldn't write the LaunchAgent: {}", said(&wrote)))
    }
}
