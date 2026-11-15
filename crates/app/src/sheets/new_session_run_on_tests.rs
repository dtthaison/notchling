use notchling_engine::cluster::Tool as ClusterTool;
use serde_json::{json, Value};

use super::new_session::{NewSession, Plan, Stage};
use super::run_on::{Code, Remote, RunOn};
use crate::acting::{Acting, Request};
use crate::cluster::model::ClusterModel;
use crate::cluster::renders::{status, ME, STANDBY, WORKER};

const FOLDER: &str = "/Users/me/work/api";

fn member<'a>(status: &'a mut Value, id: &str) -> &'a mut Value {
    status["members"]
        .as_array_mut()
        .and_then(|m| m.iter_mut().find(|m| m["id"] == json!(id)))
        .expect("a member")
}

fn run_on(status: &Value) -> RunOn {
    RunOn::from_cluster(&ClusterModel::from_status(status, ME))
}

fn sheet(status: &Value) -> NewSession {
    NewSession::new(Vec::new(), vec![FOLDER.to_owned()], "/Users/me").with_run_on(run_on(status))
}

fn online_linux_worker() -> Value {
    let mut status = status();
    let worker = member(&mut status, WORKER);
    worker["online"] = json!(true);
    worker["versions"]["tools"]["claude"] = json!("2.1.320");
    status
}

fn names(rows: &[super::run_on::RunOnRow]) -> Vec<&str> {
    rows.iter().map(|r| r.name.as_str()).collect()
}

fn args(request: &Request) -> &Value {
    assert_eq!(request.op, "cluster.dispatch");
    &request.args
}

#[test]
fn run_on_lists_online_nodes_first_with_reasons() {
    let status = online_linux_worker();
    let mut offline = status.clone();
    member(&mut offline, STANDBY)["online"] = json!(false);
    let rows = run_on(&status).rows(ClusterTool::Claude);
    assert_eq!(names(&rows), ["This Mac", "build-box", "mac-mini"]);
    assert!(rows[0].this_mac && rows[0].pickable());
    let rows = run_on(&offline).rows(ClusterTool::Agy);
    assert_eq!(names(&rows), ["This Mac", "build-box", "mac-mini"]);
    let mut down = status.clone();
    member(&mut down, WORKER)["online"] = json!(false);
    let rows = run_on(&down).rows(ClusterTool::Shell);
    assert_eq!(names(&rows), ["This Mac", "mac-mini", "build-box"]);
    let offline_row = &rows[2];
    assert!(!offline_row.online && !offline_row.pickable());
    assert_eq!(
        offline_row.reason.as_deref(),
        Some("build-box is offline"),
        "{offline_row:?}"
    );
    assert!(rows[1].pickable() && rows[1].warning.is_none());
}

#[test]
fn version_mismatch_is_pickable_with_warning() {
    let mut status = status();
    member(&mut status, STANDBY)["versions"]["tools"]["claude"] = json!("2.1.300");
    let rows = run_on(&status).rows(ClusterTool::Claude);
    let mini = rows.iter().find(|r| r.name == "mac-mini").expect("a row");
    assert!(mini.pickable(), "{mini:?}");
    let warning = mini.warning.as_deref().expect("a warning");
    assert!(
        warning.contains("2.1.300") && warning.contains("2.1.320"),
        "{warning}"
    );
    let mut sheet = sheet(&status);
    sheet.pick_node(STANDBY);
    assert_eq!(sheet.target.node.as_deref(), Some(STANDBY));
}

#[test]
fn codex_on_another_node_is_refused() {
    let status = status();
    let rows = run_on(&status).rows(ClusterTool::Codex);
    let mini = rows.iter().find(|r| r.name == "mac-mini").expect("a row");
    assert!(!mini.pickable());
    assert_eq!(
        mini.reason.as_deref(),
        Some("Codex jobs on another machine come in a later version")
    );
    assert!(rows[0].pickable(), "this Mac still runs Codex");
    let mut sheet = sheet(&status);
    sheet.pick_remote(Remote::Codex);
    sheet.pick_node(STANDBY);
    assert_eq!(sheet.target.node, None, "a refused node can't be picked");
}

