use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use notchling_engine::machines::registry::{Machine, MachineOs, NodeSettings};
use notchling_engine::machines::runner::CommandResult;
use notchling_engine::machines::step::FixOutcome;
use notchling_engine::machines::tool_login::LoginCheck;
use notchling_engine::versions::{Method, Tool, ToolState, Version, WorkerTools};
use serde_json::{json, Value};

use super::add_rig_tests::Rig;
use super::feed::ClusterFeed;
use super::model::{ClusterModel, Login};
use super::tool_runs::ToolRuns;
use super::tool_setup::{
    correct, find, idle_now, idle_text, run_check, run_install, run_sign_in, settle_check,
    settle_sign_in, setups, start_check, start_install, start_sign_in, CheckGo, Corrections,
    InstallGo, Kind, Marks, SignInGo, FRESH_MS,
};
use super::tool_updates::{offers, State};
use crate::machines::versions_feed::{MacSide, MacTool};
use crate::switches::Switches;

const ME: &str = "a1b2c3d4e5f60718";
const M1: &str = "1122334455667788";
const NOW: i64 = 1_800_000_000_000;

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn member(id: &str, name: &str, tools: Value, logins: Value) -> Value {
    json!({
        "id": id, "name": name, "role": "worker", "canBeMaster": false,
        "online": true, "client": false, "headless": true, "reference": false,
        "state": "active",
        "versions": {
            "notchling": "0.9.0", "proto": 1, "features": ["dispatch"], "schema": 1,
            "os": "linux", "arch": "x86_64", "tools": tools, "logins": logins,
        },
    })
}

fn model(worker_tools: Value, logins: Value, jobs: Value) -> ClusterModel {
    let mut me = member(
        ME,
        "m5",
        json!({"claude": "2.1.327", "codex": "0.166.0", "antigravity": "1.5.9"}),
        json!({}),
    );
    me["role"] = json!("home");
    me["reference"] = json!(true);
    let status = json!({
        "cluster": {"id": "c1", "name": "home", "term": 1, "home": ME},
        "me": ME,
        "members": [me, member(M1, "m1", worker_tools, logins)],
        "jobs": jobs,
        "link": true, "tailscale": true, "listening": [47823],
    });
    ClusterModel::from_status(&status, ME)
}

fn machine() -> Machine {
    let mut m = Machine::new("m1", "m1.test", "worker");
    m.os = Some(MachineOs::Linux);
    m.node = Some(NodeSettings::default());
    m
}

fn mac(codex: Option<&str>, agy: Option<&str>) -> MacSide {
    let tool = |version: Option<&str>| MacTool {
        installed: version.map(v),
        method: Method::Native,
        reference: version.map(v),
        usable: version.is_some(),
        installed_failed: false,
    };
    let mut tools = BTreeMap::new();
    tools.insert(Tool::Claude, tool(Some("2.1.327")));
    tools.insert(Tool::Codex, tool(codex));
    tools.insert(Tool::Agy, tool(agy));
    MacSide { tools }
}

fn none(_: &str) -> Option<String> {
    None
}

fn unread(_: &str) -> Option<WorkerTools> {
    None
}

fn read(entries: Vec<(Tool, ToolState)>, at_ms: i64) -> WorkerTools {
    WorkerTools {
        tools: entries.into_iter().collect(),
        at_ms,
    }
}

fn never(_: &str, _: Tool) -> Option<(i64, bool)> {
    None
}

fn nothing(_: &str, _: Tool) -> Option<(i64, Option<Version>)> {
    None
}

fn unreported(_: &str, _: Tool) -> Option<i64> {
    None
}

fn fixed(model: &ClusterModel, tools: &dyn Fn(&str) -> Option<WorkerTools>) -> ClusterModel {
    let marks = Marks {
        login: &never,
        installed: &nothing,
        reported: &unreported,
    };
    correct(model, &[machine()], &none, tools, &marks, NOW)
}

fn offered(
    model: &ClusterModel,
    side: Option<&MacSide>,
    sw: &Switches,
) -> Vec<super::tool_setup::Setup> {
    setups(model, &[machine()], &none, side, sw)
}

fn on() -> ToolState {
    ToolState::Installed {
        version: Some(v("0.118.0")),
        method: Method::Standalone,
        path: "/home/w/.local/bin/codex".into(),
    }
}

fn stale() -> ClusterModel {
    model(
        json!({"claude": "2.1.327", "codex": "0.118.0"}),
        json!({}),
        json!([]),
    )
}

fn missing_both() -> ClusterModel {
    model(json!({"claude": "2.1.327"}), json!({}), json!([]))
}

fn row_of(m: &ClusterModel) -> super::model::NodeRow {
    m.row(M1).unwrap().clone()
}

