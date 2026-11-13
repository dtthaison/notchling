use super::registry::{Machine, SdWorkerSettings};
use super::shell;
use super::step::{
    run, Cadence, CheckResult, Detected, FixContext, FixOutcome, FixPlan, Probe, RemoteOutput,
    SetupStep, StepContext, StepFix, StepId,
};
use crate::lanes::quote;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

pub const SD_PLUGIN: &str = "sd@system-design";
pub const SUPERPOWERS_PLUGIN: &str = "superpowers@claude-plugins-official";
pub const UNREADABLE_VERSION: &str = "(unreadable version)";

pub const MIN_SD: &str = "1.19.3";

const TEXT_CAP: usize = 300;

pub const PLUGINS_PROBE: &str =
    "head -c 1048576 \"$HOME/.claude/plugins/installed_plugins.json\" 2>/dev/null\n\
echo; echo @@sd\n\
d=\"$HOME/.local/share/agents/system-design/plugin/scripts/sd.py\"\n\
[ -f \"$d\" ] && printf 'sd=%s\\n' \"$d\"";

pub fn all(m: &Machine, job: &SdWorkerSettings, all: &[Machine]) -> Vec<SetupStep> {
    let linux = m.is_linux();
    let machine = m.name.clone();
    let mut check_step = SetupStep::new(
        StepId::SD_CHECK,
        "sd worker setup --check",
        vec![StepId::SERVICE],
        Cadence::Slow,
        super::steps_sd_check::check_probe(job),
        StepFix::Run(super::steps_sd_check::create_labels_plan(job)),
    );
    check_step.hold_when = Some(std::sync::Arc::new(move |d: &Detected| {
        let version = d.sd_version.as_deref()?;
        (!is_at_least(version, MIN_SD))
            .then(|| format!("sd on {machine} is older than {MIN_SD}; update plugins first"))
    }));
    vec![
        SetupStep::new(
            StepId::PLUGINS,
            "Plugins installed",
            vec![StepId::CLAUDE_CODE],
            Cadence::EveryRefresh,
            Probe::remote(PLUGINS_PROBE, plugins_check),
            StepFix::Run(FixPlan::new(plugins_display(), false, install_plugins)),
        ),
        super::worker_json::step(m, job, all),
        super::steps_sd_service::step(linux),
        check_step,
        SetupStep::new(
            StepId::RUNNING,
            "Running",
            vec![StepId::REACHABLE],
            Cadence::EveryRefresh,
            Probe::remote(running_probe(linux), move |out, _| {
                running_check(out, linux)
            }),
            StepFix::Toggle { needs_admin: linux },
        ),
    ]
}

pub(super) fn cut_300(s: &str, mask: &dyn Fn(&str) -> String) -> String {
    mask(s).chars().take(TEXT_CAP).collect()
}

pub(super) fn first_line_300(s: &str, mask: &dyn Fn(&str) -> String) -> String {
    first_line_raw(&mask(s))
}

fn first_line_raw(masked: &str) -> String {
    masked
        .split('\n')
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .chars()
        .take(TEXT_CAP)
        .collect()
}

pub fn is_at_least(version: &str, minimum: &str) -> bool {
    fn parts(s: &str) -> Option<Vec<i64>> {
        s.split('.').map(|p| p.parse::<i64>().ok()).collect()
    }
    let (Some(have), Some(want)) = (parts(version), parts(minimum)) else {
        return false;
    };
    for i in 0..have.len().max(want.len()) {
        let a = have.get(i).copied().unwrap_or(0);
        let b = want.get(i).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    true
}

fn is_safe_version(v: &str) -> bool {
    (1..=32).contains(&v.len())
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '+' || c == '-')
}