#[test]
fn claude_on_linux_is_refused_until_proven() {
    let status = online_linux_worker();
    let rows = run_on(&status).rows(ClusterTool::Claude);
    let worker = rows.iter().find(|r| r.name == "build-box").expect("a row");
    assert!(!worker.pickable(), "{worker:?}");
    assert!(
        worker
            .reason
            .as_deref()
            .is_some_and(|r| r.contains("Linux")),
        "{worker:?}"
    );
    let rows = run_on(&status).rows(ClusterTool::Shell);
    assert!(rows.iter().all(|r| r.pickable()), "{rows:?}");
}

#[test]
fn a_cluster_of_one_offers_no_column() {
    let alone = ClusterModel::from_status(&crate::cluster::renders::empty(), ME);
    assert!(!RunOn::from_cluster(&alone).offered());
    assert!(run_on(&status()).offered());
}

#[test]
fn start_on_worker_sends_dispatch_with_key() {
    let mut sheet = sheet(&status());
    sheet.pick_node(STANDBY);
    sheet.pick_folder(0);
    sheet.set_path("/srv/api");
    assert!(sheet.start_control(Acting::On).enabled);
    let Some(Plan::Request(request)) = sheet.plan() else {
        panic!("a dispatch")
    };
    let a = args(&request);
    assert_eq!(a["node"], STANDBY);
    assert_eq!(a["tool"], "claude");
    assert_eq!(a["cwd"], "/srv/api");
    assert_eq!(a["code"], "folder");
    let key = a["key"].as_str().expect("a key");
    assert!(key.starts_with("ns-") && key.len() > 10, "{key}");
    assert_eq!(sheet.stage, Stage::Starting);
    assert!(!sheet.start_control(Acting::On).enabled, "once at a time");
    sheet.stage = Stage::Choosing;
    let again = sheet.plan();
    let Some(Plan::Request(again)) = again else {
        panic!("a second dispatch")
    };
    assert_ne!(args(&again)["key"], a["key"], "a fresh key each time");

    let mut here = NewSession::new(Vec::new(), vec![FOLDER.to_owned()], "/Users/me");
    here.pick_folder(0);
    assert!(
        here.dispatch_request().is_none(),
        "this Mac never dispatches"
    );
}

#[test]
fn send_repository_sets_code_send() {
    let mut sheet = sheet(&status());
    sheet.pick_node(STANDBY);
    sheet.pick_code(Code::Send);
    sheet.pick_folder(0);
    let request = sheet.dispatch_request().expect("a dispatch");
    let a = args(&request);
    assert_eq!(a["code"], "send");
    assert_eq!(a["cwd"], FOLDER, "the repository on this Mac");
    let mut folder = NewSession::new(Vec::new(), vec![FOLDER.to_owned()], "/Users/me")
        .with_run_on(run_on(&status()));
    folder.pick_node(STANDBY);
    folder.pick_code(Code::Folder);
    assert!(
        folder.dispatch_request().is_none(),
        "a folder there is typed"
    );
    folder.set_path("relative/path");
    assert!(folder.dispatch_request().is_none(), "an absolute path");
}

#[test]
fn shell_command_dispatch_carries_command() {
    let mut sheet = sheet(&status());
    sheet.pick_remote(Remote::Shell);
    sheet.pick_node(STANDBY);
    sheet.pick_code(Code::Send);
    sheet.pick_folder(0);
    assert!(
        sheet.dispatch_request().is_none(),
        "a shell job needs a command"
    );
    sheet.set_text("  make test  ");
    let request = sheet.dispatch_request().expect("a dispatch");
    let a = args(&request);
    assert_eq!(a["tool"], "shell");
    assert_eq!(a["command"], "make test");
    assert!(a.get("text").is_none() && a.get("model").is_none());

    sheet.stage = Stage::Choosing;
    sheet.pick_remote(Remote::Agy);
    sheet.set_text("look at the failing test");
    assert!(
        sheet.dispatch_request().is_none(),
        "Chat can't show an Antigravity job on another machine yet"
    );
    assert_eq!(
        sheet.node_row().and_then(|r| r.reason).as_deref(),
        Some("Antigravity on another machine comes later.")
    );
}

#[test]
fn antigravity_on_another_node_is_refused_like_codex() {
    let status = status();
    let rows = run_on(&status).rows(ClusterTool::Agy);
    let mini = rows.iter().find(|r| r.name == "mac-mini").expect("a row");
    assert!(!mini.pickable());
    assert_eq!(
        mini.reason.as_deref(),
        Some("Antigravity on another machine comes later.")
    );
    assert!(rows[0].pickable(), "this Mac still runs Antigravity");
    let mut sheet = sheet(&status);
    sheet.pick_remote(Remote::Agy);
    sheet.pick_node(STANDBY);
    assert_eq!(sheet.target.node, None, "a refused node can't be picked");
}