#[test]
fn a_codex_the_node_reports_but_the_worker_does_not_have_shows_not_installed() {
    let before = row_of(&stale());
    assert!(before.tool_texts().iter().any(|t| t == "Codex 0.118.0"));
    assert!(!before.drifts.is_empty());
    let known = read(
        vec![(Tool::Codex, ToolState::NotInstalled), (Tool::Claude, on())],
        NOW - 1000,
    );
    let tools = move |_: &str| Some(known.clone());
    let after = row_of(&fixed(&stale(), &tools));
    let texts = after.tool_texts();
    assert!(
        texts.iter().any(|t| t == "Codex not installed"),
        "{texts:?}"
    );
    assert!(!texts.iter().any(|t| t.contains("0.118.0")), "{texts:?}");
    assert!(after
        .login_texts()
        .iter()
        .any(|(t, l)| t == "Codex: Not Installed" && *l == Login::NotInstalled));
    assert!(after.drifts.iter().all(|d| d.tool != Tool::Codex));
    assert!(
        after.warnings.iter().all(|w| !w.contains("Codex")),
        "{:?}",
        after.warnings
    );
}

#[test]
fn the_stale_fix_needs_a_fresh_read_and_changes_nothing_else() {
    let known = read(
        vec![(Tool::Codex, ToolState::NotInstalled)],
        NOW - FRESH_MS - 1,
    );
    let tools = move |_: &str| Some(known.clone());
    let old = fixed(&stale(), &tools);
    assert_eq!(row_of(&old), row_of(&stale()));
    let known = read(vec![(Tool::Codex, on())], NOW - 10);
    let tools = move |_: &str| Some(known.clone());
    assert_eq!(row_of(&fixed(&stale(), &tools)), row_of(&stale()));
    assert_eq!(row_of(&fixed(&stale(), &unread)), row_of(&stale()));
}

#[test]
fn a_corrected_row_gets_an_install_offer_and_no_update_offer() {
    let known = read(vec![(Tool::Codex, ToolState::NotInstalled)], NOW - 10);
    let tools = move |_: &str| Some(known.clone());
    let model = fixed(&stale(), &tools);
    let side = mac(Some("0.166.0"), None);
    let sw = Switches::all_on();
    let drift = offers(&model, &[machine()], &none, &unread, Some(&side), &sw);
    assert!(drift.iter().all(|o| o.tool != Tool::Codex), "{drift:?}");
    let list = offered(&model, Some(&side), &sw);
    let codex = find(&list, M1, Tool::Codex, Kind::Install).unwrap();
    assert_eq!(codex.button(), "Install Codex on m1");
    assert_eq!(codex.state, State::Ready);
    assert_eq!(codex.mac, Some(v("0.166.0")));
    assert!(codex.note.is_none());
}

#[test]
fn a_missing_tool_offers_an_install_and_says_when_it_cannot_pin() {
    let side = mac(Some("0.166.0"), Some("1.5.9"));
    let list = offered(&missing_both(), Some(&side), &Switches::all_on());
    let codex = find(&list, M1, Tool::Codex, Kind::Install).unwrap();
    let agy = find(&list, M1, Tool::Agy, Kind::Install).unwrap();
    assert_eq!(codex.button(), "Install Codex on m1");
    assert_eq!(agy.button(), "Install Antigravity on m1");
    assert!(agy.note.as_deref().unwrap().contains("no exact version"));
    assert!(agy.ready() && codex.ready());
    let none_checked = offered(&missing_both(), None, &Switches::all_on());
    let codex = find(&none_checked, M1, Tool::Codex, Kind::Install).unwrap();
    assert!(codex.note.as_deref().unwrap().contains("newest"));
    assert!(codex.mac.is_none());
}

#[test]
fn a_reference_that_is_not_plain_gets_a_how_to_not_a_button() {
    let side = mac(Some("0.170.0-alpha.1"), None);
    let list = offered(&missing_both(), Some(&side), &Switches::all_on());
    let codex = find(&list, M1, Tool::Codex, Kind::Install).unwrap();
    assert!(matches!(&codex.state, State::HowTo(text) if text.contains("isn't a version")));
    assert!(!codex.ready());
}

#[test]
fn the_install_switch_and_running_jobs_hold_the_install_back() {
    let side = mac(Some("0.166.0"), None);
    let off = offered(&missing_both(), Some(&side), &Switches::all_off());
    let codex = find(&off, M1, Tool::Codex, Kind::Install).unwrap();
    assert!(matches!(&codex.state, State::Off(text) if text.contains("install is off")));
    let jobs = json!([{"id": "j1", "node": M1, "state": "running", "tool": "claude"}]);
    let busy = model(json!({"claude": "2.1.327"}), json!({}), jobs);
    let list = offered(&busy, Some(&side), &Switches::all_on());
    let codex = find(&list, M1, Tool::Codex, Kind::Install).unwrap();
    assert!(matches!(&codex.state, State::Busy(text) if text.contains("running a job")));
    let why = start_install(&list, M1, Tool::Codex, &busy, &Switches::all_on()).unwrap_err();
    assert!(why.contains("running a job"), "{why}");
}

