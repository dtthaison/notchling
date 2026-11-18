use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use notchling_engine::machines::registry::{Machine, MachineOs, NodeSettings};
use notchling_engine::machines::runner::CommandResult;
use notchling_engine::machines::tool_updates::ToolMove;
use notchling_engine::versions::{Method, Tool, ToolState, Version, WorkerTools};
use serde_json::{json, Value};

use super::add_rig_tests::Rig;
use super::model::ClusterModel;
use super::tool_runs::{Run, ToolRuns};
use super::tool_updates::{for_warning, offers, offers_runnable, run, start, Offer, State};
use crate::machines::versions_feed::{MacSide, MacTool};
use crate::switches::Switches;

const ME: &str = "a1b2c3d4e5f60718";
const M1: &str = "1122334455667788";

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn member(id: &str, name: &str, tools: Value) -> Value {
    json!({
        "id": id, "name": name, "role": "worker", "canBeMaster": false,
        "online": true, "client": false, "headless": true, "reference": false,
        "state": "active",
        "versions": {
            "notchling": "0.9.0", "proto": 1, "features": ["dispatch"], "schema": 1,
            "os": "macos", "arch": "aarch64", "tools": tools,
        },
    })
}

fn model_with(worker_tools: Value, jobs: Value) -> ClusterModel {
    let mut me = member(
        ME,
        "m5",
        json!({"claude": "2.1.327", "codex": "0.166.0", "antigravity": "1.5.9"}),
    );
    me["role"] = json!("home");
    me["reference"] = json!(true);
    let status = json!({
        "cluster": {"id": "c1", "name": "home", "term": 1, "home": ME},
        "me": ME,
        "members": [me, member(M1, "m1", worker_tools)],
        "jobs": jobs,
        "link": true, "tailscale": true, "listening": [47823],
    });
    ClusterModel::from_status(&status, ME)
}

fn drifted() -> ClusterModel {
    model_with(json!({"claude": "2.1.318", "codex": "0.118.0"}), json!([]))
}

fn machine() -> Machine {
    let mut m = Machine::new("m1", "m1.test", "worker");
    m.os = Some(MachineOs::MacOs);
    m.node = Some(NodeSettings::default());
    m
}

fn mac(claude: Option<&str>, codex: Option<&str>, agy: Option<&str>) -> MacSide {
    let tool = |version: Option<&str>| MacTool {
        installed: version.map(v),
        method: Method::Native,
        reference: version.map(v),
        usable: version.is_some(),
        installed_failed: false,
    };
    let mut tools = BTreeMap::new();
    tools.insert(Tool::Claude, tool(claude));
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

fn made(
    model: &ClusterModel,
    mac: Option<&MacSide>,
    tools: &dyn Fn(&str) -> Option<WorkerTools>,
    switches: &Switches,
) -> Vec<Offer> {
    offers(model, &[machine()], &none, tools, mac, switches)
}

fn installed(version: &str, method: Method) -> ToolState {
    ToolState::Installed {
        version: Some(v(version)),
        method,
        path: "/home/w/.local/bin/x".into(),
    }
}

fn read_tools(entries: Vec<(Tool, ToolState)>) -> WorkerTools {
    WorkerTools {
        tools: entries.into_iter().collect(),
        at_ms: 0,
    }
}

fn by_tool(list: &[Offer], tool: Tool) -> Option<&Offer> {
    list.iter().find(|o| o.tool == tool)
}

#[test]
fn a_drift_with_a_usable_reference_offers_an_update() {
    let side = mac(Some("2.1.327"), Some("0.166.0"), None);
    let list = made(&drifted(), Some(&side), &unread, &Switches::all_on());
    assert_eq!(list.len(), 2, "{list:?}");
    let claude = by_tool(&list, Tool::Claude).unwrap();
    assert_eq!(claude.state, State::Ready);
    assert_eq!(claude.machine, "m1");
    assert_eq!(claude.found, v("2.1.318"));
    assert_eq!(claude.to, v("2.1.327"));
    assert_eq!(claude.button(), "Update Claude Code on m1 to 2.1.327");
    assert_eq!(
        by_tool(&list, Tool::Codex).unwrap().button(),
        "Update Codex on m1 to 0.166.0"
    );
}

#[test]
fn no_usable_reference_means_no_button() {
    let unchecked = mac(Some("2.1.327"), None, None);
    let list = made(&drifted(), Some(&unchecked), &unread, &Switches::all_on());
    assert!(by_tool(&list, Tool::Codex).is_none(), "{list:?}");
    assert!(by_tool(&list, Tool::Claude).is_some());
    let mut stale = mac(Some("2.1.327"), Some("0.166.0"), None);
    for tool in stale.tools.values_mut() {
        tool.usable = false;
    }
    assert!(made(&drifted(), Some(&stale), &unread, &Switches::all_on()).is_empty());
    assert!(made(&drifted(), None, &unread, &Switches::all_on()).is_empty());
}

#[test]
fn a_worker_on_the_reference_has_no_button() {
    let model = model_with(json!({"claude": "2.1.327", "codex": "0.118.0"}), json!([]));
    let side = mac(Some("2.1.327"), Some("0.166.0"), None);
    let list = made(&model, Some(&side), &unread, &Switches::all_on());
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].tool, Tool::Codex);
    let usable_differs = mac(Some("2.1.318"), Some("0.118.0"), None);
    assert!(
        made(
            &drifted(),
            Some(&usable_differs),
            &unread,
            &Switches::all_on()
        )
        .is_empty(),
        "the worker already runs the usable reference"
    );
}

