use super::dist_manifest;
use super::registry::{AdminAccess, Machine};
use super::runner::CommandResult;
use super::service_files::{node_launchdaemon, NODE_LABEL};
use super::shell;
use super::step::{
    run, Cadence, CheckResult, FixContext, FixOutcome, FixPlan, Probe, RemoteOutput, SetupStep,
    StepContext, StepFix, StepId,
};
use super::steps_machine::cut;
use super::steps_node::{
    arch_word, first_text_line, machine_refusal, os_word, said, value_of, BINARY, BINARY_NEW,
};
use super::steps_node_service::stops_work;
use crate::cluster::build_id;
use std::path::{Path, PathBuf};

pub(super) const MAC_BINARY: &str =
    "~/.local/share/notchling/Notchling.app/Contents/MacOS/notchling";
pub(super) const MAC_BINARY_NEW: &str =
    "~/.local/share/notchling/Notchling.app/Contents/MacOS/notchling.new";
pub(super) const MAC_REAL_TAIL: &str =
    ".local/share/notchling/Notchling.app/Contents/MacOS/notchling";
pub(super) const NODE_PLIST: &str = "/Library/LaunchDaemons/dev.notchling.node.plist";
pub(super) const OLD_LABEL: &str = "dev.notchling.daemon";
pub(super) const OLD_AGENT_PLIST: &str = "~/Library/LaunchAgents/dev.notchling.daemon.plist";
const HEREDOC_END: &str = "NOTCHLING_PLIST_END";
const BUILD_LINUX: &str = "Build the Linux nodes first: tools/build-node.sh";
const BUILD_INTEL: &str = "Build Notchling for Intel Macs first: tools/build-node.sh --mac-intel";
const UNKNOWN_CHIP: &str = "the machine's system and chip aren't known yet; check it first";

pub(super) fn binary_path(m: &Machine) -> &'static str {
    if m.is_linux() {
        BINARY
    } else {
        MAC_BINARY
    }
}

pub(super) fn binary_new(m: &Machine) -> &'static str {
    if m.is_linux() {
        BINARY_NEW
    } else {
        MAC_BINARY_NEW
    }
}

pub(super) fn where_word(m: &Machine) -> &'static str {
    if m.is_linux() {
        "~/.local/bin"
    } else {
        "~/.local/share/notchling"
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Host<'a> {
    pub arch: &'a str,
    pub build: Option<&'a str>,
}