#[test]
fn start_install_rechecks_the_switch_and_gives_the_target() {
    let side = mac(Some("0.166.0"), None);
    let model = missing_both();
    let list = offered(&model, Some(&side), &Switches::all_on());
    let go = start_install(&list, M1, Tool::Codex, &model, &Switches::all_on()).unwrap();
    assert_eq!(
        go,
        InstallGo {
            node: M1.into(),
            machine: "m1".into(),
            tool: Tool::Codex,
            mac: Some(v("0.166.0"))
        }
    );
    let why = start_install(&list, M1, Tool::Codex, &model, &Switches::all_off()).unwrap_err();
    assert!(why.contains("install is off"), "{why}");
    assert!(start_install(&list, "nobody", Tool::Codex, &model, &Switches::all_on()).is_err());
    assert!(start_install(&list, M1, Tool::Claude, &model, &Switches::all_on()).is_err());
}

fn signed(logins: Value) -> ClusterModel {
    model(
        json!({"claude": "2.1.327", "codex": "0.166.0", "antigravity": "1.5.9"}),
        logins,
        json!([]),
    )
}

#[test]
fn an_installed_tool_that_is_not_signed_in_offers_a_sign_in() {
    let side = mac(Some("0.166.0"), Some("1.5.9"));
    let sw = Switches::all_on();
    let both = json!({"codex": false, "antigravity": false});
    let out = offered(&signed(both), Some(&side), &sw);
    let codex = find(&out, M1, Tool::Codex, Kind::SignIn).unwrap();
    assert_eq!(codex.button(), "Sign In on m1");
    assert!(codex.ready());
    let agy = find(&out, M1, Tool::Agy, Kind::SignIn).unwrap();
    assert_eq!(agy.button(), "Sign In on m1");
    let unknown = offered(&signed(json!({})), Some(&side), &sw);
    assert!(find(&unknown, M1, Tool::Codex, Kind::SignIn).is_none());
    let check = find(&unknown, M1, Tool::Codex, Kind::Check).unwrap();
    assert_eq!(check.button(), "Check Sign-In on m1");
    assert!(check.ready());
    let done = offered(
        &signed(json!({"codex": true, "antigravity": true})),
        Some(&side),
        &sw,
    );
    assert!(done.is_empty(), "{done:?}");
}

#[test]
fn a_missing_tool_has_no_sign_in_and_an_installed_one_no_install() {
    let side = mac(Some("0.166.0"), Some("1.5.9"));
    let list = offered(&missing_both(), Some(&side), &Switches::all_on());
    assert!(list.iter().all(|s| s.kind == Kind::Install));
    let full = offered(&signed(json!({})), Some(&side), &Switches::all_on());
    assert!(full.iter().all(|s| s.kind == Kind::Check));
}

#[test]
fn the_accounts_switch_gates_the_sign_in() {
    let side = mac(Some("0.166.0"), None);
    let model = signed(json!({"codex": false}));
    let off = offered(&model, Some(&side), &Switches::all_off());
    let codex = find(&off, M1, Tool::Codex, Kind::SignIn).unwrap();
    assert!(matches!(&codex.state, State::Off(text) if text.contains("accounts are off")));
    let why = start_sign_in(&off, M1, Tool::Codex, &model, &Switches::all_off()).unwrap_err();
    assert!(why.contains("accounts are off"), "{why}");
    let list = offered(&model, Some(&side), &Switches::all_on());
    let go = start_sign_in(&list, M1, Tool::Codex, &model, &Switches::all_on()).unwrap();
    assert_eq!(
        go,
        SignInGo {
            node: M1.into(),
            machine: "m1".into(),
            tool: Tool::Codex
        }
    );
    assert!(start_sign_in(&list, M1, Tool::Claude, &model, &Switches::all_on()).is_err());
}

#[test]
fn a_finished_install_and_a_finished_sign_in_show_on_the_card_at_once() {
    let side = mac(Some("0.166.0"), None);
    let sw = Switches::all_on();
    let installed =
        |_: &str, tool: Tool| (tool == Tool::Codex).then(|| (NOW - 10, Some(v("0.166.0"))));
    let marks = Marks {
        login: &never,
        installed: &installed,
        reported: &unreported,
    };
    let model = correct(&missing_both(), &[machine()], &none, &unread, &marks, NOW);
    let row = row_of(&model);
    assert!(row.tool_texts().iter().any(|t| t == "Codex 0.166.0"));
    assert_eq!(row.login_of(Tool::Codex), Some(Login::Unknown));
    let list = offered(&model, Some(&side), &sw);
    assert!(find(&list, M1, Tool::Codex, Kind::Install).is_none());
    assert!(find(&list, M1, Tool::Codex, Kind::Check).is_some());
    let yes = |_: &str, tool: Tool| (tool == Tool::Codex).then_some((NOW - 5, true));
    let marks = Marks {
        login: &yes,
        installed: &installed,
        reported: &unreported,
    };
    let model = correct(&missing_both(), &[machine()], &none, &unread, &marks, NOW);
    assert_eq!(row_of(&model).login_of(Tool::Codex), Some(Login::SignedIn));
    let list = offered(&model, Some(&side), &sw);
    assert!(find(&list, M1, Tool::Codex, Kind::SignIn).is_none());
    assert!(find(&list, M1, Tool::Codex, Kind::Check).is_none());
}