#[test]
fn a_reference_older_than_the_worker_moves_it_down() {
    let model = model_with(json!({"claude": "2.1.330", "codex": "0.166.0"}), json!([]));
    let side = mac(Some("2.1.327"), Some("0.166.0"), None);
    let list = made(&model, Some(&side), &unread, &Switches::all_on());
    let claude = &list[0];
    assert!(claude.down());
    assert_eq!(claude.button(), "Move Claude Code on m1 down to 2.1.327");
}

#[test]
fn the_button_belongs_to_its_warning_line() {
    let model = drifted();
    let side = mac(Some("2.1.327"), Some("0.166.0"), None);
    let list = made(&model, Some(&side), &unread, &Switches::all_on());
    let row = model.row(M1).unwrap();
    assert!(!row.warnings.is_empty());
    for warning in &row.warnings {
        let offer = for_warning(&list, M1, warning).unwrap_or_else(|| panic!("{warning}"));
        assert!(warning.contains(offer.tool.title()), "{warning}");
    }
    assert!(for_warning(&list, M1, "something else").is_none());
    assert!(for_warning(&list, "other", &row.warnings[0]).is_none());
}

#[test]
fn only_nodes_that_can_be_reached_get_a_button() {
    let side = mac(Some("2.1.327"), Some("0.166.0"), None);
    let model = drifted();
    assert!(offers(
        &model,
        &[],
        &none,
        &unread,
        Some(&side),
        &Switches::all_on()
    )
    .is_empty());
    let mut other = machine();
    other.name = "elsewhere".into();
    assert!(offers(
        &model,
        &[other],
        &none,
        &unread,
        Some(&side),
        &Switches::all_on()
    )
    .is_empty());
    let mut status = json!({
        "cluster": {"id": "c1", "name": "home", "term": 1, "home": ME},
        "me": ME,
        "members": [
            {"id": ME, "name": "m5", "role": "home", "reference": true, "online": true,
             "state": "active", "client": false, "headless": false, "canBeMaster": false,
             "versions": {"notchling": "0.9.0", "proto": 1, "features": ["dispatch"], "schema": 1,
                          "os": "macos", "arch": "aarch64", "tools": {"claude": "2.1.327"}}},
            member(M1, "m1", json!({"claude": "2.1.318"})),
        ],
        "jobs": [], "link": true, "tailscale": true, "listening": [47823],
    });
    status["members"][1]["online"] = json!(false);
    let model = ClusterModel::from_status(&status, ME);
    assert!(made(&model, Some(&side), &unread, &Switches::all_on()).is_empty());
}

