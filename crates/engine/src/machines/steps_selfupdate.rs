use super::engine::SetupEngine;
use super::engine_check::nonce;
use super::registry::Machine;
use super::remote_edit::{
    apply, friendly, parse_read, read_quiet, read_script_expr, resolved_path, Read,
};
use super::runner::Call;
use super::shell;
use super::step::{
    Cadence, CheckResult, Detected, FixContext, FixOutcome, FixPlan, Probe, RemoteOutput,
    SetupStep, StepContext, StepFix, StepId, SubRow,
};
use crate::install::jsonc;
use crate::versions::knob::{
    place_state, plan_off, plan_on, shown_off, shown_on, Knob, KnobState, Place,
};
use crate::versions::{Tool, ToolState, Version, WorkerTools};

pub const TOOLS: [Tool; 3] = [Tool::Claude, Tool::Codex, Tool::Agy];
pub const PROBE_FILE_MAX: usize = 20 * 1024;
pub const UNMANAGED: &str = "Not managed yet: Notchling finds and repairs drift";
const CODEX_HOME_WORD: &str = "\"${CODEX_HOME:-$HOME/.codex}/config.toml\"";

pub fn knobs(tool: Tool) -> Vec<Knob> {
    let all: &[Knob] = match tool {
        Tool::Claude => &[Knob::ClaudeDisableAutoupdater],
        Tool::Codex => &[Knob::CodexUpdateCheck, Knob::CodexDaemonAutoStart],
        Tool::Agy => &[Knob::AgyEnv],
        Tool::Notchling => &[],
    };
    all.iter().copied().filter(|k| k.verified()).collect()
}

fn service_file(m: &Machine, home: &str) -> String {
    if m.is_linux() {
        "/etc/systemd/system/sd-worker.service".to_string()
    } else {
        format!("{home}/Library/LaunchAgents/com.sd.worker.plist")
    }
}

pub fn places_with(m: &Machine, home: &str, tool: Tool, codex_home: Option<&str>) -> Vec<Place> {
    let mut out = Vec::new();
    match tool {
        Tool::Claude => {
            out.push(Place::ConfigDir(format!("{home}/.claude")));
            if m.sd_worker().is_some() {
                out.push(Place::ServiceFile(service_file(m, home)));
            }
        }
        Tool::Codex => out.push(Place::CodexHome(
            codex_home.map_or_else(|| format!("{home}/.codex"), str::to_string),
        )),
        Tool::Agy => {
            if m.sd_worker().is_some() {
                out.push(Place::ServiceFile(service_file(m, home)));
            }
        }
        Tool::Notchling => {}
    }
    out.retain(|p| shell::is_safe_machine_path(&p.file()));
    out
}

pub fn places(m: &Machine, home: &str, tool: Tool) -> Vec<Place> {
    places_with(m, home, tool, None)
}

fn takes(knob: Knob, place: &Place) -> bool {
    place.fits(knob)
        || (place.is_service() && matches!(knob, Knob::ClaudeDisableAutoupdater | Knob::AgyEnv))
}

pub fn targets(
    m: &Machine,
    home: &str,
    tool: Tool,
    codex_home: Option<&str>,
) -> Vec<(Knob, Place)> {
    let places = places_with(m, home, tool, codex_home);
    let mut out = Vec::new();
    for knob in knobs(tool) {
        for place in &places {
            if takes(knob, place) {
                out.push((knob, place.clone()));
            }
        }
    }
    out
}

fn home_of(m: &Machine) -> Option<String> {
    m.abilities
        .as_ref()
        .and_then(|a| a.home.clone())
        .filter(|h| shell::is_safe_machine_path(h))
}

pub fn steps(m: &Machine) -> Vec<SetupStep> {
    TOOLS.into_iter().map(|t| step(m, t)).collect()
}

pub fn only_installed(list: Vec<SetupStep>, tools: Option<&WorkerTools>) -> Vec<SetupStep> {
    list.into_iter()
        .filter(|s| match (s.id.self_update_tool(), tools) {
            (Some(tool), Some(w)) => !matches!(w.tools.get(&tool), Some(ToolState::NotInstalled)),
            _ => true,
        })
        .collect()
}

struct Group {
    place: Place,
    knobs: Vec<Knob>,
    word: Option<String>,
}

fn groups(targets: &[(Knob, Place)], dynamic_codex: bool) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    for (knob, place) in targets {
        match out.iter_mut().find(|g| &g.place == place) {
            Some(g) => g.knobs.push(*knob),
            None => out.push(Group {
                place: place.clone(),
                knobs: vec![*knob],
                word: (dynamic_codex && matches!(place, Place::CodexHome(_)))
                    .then(|| CODEX_HOME_WORD.to_string()),
            }),
        }
    }
    out
}

