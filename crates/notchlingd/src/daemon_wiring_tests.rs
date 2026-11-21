use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::switches::{ACCOUNTS, VAULT};
use notchling_local_api::{ops, ClientKind, ErrorCode};
use serde_json::json;
use std::path::PathBuf;

struct Root(PathBuf);

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn wired(tag: &str) -> (Root, Layout, Wired) {
    let root = std::env::temp_dir().join(format!("nl-wiring-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let layout = Layout::resolve(None, Some(root.as_os_str()), || None).unwrap();
    assert!(layout.fixture);
    let sessions = Arc::new(Mutex::new(json!([])));
    let wired = Wired::build(&layout, sessions, Arc::new(FakeRoom::default()), (0, 0));
    (Root(root), layout, wired)
}

fn app() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::App,
        peer: None,
        pid: None,
    }
}

#[test]
fn only_the_cluster_may_name_a_job_on_codex_send() {
    let (_root, layout, w) = wired("jobarg");
    let ops = w.ops(
        &layout,
        Arc::new(Mutex::new(json!([]))),
        FirstScan::open(),
        Duration::ZERO,
    );
    let args = json!({"draft_folder": "/tmp", "text": "x", "job": "job-1"});
    let refused = ops.call(&app(), ops::CODEX_SEND, &args).unwrap_err();
    assert_eq!(refused.code, ErrorCode::BadRequest, "{}", refused.message);
    let node = Caller {
        tier: Tier::App,
        client: ClientKind::Node,
        peer: None,
        pid: None,
    };
    let passed = ops.call(&node, ops::CODEX_SEND, &args).unwrap_err();
    assert_ne!(passed.code, ErrorCode::BadRequest, "{}", passed.message);
}

#[test]
fn every_switch_starts_off_and_every_watcher_is_fed() {
    let (_root, _layout, w) = wired("off");
    for flag in w.switches.all() {
        assert!(!flag.is_on(), "{}", flag.name());
    }
    let status = w.limits.call("limits.status", &json!({})).unwrap();
    assert_eq!(status["accounts_on"], false);
    assert_eq!(status["watching"], false);
    for provider in status["providers"].as_array().unwrap() {
        assert_eq!(provider["registered"], true, "{provider}");
    }
    assert!(!w.limits.tick_at(1_800_000_000.0));
}

#[test]
fn the_acting_ops_check_models_against_the_daemons_own_catalog() {
    let (_root, _layout, w) = wired("catalog");
    assert!(
        std::ptr::eq(w.acting.catalog(), &*w.catalog),
        "session.new --model must read the catalog catalog.get serves"
    );
}

#[test]
fn with_accounts_on_a_round_runs_over_the_fixture_stores_and_reaches_no_login() {
    let (_root, _layout, w) = wired("on");
    assert_eq!(w.accounts_switch().name(), ACCOUNTS);
    w.accounts_switch().set(true);
    assert!(w.limits.tick_at(1_800_000_000.0));
    let status = w.limits.call("limits.status", &json!({})).unwrap();
    assert_eq!(status["watching"], true);
    for provider in status["providers"].as_array().unwrap() {
        assert_eq!(provider["active"], Value::Null, "{provider}");
        assert_eq!(provider["last_switch"], Value::Null, "{provider}");
    }
    w.accounts_switch().set(false);
    assert!(!w.limits.tick_at(1_800_000_400.0));
}