#[test]
fn what_the_worker_reads_decides_run_or_how_to() {
    let side = mac(Some("2.1.327"), Some("0.166.0"), None);
    let brew = |_: &str| {
        Some(read_tools(vec![
            (Tool::Claude, installed("2.1.318", Method::Brew)),
            (Tool::Codex, installed("0.118.0", Method::Npm)),
        ]))
    };
    let list = made(&drifted(), Some(&side), &brew, &Switches::all_on());
    match &by_tool(&list, Tool::Claude).unwrap().state {
        State::HowTo(text) => assert!(text.contains("Homebrew"), "{text}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(by_tool(&list, Tool::Codex).unwrap().state, State::Ready);
    let unknown = |_: &str| {
        Some(read_tools(vec![
            (Tool::Claude, installed("2.1.318", Method::Unknown)),
            (Tool::Codex, ToolState::NotInstalled),
        ]))
    };
    let list = made(&drifted(), Some(&side), &unknown, &Switches::all_on());
    assert!(matches!(
        by_tool(&list, Tool::Claude).unwrap().state,
        State::HowTo(_)
    ));
    assert!(by_tool(&list, Tool::Codex).is_none());
    let ahead = |_: &str| {
        Some(read_tools(vec![
            (Tool::Claude, installed("2.1.327", Method::Native)),
            (Tool::Codex, installed("0.118.0", Method::Npm)),
        ]))
    };
    let list = made(&drifted(), Some(&side), &ahead, &Switches::all_on());
    assert!(by_tool(&list, Tool::Claude).is_none(), "already there");
}

#[test]
fn antigravity_is_always_a_how_to() {
    let model = model_with(
        json!({"claude": "2.1.327", "codex": "0.166.0", "antigravity": "1.4.0"}),
        json!([]),
    );
    let side = mac(Some("2.1.327"), Some("0.166.0"), Some("1.5.9"));
    let list = made(&model, Some(&side), &unread, &Switches::all_on());
    assert_eq!(list.len(), 1, "{list:?}");
    match &list[0].state {
        State::HowTo(text) => assert!(text.contains("agy update"), "{text}"),
        other => panic!("{other:?}"),
    }
    assert!(!offers_runnable(&list, M1));
    assert!(start(&list, M1, None, &model, &Switches::all_on()).is_err());
}

#[test]
fn the_install_switch_and_running_jobs_hold_the_button_back() {
    let side = mac(Some("2.1.327"), Some("0.166.0"), None);
    let off = made(&drifted(), Some(&side), &unread, &Switches::all_off());
    assert!(
        off.iter().all(|o| matches!(o.state, State::Off(_))),
        "{off:?}"
    );
    let detached = made(&drifted(), Some(&side), &unread, &Switches::detached());
    assert!(detached.iter().all(|o| matches!(o.state, State::Off(_))));
    let busy_model = model_with(
        json!({"claude": "2.1.318", "codex": "0.118.0"}),
        json!([{"id": "j1", "state": "running", "node": M1, "tool": "claude"}]),
    );
    let busy = made(&busy_model, Some(&side), &unread, &Switches::all_on());
    match &busy[0].state {
        State::Busy(text) => assert_eq!(
            text,
            "m1 is running a job. Update its tools when it is done."
        ),
        other => panic!("{other:?}"),
    }
    let done_model = model_with(
        json!({"claude": "2.1.318", "codex": "0.118.0"}),
        json!([{"id": "j1", "state": "done", "node": M1, "tool": "claude"}]),
    );
    let free = made(&done_model, Some(&side), &unread, &Switches::all_on());
    assert!(free.iter().all(Offer::ready));
}

#[test]
fn start_rechecks_the_switch_and_the_jobs_when_clicked() {
    let side = mac(Some("2.1.327"), Some("0.166.0"), None);
    let model = drifted();
    let list = made(&model, Some(&side), &unread, &Switches::all_on());
    let go = start(&list, M1, None, &model, &Switches::all_on()).unwrap();
    assert_eq!(go.machine, "m1");
    assert_eq!(
        go.moves,
        vec![
            ToolMove {
                tool: Tool::Claude,
                to: v("2.1.327")
            },
            ToolMove {
                tool: Tool::Codex,
                to: v("0.166.0")
            },
        ]
    );
    let one = start(&list, M1, Some(Tool::Codex), &model, &Switches::all_on()).unwrap();
    assert_eq!(one.moves.len(), 1);
    assert_eq!(one.moves[0].tool, Tool::Codex);
    let off = start(&list, M1, None, &model, &Switches::all_off()).unwrap_err();
    assert!(off.contains("install is off"), "{off}");
    let busy_model = model_with(
        json!({"claude": "2.1.318", "codex": "0.118.0"}),
        json!([{"id": "j1", "state": "dispatching", "node": M1, "tool": "shell"}]),
    );
    let busy = start(&list, M1, None, &busy_model, &Switches::all_on()).unwrap_err();
    assert!(busy.contains("running a job"), "{busy}");
    assert!(start(&list, "nobody", None, &model, &Switches::all_on()).is_err());
}

#[test]
fn how_to_tools_are_left_out_of_update_tools() {
    let side = mac(Some("2.1.327"), Some("0.166.0"), None);
    let model = drifted();
    let brew = |_: &str| {
        Some(read_tools(vec![
            (Tool::Claude, installed("2.1.318", Method::Brew)),
            (Tool::Codex, installed("0.118.0", Method::Npm)),
        ]))
    };
    let list = made(&model, Some(&side), &brew, &Switches::all_on());
    let go = start(&list, M1, None, &model, &Switches::all_on()).unwrap();
    assert_eq!(go.moves.len(), 1);
    assert_eq!(go.moves[0].tool, Tool::Codex);
    let why = start(&list, M1, Some(Tool::Claude), &model, &Switches::all_on()).unwrap_err();
    assert!(why.contains("Homebrew"), "{why}");
}

#[test]
fn runs_track_progress_result_and_dismissal() {
    let runs = ToolRuns::default();
    assert!(runs.get("m1").is_none());
    assert!(runs.begin("m1", "Reading"));
    assert!(!runs.begin("m1", "again"), "one run per machine");
    assert!(runs.running("m1"));
    runs.say("m1", "Updating");
    assert_eq!(runs.get("m1"), Some(Run::Running("Updating".into())));
    runs.dismiss("m1");
    assert!(runs.running("m1"), "a running line can't be dismissed");
    let before = runs.revision();
    runs.finish(
        "m1",
        &notchling_engine::machines::step::FixOutcome::fail("boom"),
    );
    assert!(runs.revision() > before);
    assert_eq!(
        runs.get("m1"),
        Some(Run::Done {
            ok: false,
            text: "boom".into()
        })
    );
    runs.say("m1", "ignored after the end");
    assert!(!runs.running("m1"));
    runs.dismiss("m1");
    assert!(runs.get("m1").is_none());
    assert!(runs.begin("m1", "next"));
}

const PROBE: &str = "for t in claude codex agy notchling";

struct Worker {
    claude: Mutex<String>,
    installs: Mutex<Vec<String>>,
    sticky: bool,
}

fn rig(worker: &Arc<Worker>) -> Rig {
    let rig = Rig::new(vec![machine()]);
    let w = worker.clone();
    rig.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains(PROBE)),
        move |_| {
            let version = w.claude.lock().unwrap().clone();
            CommandResult::ok(&format!(
                "claude.path=/home/w/.local/bin/claude\nclaude.link=/home/w/.local/share/claude/versions/{version}\ncodex.path=\nagy.path=\nnotchling.path=\nclaude.version={version} (Claude Code)\n"
            ))
        },
    );
    rig.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("busy=")),
        |_| CommandResult::ok("busy=0\n"),
    );
    let w = worker.clone();
    rig.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains("install")),
        move |call| {
            w.installs
                .lock()
                .unwrap()
                .push(call.argv.last().cloned().unwrap_or_default());
            if !w.sticky {
                *w.claude.lock().unwrap() = "2.1.327".into();
            }
            CommandResult::ok("")
        },
    );
    rig
}