#[test]
fn no_secrets_on_dispatch() {
    let allowed = ["node", "tool", "cwd", "key", "code", "command", "text"];
    for remote in [Remote::Claude, Remote::Shell] {
        let mut sheet = sheet(&status());
        sheet.pick_remote(remote);
        sheet.pick_node(STANDBY);
        sheet.pick_code(Code::Send);
        sheet.pick_folder(0);
        sheet.set_text("run it");
        let request = sheet.dispatch_request().expect("a dispatch");
        let object = request.args.as_object().expect("an object");
        for name in object.keys() {
            assert!(allowed.contains(&name.as_str()), "{name} on {remote:?}");
        }
        assert!(request.args.get("secrets").is_none());
    }
}

#[test]
fn a_dispatch_warning_is_shown_and_an_error_never_opens_the_trust_pop_up() {
    let mut sheet = sheet(&status());
    sheet.pick_node(STANDBY);
    sheet.set_path("/srv/api");
    sheet.dispatch_request().expect("a dispatch");
    sheet.dispatched(
        &json!({"job": "j-1", "warning": "mac-mini runs Claude Code 2.1.300, this Mac 2.1.320"}),
    );
    assert!(matches!(&sheet.stage, Stage::Sent(text) if text.contains("2.1.300")));
    assert!(sheet.footer().contains("mac-mini"));
    sheet.stage = Stage::Choosing;
    sheet.dispatch_request().expect("a dispatch");
    sheet.dispatched(&json!({"job": "j-2"}));
    assert_eq!(sheet.stage, Stage::Started);
    sheet.stage = Stage::Choosing;
    sheet.dispatch_request().expect("a dispatch");
    sheet.started(Err("Workspace not trusted".into()));
    assert_eq!(sheet.stage, Stage::Failed("Workspace not trusted".into()));
    assert_eq!(sheet.untrusted_folder(), None);
}

#[test]
fn the_footer_names_the_node_the_tool_and_the_folder() {
    let mut sheet = sheet(&status());
    sheet.pick_node(STANDBY);
    assert_eq!(sheet.footer(), "Choose a folder for mac-mini.");
    sheet.set_path("/srv/site");
    assert_eq!(sheet.footer(), "Claude Code on mac-mini in site");
    let mut down = status();
    member(&mut down, STANDBY)["online"] = json!(false);
    let mut gone = NewSession::new(Vec::new(), Vec::new(), "").with_run_on(run_on(&down));
    gone.target.node = Some(STANDBY.to_owned());
    assert_eq!(gone.footer(), "mac-mini is offline");
    assert!(!gone.start_control(Acting::On).enabled);
}

#[test]
fn the_account_picked_on_this_mac_decides_which_nodes_can_be_picked() {
    use super::new_session::{AccountChoice, Pick, Section};
    let choice = |section, title: &str| AccountChoice {
        title: title.to_owned(),
        detail: String::new(),
        section,
        pick: Pick::Claude(None),
    };
    let accounts = vec![
        choice(Section::Claude, "me"),
        choice(Section::Antigravity, "agy"),
        choice(Section::Codex, "codex"),
    ];
    let mut sheet = NewSession::new(accounts, vec![FOLDER.to_owned()], "/Users/me")
        .with_run_on(run_on(&online_linux_worker()));
    sheet.pick_node(WORKER);
    assert_eq!(sheet.target.node, None, "Claude Code on Linux is refused");
    sheet.pick_account(1);
    assert_eq!(sheet.target.remote, Remote::Agy);
    sheet.pick_node(WORKER);
    assert_eq!(sheet.target.node, None, "Antigravity elsewhere is refused");
    sheet.pick_account(0);
    sheet.pick_node(STANDBY);
    assert_eq!(sheet.target.node.as_deref(), Some(STANDBY));
    sheet.pick_account(1);
    assert_eq!(
        sheet.target.remote,
        Remote::Claude,
        "the tool stays once a node is picked"
    );
    sheet.pick_remote(Remote::Agy);
    assert!(!sheet.remote_ready(), "the picked node now refuses it");
}