#[test]
fn the_server_serves_every_area_and_the_vault_skill_is_offered_to_install() {
    let (_root, layout, w) = wired("ops");
    let ops = w.ops(
        &layout,
        Arc::new(Mutex::new(json!([]))),
        FirstScan::open(),
        Duration::ZERO,
    );
    for op in [
        ops::SETTINGS_SWITCHES,
        ops::ACCOUNTS_LIST,
        ops::PROVIDERS_LIST,
        "limits.status",
        "cost.summary",
        "cost.report",
        ops::CATALOG_GET,
        ops::CATALOG_ROLES,
        ops::VAULT_LIST,
        ops::INBOX_LIST,
        ops::INSTALL_PLAN,
        ops::GIT_STATUS,
        ops::GIT_DIFF,
        ops::GIT_BRANCH_DIFF_STAT,
        ops::GIT_BRANCH_DIFF_FILE,
        ops::GIT_BRANCHES,
    ] {
        assert!(ops.spec(op).is_some(), "{op}");
    }
    for topic in ["accounts", "providers", "limits", "vault", "inbox"] {
        assert!(ops.topics().contains(&topic), "{topic}");
        assert_eq!(ops.topic_tier(topic), Tier::App, "{topic}");
    }
    assert_eq!(ops.spec(ops::GIT_STATUS), Some(OpSpec::APP_QUIET));
    assert_eq!(ops.spec(ops::GIT_DIFF), Some(OpSpec::APP_QUIET));
    for op in [
        ops::GIT_BRANCH_DIFF_STAT,
        ops::GIT_BRANCH_DIFF_FILE,
        ops::GIT_BRANCHES,
    ] {
        assert_eq!(ops.spec(op), Some(OpSpec::APP_QUIET), "{op}");
    }
    assert_eq!(w.switches.vault.name(), VAULT);
    let plan = ops
        .call(
            &app(),
            ops::INSTALL_PLAN,
            &json!({"what": ["notchling-vault"]}),
        )
        .unwrap();
    assert!(plan.to_string().contains("notchling-vault"), "{plan}");
    assert!(w.vault.skill_names().is_empty());
    w.close();
}

#[test]
fn the_wired_daemon_lists_every_tool_on_request_and_claude_code_by_default() {
    let (_root, layout, w) = wired("every");
    let fixture = crate::every_session_tests::Fixture {
        dir: crate::limits_ops_tests::TempDir::new("wiring-every"),
        layout: layout.clone(),
    };
    fixture.codex(
        crate::every_session_tests::CODEX_ID,
        None,
        "Fix the codex bug",
    );
    let ops = w.ops(
        &layout,
        Arc::new(Mutex::new(json!([]))),
        FirstScan::open(),
        Duration::ZERO,
    );
    let all = ops
        .call(&app(), ops::SESSIONS_LIST, &json!({"tools": "all"}))
        .unwrap();
    let ids: Vec<&str> = all
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["id"].as_str())
        .collect();
    assert_eq!(
        ids,
        vec![format!("codex:{}", crate::every_session_tests::CODEX_ID)]
    );
    let default = ops.call(&app(), ops::SESSIONS_LIST, &Value::Null).unwrap();
    assert_eq!(default, json!([]));
    w.close();
}

struct Rows(Value);

impl crate::cluster_sessions::RowSource for Rows {
    fn rows(&self, _node: &notchling_engine::cluster::NodeId) -> Result<Value, ApiError> {
        Ok(self.0.clone())
    }
}

fn peer(online: bool) -> crate::cluster_sessions::Peer {
    crate::cluster_sessions::Peer {
        id: notchling_engine::cluster::NodeId::parse("00000000000000a1").unwrap(),
        name: "w1".into(),
        online,
    }
}

fn remote_with(tag: &str, rows: Value) -> Arc<crate::cluster_sessions::RemoteSessions> {
    let dir = std::env::temp_dir().join(format!("nl-wiring-remote-{tag}-{}", std::process::id()));
    let remote = Arc::new(crate::cluster_sessions::RemoteSessions::new(dir));
    remote.sync(&[peer(true)], &Rows(rows), 0);
    remote
}

fn remote_with_a_row(tag: &str) -> Arc<crate::cluster_sessions::RemoteSessions> {
    remote_with(tag, json!([crate::cluster_sessions::agent_row("s1")]))
}

struct Forwarded;

impl crate::cluster_forward::Forward for Forwarded {
    fn forward(&self, caller: &Caller, op: &str, args: &Value) -> Option<Result<Value, ApiError>> {
        let me = notchling_engine::cluster::NodeId::parse("0123456789abcdef").unwrap();
        let (node, bare) = crate::cluster_forward::route(op, args, &me)?;
        Some(Ok(
            json!({"to": node, "args": bare, "tier": format!("{:?}", caller.tier)}),
        ))
    }
}

