use super::engine_plan::PlanKind;
use super::registry::{Machine, MachineAbilities};
use super::runner::{CommandResult, Recorded};
use super::step::{
    Cadence, CheckResult, CheckState, Detected, Probe, RemoteOutput, SetupStep, StepFix, StepId,
};
use super::steps_selfupdate::{self, UNMANAGED};
use super::tests_engine_world::{lock, mac, sid, Setup, World};
use crate::versions::knob::sha256_hex;
use crate::versions::{Method, Tool, ToolState, WorkerTools};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

const TOKEN: &str = "sk-ant-oat01-PLANTED";
const HOME: &str = "/home/w";
const CLAUDE: &str = "/home/w/.claude/settings.json";
const CODEX: &str = "/home/w/.codex/config.toml";

#[derive(Clone)]
enum FileState {
    Text(String),
    Unreadable,
}

type Files = Arc<Mutex<BTreeMap<String, FileState>>>;
type Extras = Arc<Mutex<BTreeMap<String, String>>>;

fn settings() -> String {
    format!("{{\n  \"env\": {{\n    \"ANTHROPIC_API_KEY\": \"{TOKEN}\"\n  }}\n}}\n")
}

fn machine() -> Machine {
    let mut m = mac("m1");
    m.abilities = Some(MachineAbilities {
        home: Some(HOME.to_string()),
        ..MachineAbilities::default()
    });
    m
}

fn read_out(path: &str, nonce: &str, files: &Files) -> String {
    match lock_files(files).get(path) {
        None => format!("path={path}\nmissing=1\n"),
        Some(FileState::Unreadable) => format!("path={path}\nunreadable=1\n"),
        Some(FileState::Text(t)) => format!(
            "path={path}\nsize={}\nsha256={}\n@@begin {nonce}\n{t}\n@@end {nonce}\n",
            t.len(),
            sha256_hex(t)
        ),
    }
}

fn lock_files(files: &Files) -> std::sync::MutexGuard<'_, BTreeMap<String, FileState>> {
    files.lock().unwrap_or_else(|e| e.into_inner())
}

fn after(text: &str, marker: &str, len: usize) -> String {
    let at = text.find(marker).map_or(0, |i| i + marker.len());
    text[at..].chars().take(len).collect()
}

fn probe_text(probe: &str, files: &Files, extras: &Extras) -> String {
    let nonce = after(probe, "@@place ", 16);
    let (tool, path) = if probe.contains("settings.json") {
        ("claude", CLAUDE)
    } else {
        ("codex", CODEX)
    };
    let head = lock(extras).get(tool).cloned().unwrap_or_default();
    format!("{head}@@place {nonce} 0\n{}", read_out(path, &nonce, files))
}

struct Rig {
    world: World,
    files: Files,
    writes: Arc<Mutex<Vec<(String, String)>>>,
    data: super::fakes::TempDir,
}

fn batch_nonce(script: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = script[from..].find("@@") {
        let at = from + i + 2;
        let candidate: String = script[at..].chars().take(16).collect();
        let rest = &script[at + candidate.len()..];
        if candidate.len() == 16
            && candidate.chars().all(|c| c.is_ascii_hexdigit())
            && rest.starts_with(' ')
            && rest[1..]
                .trim_start_matches(|c: char| c.is_ascii_digit())
                .starts_with(" %s@@")
        {
            return Some(candidate);
        }
        from = at;
    }
    None
}

fn recovered(call: &super::runner::Call) -> String {
    call.argv
        .last()
        .cloned()
        .unwrap_or_default()
        .replace("'\\''", "'")
}

fn word(script: &str) -> String {
    script
        .lines()
        .find_map(|l| l.strip_prefix("f="))
        .unwrap_or("")
        .trim_matches(|c| c == '\'' || c == '"')
        .to_string()
}