#[test]
fn a_row_for_this_mac_or_an_offline_node_gets_no_offer() {
    let side = mac(Some("0.166.0"), None);
    let mut status = json!({
        "cluster": {"id": "c1", "name": "home", "term": 1, "home": ME},
        "me": ME,
        "members": [member(M1, "m1", json!({"claude": "2.1.327"}), json!({}))],
        "jobs": [], "link": true, "tailscale": true, "listening": [47823],
    });
    status["members"][0]["online"] = json!(false);
    let model = ClusterModel::from_status(&status, ME);
    assert!(offered(&model, Some(&side), &Switches::all_on()).is_empty());
}

const PROBE: &str = "for t in claude codex agy notchling";

struct Worker {
    codex: Mutex<Option<String>>,
    commands: Mutex<Vec<String>>,
}

fn rig(worker: &Arc<Worker>, installs: bool) -> Rig {
    let rig = Rig::new(vec![machine()]);
    rig.runner.on_call(
        |argv| {
            argv.last()
                .is_some_and(|s| s.contains("cluster status --json"))
        },
        |_| CommandResult::ok(&format!("{{\"me\":\"{M1}\"}}\n")),
    );
    rig.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("busy=")),
        |_| CommandResult::ok("busy=0\n"),
    );
    let w = worker.clone();
    rig.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains(PROBE)),
        move |_| {
            let codex = w.codex.lock().unwrap().clone();
            let text = match codex {
                Some(version) => format!(
                    "claude.path=\ncodex.path=/home/w/.local/bin/codex\ncodex.link=/home/w/.codex/packages/standalone/releases/{version}/codex\nagy.path=\nnotchling.path=\ncodex.version=codex-cli {version}\n"
                ),
                None => "claude.path=\ncodex.path=\nagy.path=\nnotchling.path=\n".to_string(),
            };
            CommandResult::ok(&text)
        },
    );
    let w = worker.clone();
    rig.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("curl -fsSL")),
        move |call| {
            w.commands
                .lock()
                .unwrap()
                .push(call.argv.last().cloned().unwrap_or_default());
            if installs {
                *w.codex.lock().unwrap() = Some("0.166.0".into());
            }
            CommandResult::ok("")
        },
    );
    rig
}

fn worker() -> Arc<Worker> {
    Arc::new(Worker {
        codex: Mutex::new(None),
        commands: Mutex::new(Vec::new()),
    })
}

fn go() -> InstallGo {
    InstallGo {
        node: M1.into(),
        machine: "m1".into(),
        tool: Tool::Codex,
        mac: Some(v("0.166.0")),
    }
}

#[test]
fn run_installs_the_macs_version_and_succeeds_on_the_reread() {
    let w = worker();
    let rig = rig(&w, true);
    let runs = ToolRuns::default();
    assert!(runs.begin("m1", "start"));
    let outcome = run_install(&rig.engine, &runs, &go());
    assert!(outcome.ok, "{}", outcome.message);
    assert_eq!(outcome.message, "Codex 0.166.0 is installed on m1");
    assert_eq!(w.commands.lock().unwrap().len(), 1);
    assert!(w.commands.lock().unwrap()[0].contains("--release"));
}

#[test]
fn run_fails_when_the_install_leaves_the_tool_missing() {
    let w = worker();
    let rig = rig(&w, false);
    let runs = ToolRuns::default();
    assert!(runs.begin("m1", "start"));
    let outcome = run_install(&rig.engine, &runs, &go());
    assert!(!outcome.ok);
    assert!(
        outcome.message.contains("still isn't found"),
        "{}",
        outcome.message
    );
}

#[test]
fn run_refuses_a_tool_that_is_already_there() {
    let w = worker();
    *w.codex.lock().unwrap() = Some("0.118.0".into());
    let rig = rig(&w, true);
    let runs = ToolRuns::default();
    assert!(runs.begin("m1", "start"));
    let outcome = run_install(&rig.engine, &runs, &go());
    assert!(!outcome.ok);
    assert!(
        outcome.message.contains("already installed"),
        "{}",
        outcome.message
    );
    assert!(w.commands.lock().unwrap().is_empty());
}