fn probe_script(tool: Tool, groups: &[Group], nonce: &str) -> String {
    let mut script = String::new();
    match tool {
        Tool::Claude => script.push_str(
            "v=$(/bin/sh -lc 'printf %s \"${CLAUDE_CODE_PACKAGE_MANAGER_AUTO_UPDATE:-}\"' 2>/dev/null </dev/null | head -c 16)\ncase \"$v\" in 0|1|true|false) echo \"pkgmgr=$v\";; \"\") ;; *) echo pkgmgr=other;; esac\n",
        ),
        Tool::Codex => script.push_str(
            "pgrep -f 'app-server daemon pid-update-lo[o]p' >/dev/null 2>&1 && echo codex_update_loop=1\n",
        ),
        _ => {}
    }
    for (i, g) in groups.iter().enumerate() {
        script.push_str(&format!("echo '@@place {nonce} {i}'\n("));
        let word = g
            .word
            .clone()
            .unwrap_or_else(|| shell::path(&g.place.file()));
        script.push_str(&read_script_expr(&word, nonce, PROBE_FILE_MAX));
        script.push_str(")\n");
    }
    script
}

fn split_places(text: &str, nonce: &str) -> Vec<String> {
    let marker = format!("@@place {nonce} ");
    let mut segments = vec![String::new()];
    for line in text.split_inclusive('\n') {
        if line.starts_with(&marker) {
            segments.push(String::new());
        } else if let Some(last) = segments.last_mut() {
            last.push_str(line);
        }
    }
    segments
}

fn unset_text(knob: Knob, place: &Place, state: &KnobState) -> String {
    let line = shown_off(knob, place);
    match state {
        KnobState::Other(word) => format!("Is {word}; will become: {line}"),
        _ => format!("Will add: {line}"),
    }
}

fn minimum_version(text: &str) -> Option<Version> {
    let root = jsonc::parse(text).ok()?;
    root.get("minimumVersion")?
        .as_str()
        .and_then(Version::parse)
}

struct Seen {
    rows: Vec<SubRow>,
    cant_read: Option<String>,
    unset: Option<String>,
}

fn read_groups(
    groups: &[Group],
    segments: &[String],
    nonce: &str,
    home: Option<&str>,
) -> (Seen, Vec<SubRow>) {
    let mut seen = Seen {
        rows: Vec::new(),
        cant_read: None,
        unset: None,
    };
    let mut extra = Vec::new();
    for (g, segment) in groups.iter().zip(segments.iter().skip(1)) {
        let read = parse_read(
            &RemoteOutput {
                text: segment.clone(),
                code: 0,
            },
            nonce,
        );
        let shown_path = friendly(
            &resolved_path(segment).unwrap_or_else(|| g.place.file()),
            home,
        );
        if matches!(read, Read::Missing) && g.place.is_service() {
            continue;
        }
        let Some(text) = read.text() else {
            let why = read.why().unwrap_or("unreadable");
            seen.cant_read
                .get_or_insert_with(|| format!("Can't read {shown_path}: {why}"));
            seen.rows
                .push(SubRow::new(&format!("Can't read {shown_path}"), false, why));
            continue;
        };
        for knob in &g.knobs {
            match place_state(*knob, &g.place, text) {
                KnobState::Set => {
                    seen.rows
                        .push(SubRow::new(&shown_path, true, "Self-update is off"))
                }
                KnobState::CantRead(why) => {
                    seen.cant_read
                        .get_or_insert_with(|| format!("Can't read {shown_path}: {why}"));
                    seen.rows
                        .push(SubRow::new(&format!("Can't read {shown_path}"), false, why));
                }
                other => {
                    seen.unset.get_or_insert_with(|| {
                        if g.place.is_service() {
                            "Self-update is on in the sd service file".to_string()
                        } else {
                            format!("Self-update is on in {shown_path}")
                        }
                    });
                    seen.rows.push(SubRow::new(
                        &shown_path,
                        false,
                        &unset_text(*knob, &g.place, &other),
                    ));
                }
            }
        }
        if matches!(g.place, Place::ConfigDir(_) | Place::LaneConfigDir(_)) {
            if let Some(v) = minimum_version(text) {
                extra.push(SubRow::new(
                    "minimumVersion",
                    false,
                    &format!("{v} in {shown_path}: Claude Code may still update to it"),
                ));
            }
        }
    }
    (seen, extra)
}