fn installed(tools: &[(Tool, bool)]) -> WorkerTools {
    let mut map = BTreeMap::new();
    for (tool, here) in tools {
        map.insert(
            *tool,
            if *here {
                ToolState::Installed {
                    version: None,
                    method: Method::Native,
                    path: "/x".to_string(),
                }
            } else {
                ToolState::NotInstalled
            },
        );
    }
    WorkerTools {
        tools: map,
        at_ms: 1,
    }
}

fn rig_with(tools: WorkerTools, files: &[(&str, FileState)]) -> Rig {
    let files: Files = Arc::new(Mutex::new(
        files
            .iter()
            .map(|(p, s)| (p.to_string(), s.clone()))
            .collect(),
    ));
    let extras: Extras = Arc::new(Mutex::new(BTreeMap::new()));
    let writes = Arc::new(Mutex::new(Vec::new()));
    let t = tools.clone();
    let mut setup = Setup::new(move |m, _| {
        let t = t.clone();
        let mut list = vec![SetupStep::new(
            StepId::TOOL_VERSIONS,
            "Tool Versions",
            Vec::new(),
            Cadence::EveryRefresh,
            Probe::local(move |_| {
                CheckResult::pass("tools").detected(Detected {
                    tools: Some(t.clone()),
                    ..Detected::default()
                })
            }),
            StepFix::HowTo("read only".into()),
        )];
        list.extend(steps_selfupdate::steps(m));
        list
    });
    setup.machines = vec![machine()];
    let data = super::fakes::TempDir::new("selfupdate-data");
    setup.offers_data = Some(data.path().to_path_buf());
    let (f, e) = (files.clone(), extras.clone());
    setup.runner.on_call(
        |argv| {
            argv.last()
                .is_some_and(|s| s.contains("out=$( /bin/sh -c ") && s.contains("@@place"))
        },
        move |call| {
            let script = call.argv.last().cloned().unwrap_or_default();
            let Some(nonce) = batch_nonce(&script) else {
                return CommandResult::ok("");
            };
            let mut out = String::new();
            for (i, part) in script.split("out=$( /bin/sh -c ").skip(1).enumerate() {
                out.push_str(&format!(
                    "{}\n@@{nonce} {i} 0@@\n",
                    probe_text(part, &f, &e).trim_end_matches('\n')
                ));
            }
            CommandResult::ok(&out)
        },
    );
    let f = files.clone();
    setup.runner.on_call(
        |argv| {
            argv.last()
                .is_some_and(|s| s.contains("echo path=$f") && !s.contains("out=$( /bin/sh -c "))
        },
        move |call| {
            let script = recovered(call);
            let nonce = after(&script, "echo '@@begin ", 16);
            CommandResult::ok(&read_out(&word(&script), &nonce, &f))
        },
    );
    let (f, w) = (files.clone(), writes.clone());
    setup.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("notchling-backup")),
        move |call| {
            let path = word(&recovered(call));
            let stdin = String::from_utf8_lossy(call.stdin.as_deref().unwrap_or(b"")).into_owned();
            let (want, text) = stdin.split_once('\n').unwrap_or(("", ""));
            let have = match lock_files(&f).get(&path) {
                Some(FileState::Text(t)) => sha256_hex(t),
                Some(FileState::Unreadable) => "unreadable".to_string(),
                None => sha256_hex(""),
            };
            if have != want {
                return CommandResult::new(3, "why=changed_since_plan\n", "");
            }
            lock_files(&f).insert(path.clone(), FileState::Text(text.to_string()));
            lock(&w).push((path, text.to_string()));
            CommandResult::ok("ok=1\n")
        },
    );
    let world = setup.build();
    Rig {
        world,
        files,
        writes,
        data,
    }
}

fn rig(files: &[(&str, FileState)]) -> Rig {
    rig_with(
        installed(&[
            (Tool::Claude, true),
            (Tool::Codex, true),
            (Tool::Agy, false),
        ]),
        files,
    )
}

fn claude_id() -> StepId {
    StepId::self_update(Tool::Claude)
}

fn state(r: &Rig, tool: Tool) -> CheckState {
    r.world
        .engine
        .results("m1")
        .get(&StepId::self_update(tool))
        .map(|c| c.state.clone())
        .unwrap_or_else(|| panic!("no result for {tool:?}"))
}