#[test]
fn a_sign_in_session_holds_the_prompt_only_until_it_closes() {
    let runs = ToolRuns::default();
    let cancel = runs.open_session("m1", Tool::Codex).unwrap();
    assert!(runs.open_session("m1", Tool::Codex).is_none());
    assert!(runs.session("m1").unwrap().prompt.is_none());
    runs.offer("m1", &cancel, prompt());
    let session = runs.session("m1").unwrap();
    assert_eq!(session.prompt.as_ref().unwrap().code(), "TEST-00000");
    runs.close_session("m1", &cancel);
    assert!(runs.session("m1").is_none());
    runs.offer("m1", &cancel, session.prompt.unwrap());
    assert!(runs.session("m1").is_none());
}

fn prompt() -> notchling_engine::machines::device_login::DevicePrompt {
    notchling_engine::machines::device_login::DevicePrompt::new(
        "https://auth.openai.com/codex/device",
        "TEST-00000",
    )
    .unwrap()
}

#[test]
fn cancel_clears_the_prompt_the_sheet_and_the_running_state_at_once() {
    let runs = ToolRuns::default();
    let cancel = runs.open_session("m1", Tool::Codex).unwrap();
    assert!(runs.begin("m1", "signing in"));
    runs.set_sheet("m1", Tool::Codex, M1);
    runs.offer("m1", &cancel, prompt());
    runs.cancel("m1", "stopping");
    assert!(cancel.load(std::sync::atomic::Ordering::SeqCst));
    assert!(runs.session("m1").is_none());
    assert!(runs.sheet().is_none());
    assert!(runs.running("m1"));
    assert_eq!(
        runs.get("m1"),
        Some(super::tool_runs::Run::Running("stopping".into()))
    );
}

#[test]
fn an_old_run_cannot_touch_a_newer_session_on_the_same_machine() {
    let runs = ToolRuns::default();
    let old = runs.open_session("m1", Tool::Codex).unwrap();
    runs.cancel("m1", "stopping");
    let new = runs.open_session("m1", Tool::Codex).unwrap();
    runs.offer("m1", &old, prompt());
    assert!(runs.session("m1").unwrap().prompt.is_none());
    runs.close_session("m1", &old);
    assert!(runs.session("m1").is_some());
    runs.offer("m1", &new, prompt());
    assert!(runs.session("m1").unwrap().prompt.is_some());
}

#[test]
fn the_sheet_lives_with_the_runs_and_a_run_closes_only_its_own() {
    let runs = Arc::new(ToolRuns::default());
    runs.set_sheet("m1", Tool::Agy, M1);
    let again = runs.clone();
    let sheet = again.sheet().expect("a sheet");
    assert_eq!(
        (sheet.machine.as_str(), sheet.tool, sheet.node.as_str()),
        ("m1", Tool::Agy, M1)
    );
    runs.clear_sheet_if("m1", Tool::Codex);
    runs.clear_sheet_if("m2", Tool::Agy);
    assert!(runs.sheet().is_some());
    runs.clear_sheet_if("m1", Tool::Agy);
    assert!(runs.sheet().is_none());
}

#[test]
fn a_sign_in_is_offered_only_for_a_definite_signed_out_and_never_while_a_job_runs() {
    let side = mac(Some("0.166.0"), None);
    let sw = Switches::all_on();
    let out = signed(json!({"codex": false}));
    let list = offered(&out, Some(&side), &sw);
    assert!(find(&list, M1, Tool::Codex, Kind::Check).is_none());
    assert!(find(&list, M1, Tool::Codex, Kind::SignIn).is_some());
    let jobs = json!([{"id": "j1", "node": M1, "state": "running", "tool": "claude"}]);
    let busy = model(
        json!({"claude": "2.1.327", "codex": "0.166.0"}),
        json!({"codex": false}),
        jobs,
    );
    let list = offered(&busy, Some(&side), &sw);
    let codex = find(&list, M1, Tool::Codex, Kind::SignIn).unwrap();
    assert!(matches!(&codex.state, State::Busy(text) if text.contains("running a job")));
    let why = start_sign_in(&list, M1, Tool::Codex, &busy, &sw).unwrap_err();
    assert!(why.contains("running a job"), "{why}");
    let text = idle_text(&busy, M1, "m1", Kind::SignIn).unwrap();
    assert!(text.contains("running a job"), "{text}");
    assert!(idle_text(&out, M1, "m1", Kind::SignIn).is_none());
}

#[test]
fn an_unknown_login_gets_a_check_that_changes_nothing_on_the_machine() {
    let side = mac(Some("0.166.0"), None);
    let unknown = signed(json!({}));
    let list = offered(&unknown, Some(&side), &Switches::all_off());
    let go = start_check(&list, M1, Tool::Codex).unwrap();
    assert_eq!(go.machine, "m1");
    assert!(start_check(&list, M1, Tool::Claude).is_err());
}