fn extra_rows(head: &str) -> Vec<SubRow> {
    let mut rows = Vec::new();
    for line in head.lines().take(8) {
        match line.trim() {
            "pkgmgr=1" | "pkgmgr=true" => rows.push(SubRow::new(
                "CLAUDE_CODE_PACKAGE_MANAGER_AUTO_UPDATE",
                false,
                "on in the login shell: the package manager may update Claude Code",
            )),
            "pkgmgr=0" | "pkgmgr=false" => rows.push(SubRow::new(
                "CLAUDE_CODE_PACKAGE_MANAGER_AUTO_UPDATE",
                true,
                "off in the login shell",
            )),
            "pkgmgr=other" => rows.push(SubRow::new(
                "CLAUDE_CODE_PACKAGE_MANAGER_AUTO_UPDATE",
                false,
                "set in the login shell to something else",
            )),
            "codex_update_loop=1" => rows.push(SubRow::new(
                "Codex update loop",
                false,
                "running on this worker",
            )),
            _ => {}
        }
    }
    rows
}

fn parse_step(
    tool: Tool,
    groups: &[Group],
    nonce: &str,
    out: &RemoteOutput,
    ctx: &StepContext,
) -> CheckResult {
    let home = home_of(&ctx.machine);
    let segments = split_places(&out.text, nonce);
    if segments.len() != groups.len() + 1 {
        return CheckResult::cant_tell("couldn't read the settings files");
    }
    let (seen, minimum) = read_groups(groups, &segments, nonce, home.as_deref());
    let mut rows = seen.rows;
    rows.extend(minimum);
    rows.extend(extra_rows(&segments[0]));
    let result = if let Some(why) = seen.cant_read {
        CheckResult::fail(why)
    } else if let Some(why) = seen.unset {
        CheckResult::fail(why)
    } else {
        let places = rows
            .iter()
            .filter(|r| r.ok && r.detail == "Self-update is off")
            .count();
        CheckResult::pass(match places {
            0 | 1 => format!("Self-update is off for {}", tool.title()),
            n => format!("Self-update is off for {} in {n} places", tool.title()),
        })
    };
    result.rows(rows)
}

fn display_lines(m: &Machine, home: &str, tool: Tool, on: bool) -> Vec<String> {
    let mut lines = Vec::new();
    for (knob, place) in targets(m, home, tool, None) {
        if place.is_service() {
            continue;
        }
        let shown = if on {
            shown_on(knob, &place)
        } else {
            shown_off(knob, &place)
        };
        let dir = friendly(&place.file(), Some(home));
        let folder = dir.rsplit_once('/').map_or(dir.as_str(), |(d, _)| d);
        lines.push(format!("{folder}/{shown}"));
    }
    lines
}

fn fix_display(m: &Machine, home: &str, tool: Tool, on: bool) -> String {
    let lines = display_lines(m, home, tool, on);
    let head = if on {
        format!(
            "Self-Update On takes back what Self-Update Off added for {}",
            tool.title()
        )
    } else {
        format!(
            "Self-Update Off changes one key for {}, with a backup beside each file",
            tool.title()
        )
    };
    if lines.is_empty() {
        return head;
    }
    format!("{head}:\n{}", lines.join("\n"))
}

fn resolve_codex_home(ctx: &FixContext) -> Result<Option<String>, String> {
    let script = "printf '%s' \"${CODEX_HOME:-}\"";
    let call = Call::secs(shell::ssh_script(&ctx.step.machine, script), 20);
    let r = ctx.step.runner.run(&call);
    if r.code != 0 {
        return Err("couldn't ask the worker where Codex keeps its settings".to_string());
    }
    let value = r.out.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if shell::is_safe_machine_path(value) {
        return Ok(Some(value.to_string()));
    }
    Err("CODEX_HOME isn't a plain path".to_string())
}