fn go() -> super::tool_updates::Start {
    super::tool_updates::Start {
        machine: "m1".into(),
        moves: vec![ToolMove {
            tool: Tool::Claude,
            to: v("2.1.327"),
        }],
    }
}

#[test]
fn run_reads_updates_and_rereads_over_the_worker_runner() {
    let worker = Arc::new(Worker {
        claude: Mutex::new("2.1.318".into()),
        installs: Mutex::new(Vec::new()),
        sticky: false,
    });
    let rig = rig(&worker);
    let runs = ToolRuns::default();
    assert!(runs.begin("m1", "start"));
    let outcome = run(&rig.engine, &runs, &go());
    assert!(outcome.ok, "{}", outcome.message);
    assert_eq!(outcome.message, "Claude Code on m1 is now 2.1.327");
    assert_eq!(worker.installs.lock().unwrap().len(), 1);
    let known = rig.engine.worker_tools("m1").unwrap();
    assert_eq!(
        known.tools[&Tool::Claude],
        ToolState::Installed {
            version: Some(v("2.1.327")),
            method: Method::Native,
            path: "/home/w/.local/bin/claude".into(),
        }
    );
}

#[test]
fn run_fails_when_the_worker_still_reports_the_old_version() {
    let worker = Arc::new(Worker {
        claude: Mutex::new("2.1.318".into()),
        installs: Mutex::new(Vec::new()),
        sticky: true,
    });
    let rig = rig(&worker);
    let runs = ToolRuns::default();
    assert!(runs.begin("m1", "start"));
    let outcome = run(&rig.engine, &runs, &go());
    assert!(!outcome.ok);
    assert!(
        outcome
            .message
            .contains("still reports 2.1.318, not 2.1.327"),
        "{}",
        outcome.message
    );
}