#[test]
fn a_check_result_sets_the_card_both_ways() {
    let side = mac(Some("0.166.0"), None);
    let sw = Switches::all_on();
    let unknown = signed(json!({}));
    let out = |_: &str, tool: Tool| (tool == Tool::Codex).then_some((NOW - 5, false));
    let marks = Marks {
        login: &out,
        installed: &nothing,
        reported: &unreported,
    };
    let model = correct(&unknown, &[machine()], &none, &unread, &marks, NOW);
    assert_eq!(row_of(&model).login_of(Tool::Codex), Some(Login::SignedOut));
    let list = offered(&model, Some(&side), &sw);
    assert!(find(&list, M1, Tool::Codex, Kind::SignIn).is_some());
    assert!(find(&list, M1, Tool::Codex, Kind::Check).is_none());
}

#[test]
fn a_newer_report_from_the_node_beats_an_older_mark_or_probe() {
    let installed =
        |_: &str, tool: Tool| (tool == Tool::Codex).then(|| (NOW - 100, Some(v("0.166.0"))));
    let reported_later = |_: &str, _: Tool| Some(NOW - 50);
    let marks = Marks {
        login: &never,
        installed: &installed,
        reported: &reported_later,
    };
    let model = correct(&missing_both(), &[machine()], &none, &unread, &marks, NOW);
    let texts = row_of(&model).tool_texts();
    assert!(!texts.iter().any(|t| t == "Codex 0.166.0"), "{texts:?}");
    let known = read(vec![(Tool::Codex, ToolState::NotInstalled)], NOW - 100);
    let tools = move |_: &str| Some(known.clone());
    let marks = Marks {
        login: &never,
        installed: &nothing,
        reported: &reported_later,
    };
    let model = correct(&stale(), &[machine()], &none, &tools, &marks, NOW);
    assert_eq!(row_of(&model), row_of(&stale()));
    let known = read(vec![(Tool::Codex, ToolState::NotInstalled)], NOW - 10);
    let tools = move |_: &str| Some(known.clone());
    let model = correct(&stale(), &[machine()], &none, &tools, &marks, NOW);
    let texts = row_of(&model).tool_texts();
    assert!(
        texts.iter().any(|t| t == "Codex not installed"),
        "{texts:?}"
    );
}

#[test]
fn a_login_mark_older_than_the_nodes_report_gives_way() {
    let yes = |_: &str, tool: Tool| (tool == Tool::Codex).then_some((NOW - 100, true));
    let later = |_: &str, _: Tool| Some(NOW - 50);
    let marks = Marks {
        login: &yes,
        installed: &nothing,
        reported: &later,
    };
    let out = signed(json!({"codex": false}));
    let model = correct(&out, &[machine()], &none, &unread, &marks, NOW);
    assert_eq!(row_of(&model).login_of(Tool::Codex), Some(Login::SignedOut));
}

#[test]
fn after_an_install_the_card_keeps_the_new_version_over_the_stale_report() {
    let installed =
        |_: &str, tool: Tool| (tool == Tool::Codex).then(|| (NOW - 10, Some(v("0.166.0"))));
    let marks = Marks {
        login: &never,
        installed: &installed,
        reported: &unreported,
    };
    let model = correct(&stale(), &[machine()], &none, &unread, &marks, NOW);
    let row = row_of(&model);
    let texts = row.tool_texts();
    assert!(texts.iter().any(|t| t == "Codex 0.166.0"), "{texts:?}");
    assert!(!texts.iter().any(|t| t.contains("0.118.0")), "{texts:?}");
    assert!(row.drifts.iter().all(|d| d.tool != Tool::Codex));
}

#[test]
fn run_check_reports_the_signed_state_without_a_login() {
    let w = worker();
    let rig = rig(&w, true);
    rig.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("login status")),
        |_| CommandResult::ok("@@signed_in\n"),
    );
    let runs = ToolRuns::default();
    assert!(runs.begin("m1", "start"));
    let check = run_check(&rig.engine, &runs, "m1", Tool::Codex, M1);
    assert_eq!(check.signed, Some(true));
    assert!(check.outcome.ok);
    assert!(rig
        .runner
        .calls()
        .iter()
        .all(|c| !c.argv.last().is_some_and(|s| s.contains("--device-auth"))));
}