#[test]
fn the_list_carries_remote_rows_for_clients_but_never_for_a_node() {
    let local = Arc::new(Mutex::new(json!([{"id": "mine"}])));
    let ops = Ops::new(local).with_cluster(None, remote_with_a_row("list"));
    let listed = ops.call(&app(), ops::SESSIONS_LIST, &Value::Null).unwrap();
    let ids: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["mine", "00000000000000a1/s1"]);
    let node = Caller {
        tier: Tier::Agent,
        client: ClientKind::Node,
        peer: None,
        pid: None,
    };
    let own = ops.call(&node, ops::SESSIONS_LIST, &Value::Null).unwrap();
    assert_eq!(
        own,
        json!([{"id": "mine"}]),
        "a node is only told this node's rows"
    );
}

#[test]
fn a_qualified_id_is_routed_before_any_local_op_runs() {
    let ops = Ops::new(Arc::new(Mutex::new(json!([]))))
        .with_cluster(Some(Arc::new(Forwarded)), remote_with_a_row("route"));
    let out = ops
        .call(
            &app(),
            ops::SESSION_REPLY,
            &json!({"id": "00000000000000a1/s1", "text": "hi"}),
        )
        .unwrap();
    assert_eq!(out["to"], "00000000000000a1");
    assert_eq!(out["args"], json!({"id": "s1", "text": "hi"}));
    let local = ops.call(
        &app(),
        ops::SESSION_REPLY,
        &json!({"id": "s1", "text": "hi"}),
    );
    assert!(
        local.is_err(),
        "a bare id reaches the local op, which has no handler here"
    );
}

fn listed_ids(list: &Value) -> Vec<&str> {
    list.as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["id"].as_str())
        .collect()
}

fn publisher(
    room: &Arc<FakeRoom>,
    remote: &Arc<crate::cluster_sessions::RemoteSessions>,
    local: Arc<Mutex<Value>>,
) -> crate::cluster_wiring::Publisher {
    crate::cluster_wiring::Publisher::new(room.clone(), remote.clone(), local)
}

#[test]
fn a_local_change_is_published_with_the_remote_rows() {
    let room = Arc::new(FakeRoom::default());
    let remote = remote_with_a_row("changed");
    publisher(
        &room,
        &remote,
        Arc::new(Mutex::new(json!([{"id": "mine"}]))),
    )
    .publish();
    let sent = room.named("sessions.changed");
    assert_eq!(sent.len(), 1);
    assert_eq!(listed_ids(&sent[0]), vec!["mine", "00000000000000a1/s1"]);
}

#[test]
fn a_publish_reads_the_local_list_when_it_goes_out_and_skips_a_repeat() {
    let room = Arc::new(FakeRoom::default());
    let remote = remote_with_a_row("fresh");
    let local = Arc::new(Mutex::new(json!([{"id": "old"}])));
    let publisher = publisher(&room, &remote, local.clone());
    *local.lock().unwrap() = json!([{"id": "new"}]);
    publisher.publish();
    publisher.publish();
    let sent = room.named("sessions.changed");
    assert_eq!(
        sent.len(),
        1,
        "a second publish of the same list is dropped"
    );
    assert_eq!(listed_ids(&sent[0]), vec!["new", "00000000000000a1/s1"]);
}

fn mixed_rows() -> Value {
    let mut codex = crate::cluster_sessions::agent_row("c1");
    codex["tool"] = json!("codex");
    let mut claude = crate::cluster_sessions::agent_row("s1");
    claude["tool"] = json!("claude");
    json!([claude, codex, {"id": "junk", "name": 4}, "text"])
}

#[test]
fn a_remote_row_that_is_not_an_agent_is_dropped_and_local_rows_still_list() {
    let local = Arc::new(Mutex::new(json!([{"id": "mine"}])));
    let ops = Ops::new(local).with_cluster(None, remote_with("junk", mixed_rows()));
    let every = json!({"tools": "all"});
    let listed = ops.call(&app(), ops::SESSIONS_LIST, &every).unwrap();
    assert_eq!(
        listed_ids(&listed),
        vec!["mine", "00000000000000a1/s1", "00000000000000a1/c1"]
    );
}