impl Host<'static> {
    pub(super) fn this() -> Host<'static> {
        Host {
            arch: std::env::consts::ARCH,
            build: build_id::build_id(),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Source {
    pub path: PathBuf,
    pub label: String,
    pub what: String,
}

fn missing_dist(os: &str, arch: &str, dist: Option<&Path>, file: &str) -> String {
    match dist {
        Some(dir) => format!(
            "Put a notchling build for {os}-{arch} in {} first.",
            dir.join(file).display()
        ),
        None => format!("Put a notchling build for {os}-{arch} in the cluster dist folder first."),
    }
}

pub(super) fn source_for(
    m: &Machine,
    dist: Option<&Path>,
    mac_daemon: Option<&PathBuf>,
    host: Host,
) -> Result<Source, String> {
    let (Some(os), Some(arch)) = (os_word(m), arch_word(m)) else {
        return Err(UNKNOWN_CHIP.into());
    };
    let file = format!("notchling-{os}-{arch}");
    if os == "macos" {
        if arch == host.arch {
            if let Some(own) = mac_daemon.filter(|p| p.is_file()) {
                return Ok(Source {
                    path: own.clone(),
                    label: "this Mac's own notchling, the one beside the running app".into(),
                    what: "this Mac's".into(),
                });
            }
        }
        let path = dist.map(|d| d.join(&file)).filter(|p| p.is_file());
        return match path {
            Some(path) => Ok(Source {
                label: format!("{file} from this Mac's cluster dist folder"),
                path,
                what: "this Mac's".into(),
            }),
            None if arch == "x86_64" => Err(BUILD_INTEL.into()),
            None => Err(missing_dist(os, &arch, dist, &file)),
        };
    }
    let Some(dir) = dist else {
        return Err(missing_dist(os, &arch, dist, &file));
    };
    let path = dir.join(&file);
    if !path.is_file() {
        return Err(if host.build.is_some() {
            BUILD_LINUX.into()
        } else {
            missing_dist(os, &arch, dist, &file)
        });
    }
    match host.build {
        Some(build) => {
            let same = dist_manifest::read(dir).is_some_and(|manifest| manifest.build == build);
            if !same {
                return Err(BUILD_LINUX.into());
            }
            Ok(Source {
                label: format!("{file} from this Mac's cluster dist folder"),
                path,
                what: "this Mac's".into(),
            })
        }
        None => Ok(Source {
            label: format!("{file} from this Mac's cluster dist folder"),
            path,
            what: "the dist file's; this Mac has no build id, so the hashes are compared".into(),
        }),
    }
}

pub(super) fn copy_heading(m: &Machine) -> String {
    let (Some(os), Some(arch)) = (os_word(m), arch_word(m)) else {
        return format!(
            "Copy notchling-{}-{} from this Mac's cluster dist folder:",
            os_word(m).unwrap_or("<system>"),
            arch_word(m).as_deref().unwrap_or("<arch>")
        );
    };
    if os == "macos" && arch == std::env::consts::ARCH {
        return format!(
            "Copy this Mac's own notchling (the one beside the running app; notchling-macos-{arch} from the cluster dist folder when it isn't there):"
        );
    }
    format!("Copy notchling-{os}-{arch} from this Mac's cluster dist folder:")
}

pub(super) fn prepare_script(m: &Machine) -> String {
    if m.is_linux() {
        return format!("mkdir -p {}", shell::path("~/.local/bin"));
    }
    format!(
        "mkdir -p {} {}",
        shell::path("~/.local/bin"),
        shell::path("~/.local/share/notchling/Notchling.app/Contents/MacOS")
    )
}

pub(super) fn place_script(m: &Machine) -> String {
    let new = shell::path(binary_new(m));
    let old = shell::path(binary_path(m));
    let base = format!("chmod 755 {new} && mv -f {new} {old}");
    if m.is_linux() {
        return base;
    }
    format!("{base} && ln -sfn {old} {}", shell::path(BINARY))
}

fn kickstart() -> String {
    format!("launchctl kickstart -k system/{NODE_LABEL}")
}

fn loaded_test() -> String {
    format!("launchctl print system/{NODE_LABEL} >/dev/null 2>&1")
}

pub(super) fn mac_restart_text() -> String {
    format!("if {}; then {}; fi", loaded_test(), kickstart())
}

pub(super) fn mac_restart(ctx: &FixContext) -> CommandResult {
    let loaded = run::script(ctx, &loaded_test(), None, 60);
    if loaded.code != 0 {
        return CommandResult::ok("");
    }
    run::admin(ctx, &kickstart(), 120)
}

pub(super) fn load_script() -> String {
    format!(
        "launchctl enable system/{NODE_LABEL}\nif {}; then {}; else launchctl bootstrap system {NODE_PLIST}; fi",
        loaded_test(),
        kickstart()
    )
}

pub(super) fn data_folder_script() -> &'static str {
    "d=\"$HOME/Library/Application Support/Notchling\"; mkdir -p \"$d\" && touch \"$d/notchlingd.log\""
}

fn old_agent_script() -> String {
    format!(
        "launchctl bootout \"gui/$(id -u)/{OLD_LABEL}\" >/dev/null 2>&1; rm -f {}; true",
        shell::path(OLD_AGENT_PLIST)
    )
}

pub(super) fn install_script(plist: &str) -> Result<String, String> {
    if plist.lines().any(|l| l.contains(HEREDOC_END)) {
        return Err("the service file would end its own here-document".into());
    }
    Ok(format!(
        "set -e\numask 022\nt=$(mktemp /tmp/notchling-node.XXXXXX)\ntrap 'rm -f \"$t\"' EXIT\ncat > \"$t\" <<'{HEREDOC_END}'\n{plist}{HEREDOC_END}\nplutil -lint \"$t\" >/dev/null\ninstall -m 644 -o root -g wheel \"$t\" {NODE_PLIST}\n{}",
        load_script()
    ))
}

fn home_of(m: &Machine) -> Option<String> {
    m.abilities
        .as_ref()
        .and_then(|a| a.home.clone())
        .filter(|h| shell::is_safe_machine_path(h))
}

fn real_path(home: &str) -> String {
    format!("{home}/{MAC_REAL_TAIL}")
}

fn probe_script(m: &Machine) -> String {
    format!(
        "p={NODE_PLIST}\nif test -f \"$p\"; then echo plist=yes; else echo plist=no; fi\necho \"user=$(plutil -extract UserName raw -o - \"$p\" 2>/dev/null)\"\necho \"binary=$(plutil -extract ProgramArguments.0 raw -o - \"$p\" 2>/dev/null)\"\n{}; echo \"loaded=$?\"\nlaunchctl print gui/$(id -u)/{OLD_LABEL} >/dev/null 2>&1; echo \"old_agent=$?\"\ns=$({} status 2>&1); echo \"status_code=$?\"\necho \"status=$(printf '%s\\n' \"$s\" | head -1)\"",
        loaded_test(),
        shell::path(binary_path(m))
    )
}