fn machine_versions(json: &str) -> BTreeMap<&'static str, String> {
    let mut found = BTreeMap::new();
    let Ok(root) = serde_json::from_str::<Value>(json.trim()) else {
        return found;
    };
    let Some(plugins) = root.get("plugins") else {
        return found;
    };
    for id in [SD_PLUGIN, SUPERPOWERS_PLUGIN] {
        let entry = plugins.get(id).and_then(Value::as_array).and_then(|list| {
            list.iter()
                .find(|e| e.get("scope").and_then(Value::as_str) == Some("user"))
        });
        let Some(entry) = entry else { continue };
        let version = entry.get("version").and_then(Value::as_str).unwrap_or("");
        found.insert(
            id,
            if is_safe_version(version) {
                version.to_string()
            } else {
                UNREADABLE_VERSION.to_string()
            },
        );
    }
    found
}

fn plugins_check(out: &RemoteOutput, ctx: &StepContext) -> CheckResult {
    let lines: Vec<&str> = out.text.split('\n').collect();
    let marker = lines.iter().position(|l| *l == "@@sd");
    let json = lines[..marker.unwrap_or(lines.len())].join("\n");
    let mut detected = Detected::default();
    if let Some(marker) = marker {
        if let Some(line) = lines[marker + 1..].iter().find(|l| l.starts_with("sd=")) {
            let path = &line[3..];
            if shell::is_safe_machine_path(path) {
                detected.sd_path = Some(path.to_string());
            }
        }
    }
    let (Some(sd), Some(superpowers)) = (
        ctx.mac.plugins.get(SD_PLUGIN),
        ctx.mac.plugins.get(SUPERPOWERS_PLUGIN),
    ) else {
        return CheckResult::cant_tell("this Mac doesn't have both superpowers and sd installed")
            .detected(detected);
    };
    let here = machine_versions(&json);
    if let Some(v) = here.get(SD_PLUGIN) {
        if v != UNREADABLE_VERSION {
            detected.sd_version = Some(v.clone());
        }
    }
    let mut wrong: Vec<String> = Vec::new();
    for (name, id, mac, exact) in [
        ("superpowers", SUPERPOWERS_PLUGIN, superpowers, false),
        ("sd", SD_PLUGIN, sd, true),
    ] {
        let Some(version) = here.get(id) else {
            wrong.push(format!("{name} missing, {} on this Mac", mac.version));
            continue;
        };
        let ok = if exact {
            *version == mac.version
        } else {
            *version == mac.version || is_at_least(version, &mac.version)
        };
        if !ok {
            wrong.push(format!(
                "{name} {version} here, {} on this Mac",
                mac.version
            ));
        }
    }
    if !wrong.is_empty() {
        return CheckResult::fail(wrong.join("; ")).detected(detected);
    }
    if detected.sd_path.is_none() {
        return CheckResult::fail("sd.py is not at ~/.local/share/agents/system-design/plugin")
            .detected(detected);
    }
    let empty = String::new();
    let sp = here.get(SUPERPOWERS_PLUGIN).unwrap_or(&empty);
    let sdv = here.get(SD_PLUGIN).unwrap_or(&empty);
    CheckResult::pass(format!("superpowers {sp}, sd {sdv}")).detected(detected)
}

pub fn plugins_script(marketplace: &str) -> String {
    format!(
        "set -e; d=\"$HOME/.local/share/agents/system-design\"\n\
rm -rf \"$d.new\"; mkdir -p \"$d.new/plugin\" \"$d.new/.claude-plugin\"\n\
tar -xzf - -C \"$d.new/plugin\"\n\
printf '%s' {} > \"$d.new/.claude-plugin/marketplace.json\"\n\
rm -rf \"$d.old\"; [ ! -e \"$d\" ] || mv \"$d\" \"$d.old\"\n\
mv \"$d.new\" \"$d\" || {{ [ ! -e \"$d.old\" ] || mv \"$d.old\" \"$d\"; exit 1; }}\n\
rm -rf \"$d.old\"\n\
claude plugin marketplace add \"$d\" >/dev/null 2>&1 || claude plugin marketplace update system-design\n\
claude plugin install sd@system-design || claude plugin update sd@system-design\n\
claude plugin marketplace add anthropics/claude-plugins-official >/dev/null 2>&1 || claude plugin marketplace update claude-plugins-official\n\
claude plugin install superpowers@claude-plugins-official || claude plugin update superpowers@claude-plugins-official",
        quote(marketplace)
    )
}