#[test]
fn remote_codex_rows_stay_out_of_the_claude_list_and_the_change_event() {
    let local = Arc::new(Mutex::new(json!([{"id": "mine"}])));
    let remote = remote_with("tools", mixed_rows());
    let ops = Ops::new(local.clone()).with_cluster(None, remote.clone());
    let claude = ops.call(&app(), ops::SESSIONS_LIST, &Value::Null).unwrap();
    assert_eq!(listed_ids(&claude), vec!["mine", "00000000000000a1/s1"]);
    let every = ops
        .call(&app(), ops::SESSIONS_LIST, &json!({"tools": "all"}))
        .unwrap();
    assert!(listed_ids(&every).contains(&"00000000000000a1/c1"));
    let room = Arc::new(FakeRoom::default());
    publisher(&room, &remote, local).publish();
    let sent = room.named("sessions.changed");
    assert_eq!(listed_ids(&sent[0]), vec!["mine", "00000000000000a1/s1"]);
}

struct Counted(std::sync::atomic::AtomicUsize);

impl crate::cluster_sessions::RowSource for Counted {
    fn rows(&self, _node: &notchling_engine::cluster::NodeId) -> Result<Value, ApiError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(json!([crate::cluster_sessions::agent_row("s1")]))
    }
}

#[test]
fn a_worker_lists_and_announces_no_remote_rows_and_asks_for_none() {
    use std::sync::atomic::Ordering::SeqCst;
    let dir = std::env::temp_dir().join(format!("nl-wiring-worker-{}", std::process::id()));
    let client = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let gate = client.clone();
    let remote = Arc::new(
        crate::cluster_sessions::RemoteSessions::new(dir)
            .with_gate(Arc::new(move || gate.load(SeqCst))),
    );
    let asked = Counted(std::sync::atomic::AtomicUsize::new(0));
    remote.sync(&[peer(true)], &asked, 0);
    assert_eq!(asked.0.load(SeqCst), 0, "a Worker sends no node.sessions");
    let local = Arc::new(Mutex::new(json!([{"id": "mine"}])));
    let ops = Ops::new(local.clone()).with_cluster(None, remote.clone());
    for args in [Value::Null, json!({"tools": "all"})] {
        let list = ops.call(&app(), ops::SESSIONS_LIST, &args).unwrap();
        assert_eq!(listed_ids(&list), vec!["mine"]);
    }
    let room = Arc::new(FakeRoom::default());
    publisher(&room, &remote, local).publish();
    assert_eq!(
        room.named("sessions.changed"),
        vec![json!([{"id": "mine"}])]
    );
    client.store(true, SeqCst);
    remote.sync(&[peer(true)], &asked, 0);
    assert_eq!(
        asked.0.load(SeqCst),
        1,
        "decided from the record at use time"
    );
    assert_eq!(remote.rows().len(), 1);
}