fn text_of(r: &Rig, path: &str) -> String {
    match lock_files(&r.files).get(path) {
        Some(FileState::Text(t)) => t.clone(),
        _ => String::new(),
    }
}

fn run_fix(r: &Rig, tool: Tool) -> super::step::FixOutcome {
    r.world.click(
        "m1",
        PlanKind::Fix(StepId::self_update(tool)),
        super::engine_plan::AdminAnswer::None,
    )
}

#[test]
fn off_sets_every_known_place() {
    let r = rig(&[
        (CLAUDE, FileState::Text(settings())),
        (CODEX, FileState::Text("model = \"x\"\n".into())),
    ]);
    r.world.refresh("m1", true);
    assert_eq!(
        state(&r, Tool::Claude),
        CheckState::Fail("Self-update is on in ~/.claude/settings.json".into())
    );
    assert_eq!(
        state(&r, Tool::Codex),
        CheckState::Fail("Self-update is on in ~/.codex/config.toml".into())
    );
    let claude = run_fix(&r, Tool::Claude);
    assert!(claude.ok, "{}", claude.message);
    assert_eq!(claude.message, "Self-update off in ~/.claude/settings.json");
    let codex = run_fix(&r, Tool::Codex);
    assert!(codex.ok, "{}", codex.message);
    assert!(text_of(&r, CLAUDE).contains("\"DISABLE_AUTOUPDATER\": \"1\""));
    assert!(text_of(&r, CLAUDE).contains(TOKEN));
    assert_eq!(
        text_of(&r, CODEX),
        "model = \"x\"\ncheck_for_update_on_startup = false\n"
    );
    r.world.refresh("m1", true);
    assert_eq!(
        state(&r, Tool::Claude),
        CheckState::Pass("Self-update is off for Claude Code".into())
    );
    assert_eq!(
        state(&r, Tool::Codex),
        CheckState::Pass("Self-update is off for Codex".into())
    );
    assert_eq!(lock(&r.writes).len(), 2);
}

#[test]
fn a_missing_codex_config_is_created() {
    let r = rig(&[(CLAUDE, FileState::Text(settings()))]);
    r.world.refresh("m1", true);
    let outcome = run_fix(&r, Tool::Codex);
    assert!(outcome.ok, "{}", outcome.message);
    assert_eq!(text_of(&r, CODEX), "check_for_update_on_startup = false\n");
}

#[test]
fn already_set_runs_no_write() {
    let on = "{\"env\": {\"DISABLE_AUTOUPDATER\": \"1\"}}";
    let r = rig(&[
        (CLAUDE, FileState::Text(on.into())),
        (
            CODEX,
            FileState::Text("check_for_update_on_startup = false\n".into()),
        ),
    ]);
    r.world.refresh("m1", true);
    assert!(state(&r, Tool::Claude).is_pass());
    let outcome = run_fix(&r, Tool::Claude);
    assert!(outcome.ok);
    assert_eq!(outcome.message, "Self-update was already off");
    assert!(lock(&r.writes).is_empty());
}

#[test]
fn unreadable_place_fails_the_step() {
    let r = rig(&[
        (CLAUDE, FileState::Unreadable),
        (CODEX, FileState::Text("x = 1\n".into())),
    ]);
    r.world.refresh("m1", true);
    let CheckState::Fail(text) = state(&r, Tool::Claude) else {
        panic!("{:?}", state(&r, Tool::Claude));
    };
    assert_eq!(text, "Can't read ~/.claude/settings.json: unreadable");
    let rows = &r.world.engine.results("m1")[&claude_id()].rows;
    assert!(rows
        .iter()
        .any(|row| row.name == "Can't read ~/.claude/settings.json" && !row.ok));
    let outcome = run_fix(&r, Tool::Claude);
    assert!(!outcome.ok);
    assert!(lock(&r.writes).is_empty());
}