#[test]
fn run_sign_in_goes_through_the_engine_and_needs_the_status_check() {
    let w = worker();
    let rig = rig(&w, true);
    rig.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("--device-auth")),
        |_| CommandResult::ok(""),
    );
    rig.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("login status")),
        |_| CommandResult::ok("@@signed_out\n"),
    );
    let runs = ToolRuns::default();
    assert!(runs.begin("m1", "start"));
    let sink: notchling_engine::machines::device_login::PromptSink = Arc::new(|_| {});
    let go = SignInGo {
        node: M1.into(),
        machine: "m1".into(),
        tool: Tool::Codex,
    };
    let outcome = run_sign_in(
        &rig.engine,
        &runs,
        &go,
        sink,
        Arc::new(AtomicBool::new(false)),
    );
    assert!(!outcome.ok);
    assert!(
        outcome.message.contains("isn't signed in on m1"),
        "{}",
        outcome.message
    );
}

#[test]
fn a_cancelled_run_stays_running_until_it_ends_and_cannot_touch_a_retry() {
    let runs = ToolRuns::default();
    let cancel = runs.open_session("m1", Tool::Codex).unwrap();
    let old = runs.begin_run("m1", "signing in").unwrap();
    runs.cancel("m1", "stopping");
    assert!(runs.running("m1"));
    assert!(runs.begin_run("m1", "again").is_none());
    runs.finish_run("m1", old, &FixOutcome::fail("cancelled"));
    assert!(!runs.running("m1"));
    let retry = runs.begin_run("m1", "again").unwrap();
    assert_ne!(old, retry);
    runs.finish_run("m1", old, &FixOutcome::fail("late"));
    runs.mark_login_run("m1", old, Tool::Codex, true, NOW);
    assert_eq!(runs.login_mark("m1", Tool::Codex), None);
    assert_eq!(
        runs.get("m1"),
        Some(super::tool_runs::Run::Running("again".into()))
    );
    runs.mark_login_run("m1", retry, Tool::Codex, true, NOW);
    assert_eq!(runs.login_mark("m1", Tool::Codex), Some((NOW, true)));
    runs.close_session("m1", &cancel);
}

fn sign_in_go() -> SignInGo {
    SignInGo {
        node: M1.into(),
        machine: "m1".into(),
        tool: Tool::Codex,
    }
}