fn plugins_display() -> String {
    format!(
        "On this Mac: tar -czf <temp>/sd-plugin.tgz -C <this Mac's sd folder> .\nOn the machine, with that archive on stdin:\n{}",
        plugins_script("<this Mac's marketplace.json>")
    )
}

struct ScratchDir(std::path::PathBuf);

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn install_plugins(ctx: &FixContext) -> FixOutcome {
    let mac = &ctx.step.mac;
    let (Some(sd), Some(market)) = (
        mac.plugins.get(SD_PLUGIN),
        mac.marketplace_json.get("system-design"),
    ) else {
        return FixOutcome::fail("this Mac has no sd plugin or system-design marketplace to copy");
    };
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = ctx.step.scratch.join(format!(
        "sd-plugin-{}-{nanos}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    if notchling_platform::files::ensure_private_dir(&ctx.step.scratch).is_err()
        || std::fs::create_dir_all(&dir).is_err()
    {
        return FixOutcome::fail("couldn't make a temporary folder");
    }
    let dir = ScratchDir(dir);
    let archive = dir.0.join("sd-plugin.tgz").to_string_lossy().into_owned();
    let tar = run::local_env(
        ctx,
        vec![
            "/usr/bin/tar".into(),
            "-czf".into(),
            archive.clone(),
            "-C".into(),
            sd.path.clone(),
            ".".into(),
        ],
        &[("COPYFILE_DISABLE", "1")],
        Duration::from_secs(120),
    );
    let data = std::fs::read(&archive).unwrap_or_default();
    if tar.code != 0 || data.is_empty() {
        return FixOutcome::fail(format!(
            "couldn't pack sd {} on this Mac: {}",
            sd.version,
            first_line_300(&tar.err, &|s| ctx.mask(s))
        ));
    }
    let ran = run::script(ctx, &plugins_script(market), Some(data), 600);
    if ran.code == 0 {
        FixOutcome::ok(format!("sd {} and superpowers installed", sd.version))
    } else {
        FixOutcome::fail(format!(
            "the install failed (exit {}): {}",
            ran.code,
            first_line_300(&ran.err, &|s| ctx.mask(s))
        ))
    }
}

pub(super) fn running_probe(linux: bool) -> String {
    if linux {
        "systemctl is-enabled sd-worker.service; systemctl is-active sd-worker.service".into()
    } else {
        r#"launchctl print "gui/$(id -u)/com.sd.worker" >/dev/null 2>&1 && echo loaded || echo notloaded; launchctl print-disabled "gui/$(id -u)" 2>/dev/null | grep '"com.sd.worker" =>'"#.into()
    }
}

fn running_check(out: &RemoteOutput, linux: bool) -> CheckResult {
    let lines: Vec<&str> = out
        .text
        .split('\n')
        .map(|l| l.trim_matches([' ', '\t']))
        .filter(|l| !l.is_empty())
        .collect();
    if linux {
        return if lines == ["enabled", "active"] {
            CheckResult::pass("on")
        } else {
            CheckResult::fail("off")
        };
    }
    if lines.first() == Some(&"loaded") && !super::steps_sd_service::label_disabled(&lines) {
        CheckResult::pass("on")
    } else {
        CheckResult::fail("off")
    }
}

pub fn toggle_script(linux: bool, on: bool) -> String {
    match (linux, on) {
        (true, true) => "systemctl enable --now sd-worker.service".into(),
        (true, false) => "systemctl disable --now sd-worker.service".into(),
        (false, true) => concat!(
            r#"launchctl enable "gui/$(id -u)/com.sd.worker" || exit 1; "#,
            r#"launchctl print "gui/$(id -u)/com.sd.worker" >/dev/null 2>&1 || "#,
            r#"launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/com.sd.worker.plist""#
        )
        .into(),
        (false, false) => concat!(
            r#"if launchctl print "gui/$(id -u)/com.sd.worker" >/dev/null 2>&1; then "#,
            r#"launchctl bootout "gui/$(id -u)/com.sd.worker" || exit 1; fi; "#,
            r#"launchctl disable "gui/$(id -u)/com.sd.worker""#
        )
        .into(),
    }
}