#[test]
fn on_is_its_own_click_and_not_in_set_up_all() {
    let r = rig(&[
        (CLAUDE, FileState::Text(settings())),
        (CODEX, FileState::Text("x = 1\n".into())),
    ]);
    r.world.refresh("m1", true);
    let m = machine();
    let all = r.world.engine.set_up_all_steps(&m);
    assert!(all.iter().any(|a| a.id == claude_id()));
    assert!(all.iter().all(|a| !a.display.contains("takes back")));
    assert!(run_fix(&r, Tool::Claude).ok);
    r.world.refresh("m1", true);
    let again = r.world.engine.set_up_all_steps(&m);
    assert!(again.iter().all(|a| a.id != claude_id()));
    let back = r.world.click(
        "m1",
        PlanKind::Replace(claude_id()),
        super::engine_plan::AdminAnswer::None,
    );
    assert!(back.ok, "{}", back.message);
    assert_eq!(text_of(&r, CLAUDE), settings());
}

#[test]
fn remove_machine_shows_and_runs_self_update_on() {
    let r = rig(&[
        (CLAUDE, FileState::Text(settings())),
        (CODEX, FileState::Text("model = \"x\"\n".into())),
    ]);
    r.world.refresh("m1", true);
    assert!(r.world.engine.self_update_undo_plan("m1").is_empty());
    assert!(run_fix(&r, Tool::Claude).ok);
    assert!(run_fix(&r, Tool::Codex).ok);
    r.world.refresh("m1", true);
    let plan = r.world.engine.self_update_undo_plan("m1");
    assert_eq!(plan.len(), 2);
    assert_eq!(plan[0].1, "Self-Update On: Claude Code");
    assert!(plan[0].2.contains("remove \"DISABLE_AUTOUPDATER\": \"1\""));
    assert!(plan[1]
        .2
        .contains("remove check_for_update_on_startup = false"));
    let outcome = r.world.engine.undo_self_update("m1");
    assert!(outcome.ok, "{}", outcome.message);
    assert_eq!(text_of(&r, CLAUDE), settings());
    assert_eq!(text_of(&r, CODEX), "model = \"x\"\n");
    assert!(r.world.engine.undo_self_update("m1").ok);
}

#[test]
fn a_users_own_value_is_not_taken_back() {
    let zero = "{\"env\": {\"DISABLE_AUTOUPDATER\": \"0\"}}";
    let r = rig(&[
        (CLAUDE, FileState::Text(zero.into())),
        (
            CODEX,
            FileState::Text("check_for_update_on_startup = true\n".into()),
        ),
    ]);
    r.world.refresh("m1", true);
    assert!(r.world.engine.self_update_undo_plan("m1").is_empty());
    assert!(lock(&r.writes).is_empty());
}

fn parse_with(tool: Tool, files: &[(&str, FileState)], head: &str) -> CheckResult {
    let m = machine();
    let step = steps_selfupdate::step(&m, tool);
    let Probe::Remote { script, parse } = &step.probe else {
        panic!("no probe");
    };
    let files: Files = Arc::new(Mutex::new(
        files
            .iter()
            .map(|(p, s)| (p.to_string(), s.clone()))
            .collect(),
    ));
    let extras: Extras = Arc::new(Mutex::new(BTreeMap::new()));
    lock(&extras).insert(tool.name().to_string(), head.to_string());
    let text = probe_text(script, &files, &extras);
    let ctx = super::fakes::Ctx::new(m);
    parse(&RemoteOutput { text, code: 0 }, &ctx.step())
}

#[test]
fn package_manager_auto_update_is_reported() {
    let on = "{\"env\": {\"DISABLE_AUTOUPDATER\": \"1\"}}";
    let result = parse_with(
        Tool::Claude,
        &[(CLAUDE, FileState::Text(on.into()))],
        "pkgmgr=1\n",
    );
    assert!(result.state.is_pass(), "{:?}", result.state);
    let row = result
        .rows
        .iter()
        .find(|r| r.name == "CLAUDE_CODE_PACKAGE_MANAGER_AUTO_UPDATE")
        .expect("a row");
    assert!(!row.ok);
    let off = parse_with(
        Tool::Claude,
        &[(CLAUDE, FileState::Text(on.into()))],
        "pkgmgr=other\n",
    );
    assert!(off.rows.iter().any(|r| r.detail.contains("something else")));
}