#[test]
fn run_says_how_to_when_the_method_cannot_pin() {
    let worker = Arc::new(Worker {
        claude: Mutex::new("2.1.318".into()),
        installs: Mutex::new(Vec::new()),
        sticky: false,
    });
    let rig = Rig::new(vec![machine()]);
    rig.runner.on_call(
        |argv| argv.last().is_some_and(|s| s.contains(PROBE)),
        |_| {
            CommandResult::ok(
                "claude.path=/opt/homebrew/bin/claude\nclaude.link=/opt/homebrew/Caskroom/claude-code/2.1.318/claude\ncodex.path=\nagy.path=\nnotchling.path=\nclaude.version=2.1.318 (Claude Code)\n",
            )
        },
    );
    let runs = ToolRuns::default();
    assert!(runs.begin("m1", "start"));
    let outcome = run(&rig.engine, &runs, &go());
    assert!(!outcome.ok);
    assert!(outcome.message.contains("Homebrew"), "{}", outcome.message);
    assert!(worker.installs.lock().unwrap().is_empty());
}

fn two_named_m1() -> ClusterModel {
    let mut me = member(ME, "m5", json!({"claude": "2.1.327"}));
    me["role"] = json!("home");
    me["reference"] = json!(true);
    let status = json!({
        "cluster": {"id": "c1", "name": "home", "term": 1, "home": ME},
        "me": ME,
        "members": [
            me,
            member(M1, "m1", json!({"claude": "2.1.318"})),
            member("99aabbccddeeff00", "m1", json!({"claude": "2.1.318"})),
        ],
        "jobs": [], "link": true, "tailscale": true, "listening": [47823],
    });
    ClusterModel::from_status(&status, ME)
}

#[test]
fn two_rows_with_one_name_never_pick_a_machine_by_name() {
    let model = two_named_m1();
    let side = mac(Some("2.1.327"), None, None);
    let list = made(&model, Some(&side), &unread, &Switches::all_on());
    assert!(list.is_empty(), "{list:?}");
}

#[test]
fn a_joined_node_id_binds_the_row_to_its_machine() {
    let model = two_named_m1();
    let side = mac(Some("2.1.327"), None, None);
    let bound = |name: &str| (name == "m1").then(|| M1.to_string());
    let list = offers(
        &model,
        &[machine()],
        &bound,
        &unread,
        Some(&side),
        &Switches::all_on(),
    );
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0].node, M1);
}

#[test]
fn a_machine_joined_to_another_node_is_not_matched_by_name() {
    let model = drifted();
    let side = mac(Some("2.1.327"), Some("0.166.0"), None);
    let elsewhere = |name: &str| (name == "m1").then(|| "ffffffffffffffff".to_string());
    let list = offers(
        &model,
        &[machine()],
        &elsewhere,
        &unread,
        Some(&side),
        &Switches::all_on(),
    );
    assert!(list.is_empty(), "{list:?}");
}

#[test]
fn a_unique_name_with_no_joined_machine_still_matches() {
    let side = mac(Some("2.1.327"), Some("0.166.0"), None);
    let list = made(&drifted(), Some(&side), &unread, &Switches::all_on());
    assert_eq!(list.len(), 2);
}

#[test]
fn two_machines_with_one_name_match_nothing() {
    let side = mac(Some("2.1.327"), Some("0.166.0"), None);
    let list = offers(
        &drifted(),
        &[machine(), machine()],
        &none,
        &unread,
        Some(&side),
        &Switches::all_on(),
    );
    assert!(list.is_empty(), "{list:?}");
}