#[test]
fn the_daemons_scan_publishes_through_the_cluster_parts() {
    let root = std::env::temp_dir().join(format!("nl-wiring-scan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let layout = Layout::resolve(None, Some(root.as_os_str()), || None).unwrap();
    let room = Arc::new(FakeRoom::default());
    let sessions = Arc::new(Mutex::new(json!([])));
    let w = Wired::build(&layout, sessions.clone(), room.clone(), (0, 0));
    *sessions.lock().unwrap() = json!([{"id": "mine"}]);
    w.cluster.local_changed();
    assert_eq!(
        room.named("sessions.changed"),
        vec![json!([{"id": "mine"}])]
    );
    w.close();
    let _ = std::fs::remove_dir_all(&root);
}

fn shell_row() -> Value {
    let mut row = crate::cluster_sessions::agent_row("shell:j1");
    row["tool"] = json!("shell");
    row["command"] = json!("make");
    row
}

fn with_shell_and_agy() -> Value {
    let mut agy = crate::cluster_sessions::agent_row("agy:a1");
    agy["tool"] = json!("antigravity");
    json!([crate::cluster_sessions::agent_row("s1"), shell_row(), agy])
}

fn codex_row() -> Value {
    let mut row = crate::cluster_sessions::agent_row("codex:c1");
    row["tool"] = json!("codex");
    row
}

fn with_shell_codex_and_agy() -> Value {
    let mut rows = with_shell_and_agy();
    rows.as_array_mut().unwrap().push(codex_row());
    rows
}

#[test]
fn remote_shell_rows_come_only_when_asked_for() {
    let local = Arc::new(Mutex::new(json!([{"id": "mine"}])));
    let ops = Ops::new(local).with_cluster(None, remote_with("shell", with_shell_and_agy()));
    for args in [Value::Null, json!({}), json!({"tools": "claude"})] {
        let list = ops.call(&app(), ops::SESSIONS_LIST, &args).unwrap();
        assert_eq!(listed_ids(&list), vec!["mine", "00000000000000a1/s1"]);
    }
    let shell = ops
        .call(&app(), ops::SESSIONS_LIST, &json!({"remote": "shell"}))
        .unwrap();
    assert_eq!(
        listed_ids(&shell),
        vec!["mine", "00000000000000a1/s1", "00000000000000a1/shell:j1"],
        "Antigravity rows stay out: Chat reads those itself"
    );
    let bad = ops.call(&app(), ops::SESSIONS_LIST, &json!({"remote": "agy"}));
    assert!(bad.is_err());
    let node = Caller {
        tier: Tier::Agent,
        client: ClientKind::Node,
        peer: None,
        pid: None,
    };
    let own = ops
        .call(&node, ops::SESSIONS_LIST, &json!({"remote": "shell"}))
        .unwrap();
    assert_eq!(own, json!([{"id": "mine"}]));
}

#[test]
fn remote_codex_rows_come_only_when_asked_for_and_apart_from_the_shell_ones() {
    let local = Arc::new(Mutex::new(json!([{"id": "mine"}])));
    let ops = Ops::new(local).with_cluster(None, remote_with("codex", with_shell_codex_and_agy()));
    let asked = |remote: Value| {
        ops.call(&app(), ops::SESSIONS_LIST, &json!({"remote": remote}))
            .unwrap()
    };
    let (s1, shell, codex) = (
        "00000000000000a1/s1",
        "00000000000000a1/shell:j1",
        "00000000000000a1/codex:c1",
    );
    assert_eq!(listed_ids(&asked(json!("codex"))), vec!["mine", s1, codex]);
    assert_eq!(listed_ids(&asked(json!("shell"))), vec!["mine", s1, shell]);
    assert_eq!(
        listed_ids(&asked(json!(["shell", "codex"]))),
        vec!["mine", s1, shell, codex]
    );
    let plain = ops.call(&app(), ops::SESSIONS_LIST, &json!({})).unwrap();
    assert_eq!(listed_ids(&plain), vec!["mine", s1]);
    for bad in [json!("agy"), json!(["codex", "agy"]), json!([1]), json!(7)] {
        let got = ops.call(&app(), ops::SESSIONS_LIST, &json!({"remote": bad}));
        assert!(got.is_err(), "{bad}");
    }
}

#[test]
fn a_change_in_remote_codex_rows_is_announced_apart_from_the_others() {
    let room = Arc::new(FakeRoom::default());
    let local = Arc::new(Mutex::new(json!([{"id": "mine"}])));
    let remote = remote_with(
        "codex-event",
        json!([crate::cluster_sessions::agent_row("s1")]),
    );
    let publisher = publisher(&room, &remote, local);
    publisher.publish();
    assert!(
        room.named("sessions.remote.codex").is_empty(),
        "no Codex rows, nothing to say"
    );
    remote.mark_dirty(&peer(true).id);
    remote.sync(&[peer(true)], &Rows(with_shell_codex_and_agy()), 1);
    publisher.publish();
    publisher.publish();
    let sent = room.named("sessions.remote.codex");
    assert_eq!(sent.len(), 1, "a repeat is dropped");
    assert_eq!(listed_ids(&sent[0]), vec!["00000000000000a1/codex:c1"]);
    assert_eq!(room.named("sessions.remote.shell").len(), 1);
    assert_eq!(
        room.named("sessions.changed").len(),
        1,
        "other clients' event is unchanged"
    );
    remote.clear();
    publisher.publish();
    assert_eq!(room.named("sessions.remote.codex").last(), Some(&json!([])));
}

#[test]
fn a_change_in_remote_shell_rows_is_announced_apart_from_sessions_changed() {
    let room = Arc::new(FakeRoom::default());
    let local = Arc::new(Mutex::new(json!([{"id": "mine"}])));
    let remote = remote_with(
        "shell-event",
        json!([crate::cluster_sessions::agent_row("s1")]),
    );
    let publisher = publisher(&room, &remote, local);
    publisher.publish();
    assert!(
        room.named("sessions.remote.shell").is_empty(),
        "no shell rows, nothing to say"
    );
    remote.mark_dirty(&peer(true).id);
    remote.sync(&[peer(true)], &Rows(with_shell_and_agy()), 1);
    publisher.publish();
    publisher.publish();
    let sent = room.named("sessions.remote.shell");
    assert_eq!(sent.len(), 1, "a repeat is dropped");
    assert_eq!(listed_ids(&sent[0]), vec!["00000000000000a1/shell:j1"]);
    assert_eq!(
        room.named("sessions.changed").len(),
        1,
        "other clients' event is unchanged"
    );
    remote.clear();
    publisher.publish();
    assert_eq!(room.named("sessions.remote.shell").last(), Some(&json!([])));
}

#[test]
fn a_move_is_recovered_only_when_the_daemon_asks_after_it_has_started() {
    use notchling_engine::cluster::move_home::{MoveSaga, MoveStep};
    let root = std::env::temp_dir().join(format!("nl-wiring-recover-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let layout = Layout::resolve(None, Some(root.as_os_str()), || None).unwrap();
    let _cleanup = Root(root.clone());
    let a = notchling_engine::cluster::NodeId::parse(&"a".repeat(16)).unwrap();
    let b = notchling_engine::cluster::NodeId::parse(&"b".repeat(16)).unwrap();
    let mut saga = MoveSaga::new(a, b, 1, false);
    saga.step = MoveStep::StopOld;
    saga.moving = vec!["job-1".to_string()];
    std::fs::create_dir_all(layout.cluster_dir()).unwrap();
    std::fs::write(
        layout
            .cluster_dir()
            .join(crate::cluster_move_run::SAGA_FILE),
        serde_json::to_vec(&saga).unwrap(),
    )
    .unwrap();
    let sessions = Arc::new(Mutex::new(json!([])));
    let w = Wired::build(&layout, sessions, Arc::new(FakeRoom::default()), (0, 0));
    assert!(w.cluster.node.is_ok(), "the fixture daemon has a node");
    w.cluster
        .runner
        .wait_done(std::time::Duration::from_secs(5));
    assert_eq!(
        w.cluster.runner.status()["step"],
        "stop_old",
        "building the daemon starts nothing"
    );
    w.cluster.recover_moves();
    w.cluster
        .runner
        .wait_done(std::time::Duration::from_secs(10));
    assert_ne!(w.cluster.runner.status()["step"], "stop_old");
}

#[test]
fn an_agent_search_with_no_organization_or_no_reads_answers_empty_with_no_session() {
    let (_root, layout, w) = wired("search-org");
    let agent = Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
        peer: None,
        pid: Some(1),
    };
    let args = json!({"text": "anything"});
    let sessions = Arc::new(Mutex::new(json!([])));
    let bare = Ops::new(sessions.clone())
        .with_pages(Arc::new(PagesOps::new(PagesSetup::from_layout(&layout))));
    let full = w.ops(&layout, sessions, FirstScan::open(), Duration::ZERO);
    for ops in [&bare, &full] {
        let reply = ops.call(&agent, ops::SEARCH_QUERY, &args).unwrap();
        assert_eq!(reply["reason"], json!("no_session"), "{reply}");
        assert_eq!(reply["pages"], json!([]));
        assert_eq!(reply["sessions"], json!([]));
    }
}