#[test]
fn minimum_version_is_reported() {
    let text = "{\"minimumVersion\": \"2.1.300\", \"env\": {\"DISABLE_AUTOUPDATER\": \"1\"}}";
    let result = parse_with(Tool::Claude, &[(CLAUDE, FileState::Text(text.into()))], "");
    let row = result
        .rows
        .iter()
        .find(|r| r.name == "minimumVersion")
        .expect("a row");
    assert_eq!(
        row.detail,
        "2.1.300 in ~/.claude/settings.json: Claude Code may still update to it"
    );
    let junk = "{\"minimumVersion\": \"x;rm -rf\", \"env\": {\"DISABLE_AUTOUPDATER\": \"1\"}}";
    let none = parse_with(Tool::Claude, &[(CLAUDE, FileState::Text(junk.into()))], "");
    assert!(none.rows.iter().all(|r| r.name != "minimumVersion"));
}

#[test]
fn codex_update_loop_is_reported() {
    let config = "check_for_update_on_startup = false\n";
    let result = parse_with(
        Tool::Codex,
        &[(CODEX, FileState::Text(config.into()))],
        "codex_update_loop=1\n",
    );
    assert!(result.state.is_pass());
    assert!(result
        .rows
        .iter()
        .any(|r| r.name == "Codex update loop" && !r.ok));
}

#[test]
fn the_probe_names_no_file_content() {
    let result = parse_with(Tool::Claude, &[(CLAUDE, FileState::Text(settings()))], "");
    let shown = format!("{:?}", result);
    assert!(!shown.contains(TOKEN));
    assert!(!shown.contains("ANTHROPIC"));
}

#[test]
fn not_installed_tool_has_no_step() {
    let r = rig(&[(CLAUDE, FileState::Text(settings()))]);
    r.world.refresh("m1", true);
    let ids: Vec<String> = r
        .world
        .engine
        .results("m1")
        .keys()
        .map(|k| k.raw().to_string())
        .collect();
    assert!(ids.contains(&"self-update:claude".to_string()));
    assert!(ids.contains(&"self-update:codex".to_string()));
    let listed: Vec<String> = r
        .world
        .engine
        .self_update_undo_plan("m1")
        .into_iter()
        .map(|p| p.0.raw().to_string())
        .collect();
    assert!(listed.is_empty());
    let gone = rig_with(
        installed(&[
            (Tool::Claude, true),
            (Tool::Codex, false),
            (Tool::Agy, false),
        ]),
        &[(CLAUDE, FileState::Text(settings()))],
    );
    gone.world.refresh("m1", true);
    gone.world.refresh("m1", true);
    let list: Vec<String> = gone
        .world
        .engine
        .step_list(&machine())
        .into_iter()
        .map(|s| s.id.raw().to_string())
        .collect();
    assert!(list.contains(&"self-update:claude".to_string()));
    assert!(!list.contains(&"self-update:codex".to_string()));
    assert!(!list.contains(&"self-update:antigravity".to_string()));
}

#[test]
fn agy_is_detect_and_repair_until_verified() {
    let r = rig_with(
        installed(&[(Tool::Claude, true), (Tool::Codex, true), (Tool::Agy, true)]),
        &[],
    );
    r.world.refresh("m1", true);
    assert_eq!(
        state(&r, Tool::Agy),
        CheckState::Pass(UNMANAGED.to_string())
    );
    let step = steps_selfupdate::step(&machine(), Tool::Agy);
    assert!(step.replace.is_none());
    assert!(matches!(step.fix, StepFix::HowTo(_)));
    assert!(!super::steps_versions::TOOL_VERSIONS_SCRIPT.contains("agy --version"));
    assert!(steps_selfupdate::knobs(Tool::Agy).is_empty());
    assert!(steps_selfupdate::targets(&machine(), HOME, Tool::Agy, None).is_empty());
}