#[test]
fn a_cancelled_sign_in_ends_as_cancelled_and_rechecks_the_status_before_marking() {
    let w = worker();
    let rig = rig(&w, true);
    rig.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("login status")),
        |_| CommandResult::ok("@@signed_out\n"),
    );
    let runs = ToolRuns::default();
    let cancel = runs.open_session("m1", Tool::Codex).unwrap();
    let id = runs.begin_run("m1", "signing in").unwrap();
    runs.set_sheet("m1", Tool::Codex, M1);
    runs.cancel("m1", "stopping");
    let outcome = FixOutcome::fail("cancelled");
    settle_sign_in(
        &rig.engine,
        &runs,
        &sign_in_go(),
        id,
        &cancel,
        &outcome,
        &|| NOW,
    );
    assert!(!runs.running("m1"));
    match runs.get("m1") {
        Some(super::tool_runs::Run::Done { ok, text }) => {
            assert!(!ok);
            assert!(text.contains("cancel"), "{text}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(runs.login_mark("m1", Tool::Codex), Some((NOW, false)));
    assert!(runs.sheet().is_none());
}

#[test]
fn a_finished_sign_in_marks_and_closes_the_sheet() {
    let w = worker();
    let rig = rig(&w, true);
    let runs = ToolRuns::default();
    let cancel = runs.open_session("m1", Tool::Codex).unwrap();
    let id = runs.begin_run("m1", "signing in").unwrap();
    runs.set_sheet("m1", Tool::Codex, M1);
    let outcome = FixOutcome::ok("signed in");
    settle_sign_in(
        &rig.engine,
        &runs,
        &sign_in_go(),
        id,
        &cancel,
        &outcome,
        &|| NOW,
    );
    assert_eq!(runs.login_mark("m1", Tool::Codex), Some((NOW, true)));
    assert!(runs.sheet().is_none());
    assert!(runs.session("m1").is_none());
}

fn check_go(tool: Tool) -> CheckGo {
    CheckGo {
        node: M1.into(),
        machine: "m1".into(),
        tool,
    }
}

#[test]
fn an_unknown_antigravity_check_brings_back_the_how_to_and_codex_does_not() {
    let runs = ToolRuns::default();
    let unknown = LoginCheck {
        outcome: FixOutcome::fail("couldn't tell"),
        signed: None,
    };
    let id = runs.begin_run("m1", "checking").unwrap();
    settle_check(&runs, &check_go(Tool::Agy), id, &unknown, NOW);
    let sheet = runs.sheet().expect("the how-to");
    assert_eq!((sheet.tool, sheet.node.as_str()), (Tool::Agy, M1));
    runs.clear_sheet();
    runs.dismiss("m1");
    let id = runs.begin_run("m1", "checking").unwrap();
    settle_check(&runs, &check_go(Tool::Codex), id, &unknown, NOW);
    assert!(runs.sheet().is_none());
    runs.dismiss("m1");
    let id = runs.begin_run("m1", "checking").unwrap();
    let known = LoginCheck {
        outcome: FixOutcome::ok("signed in"),
        signed: Some(true),
    };
    settle_check(&runs, &check_go(Tool::Agy), id, &known, NOW);
    assert!(runs.sheet().is_none());
    assert_eq!(runs.login_mark("m1", Tool::Agy), Some((NOW, true)));
}

#[test]
fn a_check_carries_the_node_it_was_offered_for() {
    let side = mac(Some("0.166.0"), None);
    let unknown = signed(json!({}));
    let list = offered(&unknown, Some(&side), &Switches::all_off());
    assert_eq!(start_check(&list, M1, Tool::Codex).unwrap().node, M1);
}

#[test]
fn an_install_waits_on_the_reports_of_its_own_tool_only() {
    let installed =
        |_: &str, tool: Tool| (tool == Tool::Codex).then(|| (NOW - 100, Some(v("0.166.0"))));
    let claude_later = |_: &str, tool: Tool| (tool == Tool::Claude).then_some(NOW - 50);
    let marks = Marks {
        login: &never,
        installed: &installed,
        reported: &claude_later,
    };
    let model = correct(&missing_both(), &[machine()], &none, &unread, &marks, NOW);
    let texts = row_of(&model).tool_texts();
    assert!(texts.iter().any(|t| t == "Codex 0.166.0"), "{texts:?}");
    let codex_later = |_: &str, tool: Tool| (tool == Tool::Codex).then_some(NOW - 50);
    let marks = Marks {
        login: &never,
        installed: &installed,
        reported: &codex_later,
    };
    let model = correct(&missing_both(), &[machine()], &none, &unread, &marks, NOW);
    let texts = row_of(&model).tool_texts();
    assert!(!texts.iter().any(|t| t == "Codex 0.166.0"), "{texts:?}");
}

#[test]
fn an_install_mark_gives_way_to_a_newer_absence_probe() {
    let installed =
        |_: &str, tool: Tool| (tool == Tool::Codex).then(|| (NOW - 10, Some(v("0.166.0"))));
    let marks = Marks {
        login: &never,
        installed: &installed,
        reported: &unreported,
    };
    let known = read(vec![(Tool::Codex, ToolState::NotInstalled)], NOW - 5);
    let tools = move |_: &str| Some(known.clone());
    let model = correct(&stale(), &[machine()], &none, &tools, &marks, NOW);
    let texts = row_of(&model).tool_texts();
    assert!(
        texts.iter().any(|t| t == "Codex not installed"),
        "{texts:?}"
    );
    let known = read(vec![(Tool::Codex, ToolState::NotInstalled)], NOW - 20);
    let tools = move |_: &str| Some(known.clone());
    let model = correct(&stale(), &[machine()], &none, &tools, &marks, NOW);
    let texts = row_of(&model).tool_texts();
    assert!(texts.iter().any(|t| t == "Codex 0.166.0"), "{texts:?}");
}

#[test]
fn nothing_changes_when_the_cluster_cannot_be_read_first() {
    let feed = ClusterFeed::none();
    let refused = idle_now(&feed, M1, "m1", Kind::Install).expect("refused");
    assert!(!refused.ok);
    assert!(refused.message.contains("m1"), "{}", refused.message);
    assert!(idle_now(&feed, M1, "m1", Kind::SignIn).is_some());
}

#[test]
fn a_cancelled_sign_in_stays_running_through_the_recheck_and_stamps_after_it() {
    let w = worker();
    let rig = rig(&w, true);
    let runs = Arc::new(ToolRuns::default());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let probed = Arc::new(AtomicBool::new(false));
    let (during, flag, watch) = (runs.clone(), probed.clone(), seen.clone());
    rig.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("login status")),
        move |_| {
            watch.lock().unwrap().push(during.running("m1"));
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            CommandResult::ok("@@signed_in\n")
        },
    );
    let cancel = runs.open_session("m1", Tool::Codex).unwrap();
    let id = runs.begin_run("m1", "signing in").unwrap();
    runs.cancel("m1", "stopping");
    let after = probed.clone();
    let stamped = Arc::new(Mutex::new(None));
    let record = stamped.clone();
    let now = move || {
        *record.lock().unwrap() = Some(after.load(std::sync::atomic::Ordering::SeqCst));
        NOW
    };
    let outcome = FixOutcome::fail("cancelled");
    settle_sign_in(
        &rig.engine,
        &runs,
        &sign_in_go(),
        id,
        &cancel,
        &outcome,
        &now,
    );
    assert_eq!(*seen.lock().unwrap(), vec![true]);
    assert_eq!(*stamped.lock().unwrap(), Some(true));
    assert!(!runs.running("m1"));
    assert_eq!(runs.login_mark("m1", Tool::Codex), Some((NOW, true)));
}