fn run_knobs(ctx: &FixContext, tool: Tool, on: bool) -> FixOutcome {
    let m = &ctx.step.machine;
    let Some(home) = home_of(m) else {
        return FixOutcome::fail("the worker's home folder isn't known yet; check again");
    };
    let codex_home = if tool == Tool::Codex {
        match resolve_codex_home(ctx) {
            Ok(path) => path,
            Err(why) => return FixOutcome::fail(why),
        }
    } else {
        None
    };
    let Some(nonce) = nonce() else {
        return FixOutcome::fail("no randomness for the read markers");
    };
    let all = targets(m, &home, tool, codex_home.as_deref());
    let mut changed: Vec<String> = Vec::new();
    let mut waits_for_service = false;
    for (knob, place) in &all {
        let shown = friendly(&place.file(), Some(&home));
        if place.is_service() {
            waits_for_service = true;
            continue;
        }
        let read = read_quiet(ctx, &place.file(), &nonce);
        let Some(text) = read.text() else {
            return stopped(
                format!("Can't read {shown}: {}", read.why().unwrap_or("unreadable")),
                &changed,
            );
        };
        let planned = if on {
            plan_on(*knob, place, text)
        } else {
            plan_off(*knob, place, text)
        };
        match planned {
            Err(why) => return stopped(format!("{shown}: {why}"), &changed),
            Ok(None) => {}
            Ok(Some(plan)) => {
                let outcome = apply(ctx, &plan);
                if !outcome.ok {
                    return stopped(outcome.message, &changed);
                }
                changed.push(shown);
            }
        }
    }
    let word = if on { "on" } else { "off" };
    if changed.is_empty() {
        if waits_for_service && !on {
            return FixOutcome::fail("The sd service file gets its line from the sd Service step");
        }
        return FixOutcome::ok(format!("Self-update was already {word}"));
    }
    FixOutcome::ok(format!("Self-update {word} in {}", changed.join(", ")))
}

fn stopped(why: String, changed: &[String]) -> FixOutcome {
    if changed.is_empty() {
        return FixOutcome::fail(why);
    }
    FixOutcome::fail(format!("{why} (already changed: {})", changed.join(", ")))
}

fn plan_of(m: &Machine, home: &str, tool: Tool, on: bool) -> FixPlan {
    FixPlan::new(fix_display(m, home, tool, on), false, move |ctx| {
        run_knobs(ctx, tool, on)
    })
}

pub fn step(m: &Machine, tool: Tool) -> SetupStep {
    let id = StepId::self_update(tool);
    let title = format!("Self-Update Off: {}", tool.title());
    let home = home_of(m);
    let all = home
        .as_deref()
        .map(|h| targets(m, h, tool, None))
        .unwrap_or_default();
    let dynamic = tool == Tool::Codex;
    let grouped = groups(&all, dynamic);
    let unmanaged = knobs(tool).is_empty();
    let probe = match (&home, unmanaged) {
        (_, true) => Probe::remote("true", |_, _| CheckResult::pass(UNMANAGED)),
        (None, _) => Probe::remote("true", |_, _| {
            CheckResult::cant_tell("the worker's home folder isn't known yet")
        }),
        (Some(_), false) => {
            let marker = nonce().unwrap_or_else(|| "0000000000000000".to_string());
            let script = probe_script(tool, &grouped, &marker);
            Probe::remote(script, move |out, ctx| {
                parse_step(tool, &grouped, &marker, out, ctx)
            })
        }
    };
    let (fix, replace) = match (&home, unmanaged) {
        (Some(h), false) => (
            StepFix::Run(plan_of(m, h, tool, false)),
            Some(plan_of(m, h, tool, true)),
        ),
        _ => (StepFix::HowTo(UNMANAGED.to_string()), None),
    };
    let mut step = SetupStep::new(
        id,
        title,
        vec![StepId::TOOL_VERSIONS],
        Cadence::Slow,
        probe,
        fix,
    );
    step.replace = replace;
    step.hold_when = Some(std::sync::Arc::new(move |d: &Detected| {
        match d.tools.as_ref().and_then(|t| t.tools.get(&tool)) {
            Some(ToolState::NotInstalled) => Some(format!("{} isn't installed here", tool.title())),
            _ => None,
        }
    }));
    step
}

impl SetupEngine {
    pub fn self_update_undo_plan(&self, machine: &str) -> Vec<(StepId, String, String)> {
        let Some(m) = self.machine(machine) else {
            return Vec::new();
        };
        let results = self.results(machine);
        self.step_list(&m)
            .into_iter()
            .filter(|s| s.id.self_update_tool().is_some())
            .filter(|s| results.get(&s.id).is_some_and(|r| r.state.is_pass()))
            .filter_map(|s| {
                let display = s.replace.as_ref()?.display.clone();
                Some((s.id.clone(), s.replace_title(), display))
            })
            .collect()
    }

    pub fn undo_self_update(&self, machine: &str) -> FixOutcome {
        let plan = self.self_update_undo_plan(machine);
        if plan.is_empty() {
            return FixOutcome::ok("Nothing to take back");
        }
        let mut done = Vec::new();
        for (id, title, display) in plan {
            let outcome = self.run_replace(machine, &id, &display);
            if !outcome.ok {
                return FixOutcome::fail(format!("{title}: {}", outcome.message));
            }
            done.push(outcome.message);
        }
        FixOutcome::ok(done.join("\n"))
    }
}