#[test]
fn refresh_runs_only_reads() {
    let r = rig(&[
        (CLAUDE, FileState::Text(settings())),
        (CODEX, FileState::Text("x = 1\n".into())),
    ]);
    r.world.refresh("m1", true);
    r.world.refresh("m1", true);
    let calls: Vec<Recorded> = r.world.runner.calls();
    assert!(!calls.is_empty());
    for call in calls {
        let script = call.argv.last().cloned().unwrap_or_default();
        for word in [
            " mv ",
            "cp ",
            "tee ",
            "notchling-backup",
            "notchling-tmp",
            " > \"",
            ">>",
        ] {
            assert!(!script.contains(word), "{word} in {script}");
        }
        assert!(call.stdin.is_none());
    }
    assert!(lock(&r.writes).is_empty());
}

#[test]
fn the_plan_shows_one_line_per_place_and_no_content() {
    let m = machine();
    let step = steps_selfupdate::step(&m, Tool::Claude);
    let plan = step.plan().expect("a fix").display.clone();
    assert_eq!(
        plan,
        "Self-Update Off changes one key for Claude Code, with a backup beside each file:\n~/.claude/settings.json: \"env\": {\"DISABLE_AUTOUPDATER\": \"1\"}"
    );
    let codex = steps_selfupdate::step(&m, Tool::Codex);
    assert!(codex
        .plan()
        .unwrap()
        .display
        .ends_with("~/.codex/config.toml: check_for_update_on_startup = false"));
    let back = step.replace.as_ref().expect("a replace").display.clone();
    assert!(
        back.starts_with("Self-Update On takes back what Self-Update Off added for Claude Code")
    );
    assert!(!plan.contains(TOKEN));
}

#[test]
fn places_follow_the_worker() {
    let mut m = machine();
    let claude = steps_selfupdate::places(&m, HOME, Tool::Claude);
    assert_eq!(claude.len(), 1);
    assert_eq!(claude[0].file(), CLAUDE);
    let codex = steps_selfupdate::places_with(&m, HOME, Tool::Codex, Some("/srv/codex"));
    assert_eq!(codex[0].file(), "/srv/codex/config.toml");
    m.set_sd_worker(Some(super::registry::SdWorkerSettings::new("m1")));
    let with_service = steps_selfupdate::places(&m, HOME, Tool::Claude);
    assert_eq!(with_service.len(), 2);
    assert!(with_service[1].is_service());
    let bad = steps_selfupdate::places(&m, "/home/w x", Tool::Claude);
    assert!(bad.is_empty());
}

#[test]
fn the_step_id_names_its_tool() {
    assert_eq!(
        StepId::self_update(Tool::Agy).raw(),
        "self-update:antigravity"
    );
    assert_eq!(
        StepId::self_update(Tool::Codex).self_update_tool(),
        Some(Tool::Codex)
    );
    assert_eq!(sid("claude-login").self_update_tool(), None);
}

#[test]
fn each_fix_is_logged_without_file_content() {
    let r = rig(&[
        (CLAUDE, FileState::Text(settings())),
        (CODEX, FileState::Text("x = 1\n".into())),
    ]);
    r.world.refresh("m1", true);
    assert!(run_fix(&r, Tool::Claude).ok);
    let back = r.world.click(
        "m1",
        PlanKind::Replace(claude_id()),
        super::engine_plan::AdminAnswer::None,
    );
    assert!(back.ok, "{}", back.message);
    let log = crate::versions::selfupdate_log::unrecorded(r.data.path());
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].action, "off");
    assert_eq!(log[1].action, "on");
    assert!(log
        .iter()
        .all(|e| e.ok && e.tool == Tool::Claude && e.machine == "m1"));
    assert!(
        log[0].detail.contains("~/.claude/settings.json"),
        "{}",
        log[0].detail
    );
    for e in &log {
        assert!(!e.detail.contains(TOKEN));
        assert!(!e.detail.contains("DISABLE_AUTOUPDATER"), "{}", e.detail);
    }
}
