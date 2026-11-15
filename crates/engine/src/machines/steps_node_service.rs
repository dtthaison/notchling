use super::registry::Machine;
use super::shell;
use super::step::{
    run, Cadence, CheckResult, FixContext, FixOutcome, FixPlan, Probe, RemoteOutput, SetupStep,
    StepContext, StepFix, StepId,
};
use super::steps_machine::{cut, is_newline, trim_ws};
use super::steps_node::{
    first_text_line, machine_refusal, said, value_of, BINARY, LABEL, NOT_NEEDED, RUNTIME_DIR, UNIT,
};
use crate::lanes::quote;

const PLIST: &str = "~/Library/LaunchAgents/dev.notchling.daemon.plist";
const STATES: [&str; 8] = [
    "active",
    "inactive",
    "activating",
    "deactivating",
    "failed",
    "reloading",
    "maintenance",
    "refreshing",
];

pub(super) fn stops_work(linux: bool) -> &'static str {
    if linux {
        "A restart leaves shell jobs and sessions running when the service file is up to date. The restarted service doesn't know the ones it started before."
    } else {
        "Work running on this machine stops while the service restarts."
    }
}

pub(super) fn service(m: &Machine) -> SetupStep {
    let linux = m.is_linux();
    let (script, parse): (String, fn(&RemoteOutput, &StepContext) -> CheckResult) = if linux {
        (linux_probe(), parse_linux)
    } else {
        (mac_probe(), parse_mac)
    };
    SetupStep::new(
        StepId::NODE_SERVICE,
        "Notchling service",
        vec![StepId::NODE_BINARY],
        Cadence::EveryRefresh,
        Probe::remote(script, parse),
        StepFix::Run(FixPlan::new(display(m), linux, service_fix)),
    )
}

fn linux_probe() -> String {
    format!(
        "{RUNTIME_DIR}\necho \"active=$(systemctl --user is-active {UNIT} 2>&1 | head -1)\"\necho \"killmode=$(systemctl --user show {UNIT} -p KillMode 2>&1 | head -1)\"\necho \"linger=$(loginctl show-user \"$(id -un)\" -p Linger 2>&1 | head -1)\""
    )
}

fn mac_probe() -> String {
    format!("{} status", shell::path(BINARY))
}

fn parse_linux(out: &RemoteOutput, _ctx: &StepContext) -> CheckResult {
    let active = value_of(&out.text, "active=").unwrap_or("");
    if active.is_empty() {
        return CheckResult::cant_tell("the machine didn't say whether the service runs");
    }
    if !STATES.contains(&active) {
        return CheckResult::fail(format!(
            "the machine's service manager didn't answer: {}",
            cut(active)
        ));
    }
    if active != "active" {
        return CheckResult::fail(format!("the service is {active}"));
    }
    match value_of(&out.text, "killmode=") {
        Some("KillMode=process") => {}
        Some(mode) if mode.starts_with("KillMode=") => {
            return CheckResult::fail("the service file is out of date");
        }
        _ => {
            return CheckResult::cant_tell("the machine didn't say how the service restarts");
        }
    }
    let linger = value_of(&out.text, "linger=").unwrap_or("");
    if linger != "Linger=yes" {
        return CheckResult::fail(
            "running, but it stops when the last login ends: Linger is off for the user",
        );
    }
    CheckResult::pass("running, and it stays up after logout")
}

fn parse_mac(out: &RemoteOutput, ctx: &StepContext) -> CheckResult {
    if ctx.machine.runs_app() {
        return CheckResult::pass(NOT_NEEDED);
    }
    let line = first_text_line(&out.text);
    if out.code == 0 && !line.is_empty() {
        return CheckResult::pass(line);
    }
    CheckResult::fail("no Notchling daemon answers there")
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

pub(super) fn load_script(linux: bool) -> String {
    if linux {
        format!(
            "{RUNTIME_DIR}\nfor i in 1 2 3 4 5 6 7 8 9 10; do [ -S \"$XDG_RUNTIME_DIR/bus\" ] && break; sleep 1; done\n{}",
            load_command(true)
        )
    } else {
        load_command(false)
    }
}

pub(super) fn log_folder_script(linux: bool) -> String {
    if linux {
        "case \"${XDG_DATA_HOME:-}\" in /*) d=\"$XDG_DATA_HOME/notchling\";; *) d=\"$HOME/.local/share/notchling\";; esac; mkdir -p \"$d\"".into()
    } else {
        "mkdir -p \"$HOME/Library/Application Support/Notchling\"".into()
    }
}

fn linger_query() -> &'static str {
    r#"loginctl show-user "$(id -un)" -p Linger"#
}

fn install_script() -> String {
    format!("{} service install", shell::path(BINARY))
}

fn display(m: &Machine) -> String {
    let linux = m.is_linux();
    let head = format!(
        "{}\nMake the folder the service logs to:\n{}\nWrite the service file as the worker user:\n{}",
        stops_work(linux),
        log_folder_script(linux),
        install_script()
    );
    if linux {
        format!(
            "{head}\nThen, as admin, keep the service running after logout:\nloginctl enable-linger {}\n(skipped when {} already says Linger=yes)\nThen load it and restart it:\n{}",
            quote(&m.user),
            linger_query(),
            load_script(true)
        )
    } else {
        format!(
            "{head}\nThen load it, or restart it when it already runs:\n{}",
            load_script(false)
        )
    }
}

fn service_fix(ctx: &FixContext) -> FixOutcome {
    let m = &ctx.step.machine;
    if let Some(why) = machine_refusal(m) {
        return FixOutcome::fail(why);
    }
    let linux = m.is_linux();
    if linux {
        if let Some(why) = shell::admin_refusal(m) {
            return FixOutcome::fail(why);
        }
    }
    let folder = run::script(ctx, &log_folder_script(linux), None, 60);
    if folder.code != 0 {
        return FixOutcome::fail(format!(
            "couldn't make the log folder (exit {}): {}",
            folder.code,
            said(ctx, &folder)
        ));
    }
    let installed = run::script(ctx, &install_script(), None, 120);
    if installed.code != 0 {
        return FixOutcome::fail(format!(
            "couldn't write the service file (exit {}): {}",
            installed.code,
            said(ctx, &installed)
        ));
    }
    if linux {
        let asked = run::script(ctx, linger_query(), None, 60);
        if !asked
            .out
            .split(is_newline)
            .any(|l| trim_ws(l) == "Linger=yes")
        {
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