fn parse_service(out: &RemoteOutput, ctx: &StepContext) -> CheckResult {
    if value_of(&out.text, "plist=") != Some("yes") {
        return CheckResult::fail("no service file there; the daemon wouldn't start by itself");
    }
    let user = value_of(&out.text, "user=").unwrap_or("");
    if user != ctx.machine.user {
        return CheckResult::fail(format!(
            "the service file runs as {}, not {}",
            cut(user),
            ctx.machine.user
        ));
    }
    let binary = value_of(&out.text, "binary=").unwrap_or("");
    let right = match home_of(&ctx.machine) {
        Some(home) => binary == real_path(&home),
        None => binary.ends_with(&format!("/{MAC_REAL_TAIL}")),
    };
    if !right {
        return CheckResult::fail(format!(
            "the service file starts another notchling: {}",
            cut(binary)
        ));
    }
    if value_of(&out.text, "loaded=") != Some("0") {
        return CheckResult::fail("the service file is there, but the service isn't loaded");
    }
    if value_of(&out.text, "old_agent=") == Some("0") {
        return CheckResult::fail(
            "an old per-login service is still loaded and competes for the daemon's folder",
        );
    }
    let line = value_of(&out.text, "status=")
        .map(first_text_line)
        .unwrap_or_default();
    if value_of(&out.text, "status_code=") == Some("0") && !line.is_empty() {
        return CheckResult::pass(line);
    }
    CheckResult::fail("the service is loaded, but the daemon doesn't answer")
}

fn display(m: &Machine) -> String {
    let head = format!(
        "{}\nMake the folder the service logs to, as the node user:\n{}\nBoot out and remove an old per-login service, as the node user:\n{}",
        stops_work(false),
        data_folder_script(),
        old_agent_script()
    );
    let plist = home_of(m)
        .ok_or_else(|| "the machine's home folder isn't known yet; check it first".to_string())
        .and_then(|home| node_launchdaemon(&m.user, &home, &real_path(&home)))
        .and_then(|text| install_script(&text));
    match plist {
        Ok(script) => format!(
            "{head}\nThen, as admin, write the LaunchDaemon, check it, and load it or restart it:\n{script}"
        ),
        Err(why) => format!("{head}\nThen, as admin, write the LaunchDaemon (not shown yet: {why})"),
    }
}

pub(super) fn service(m: &Machine) -> SetupStep {
    SetupStep::new(
        StepId::NODE_SERVICE,
        "Notchling service",
        vec![StepId::NODE_BINARY],
        Cadence::EveryRefresh,
        Probe::remote(probe_script(m), parse_service),
        StepFix::Run(FixPlan::new(display(m), true, service_fix)),
    )
}

fn service_fix(ctx: &FixContext) -> FixOutcome {
    let m = &ctx.step.machine;
    if let Some(why) = machine_refusal(m) {
        return FixOutcome::fail(why);
    }
    if m.admin() != AdminAccess::RootSsh && ctx.admin_password.is_none() {
        return FixOutcome::fail("needs the admin password");
    }
    let Some(home) = home_of(m) else {
        return FixOutcome::fail("the machine's home folder isn't known yet; check it first");
    };
    let script = match node_launchdaemon(&m.user, &home, &real_path(&home))
        .and_then(|text| install_script(&text))
    {
        Ok(script) => script,
        Err(why) => return FixOutcome::fail(why),
    };
    let folder = run::script(ctx, data_folder_script(), None, 60);
    if folder.code != 0 {
        return FixOutcome::fail(format!(
            "couldn't make the log folder (exit {}): {}",
            folder.code,
            said(ctx, &folder)
        ));
    }
    let _ = run::script(ctx, &old_agent_script(), None, 60);
    let wrote = run::admin(ctx, &script, 180);
    if wrote.code != 0 {
        return FixOutcome::fail(format!(
            "couldn't write and load the LaunchDaemon (exit {}): {}",
            wrote.code,
            said(ctx, &wrote)
        ));
    }
    FixOutcome::ok("LaunchDaemon written and loaded")
}

pub(super) fn app_closed(m: &Machine) -> SetupStep {
    SetupStep::new(
        StepId::NODE_APP_CLOSED,
        "Notchling app closed",
        vec![StepId::NODE_BINARY],
        Cadence::EveryRefresh,
        Probe::remote(
            "pgrep -x notchling-app >/dev/null && echo running || echo closed",
            |out, _| match first_text_line(&out.text).as_str() {
                "running" => CheckResult::fail(
                    "the Notchling app is open there; the daemon refuses a join while it is",
                ),
                "closed" => CheckResult::pass("the app isn't open"),
                _ => CheckResult::cant_tell("the machine didn't say whether the app is open"),
            },
        ),
        StepFix::HowTo(format!(
            "Quit Notchling on {}; the service keeps running.",
            m.name
        )),
    )
}
