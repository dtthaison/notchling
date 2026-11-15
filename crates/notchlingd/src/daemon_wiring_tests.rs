use super::*;
use crate::acting_ops_tests::FakeRoom;
use crate::switches::{ACCOUNTS, VAULT};
use notchling_local_api::{ops, ClientKind};
use serde_json::json;

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
        ops::CATALOG_GET,
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

fn remote_with_a_row(tag: &str) -> Arc<crate::cluster_sessions::RemoteSessions> {
    let dir = std::env::temp_dir().join(format!("nl-wiring-remote-{tag}-{}", std::process::id()));
    let remote = Arc::new(crate::cluster_sessions::RemoteSessions::new(dir));
    remote.sync(&[peer(true)], &Rows(json!([{"id": "s1"}])), 0);
    remote
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

#[test]
fn a_local_change_is_published_with_the_remote_rows() {
    let room = FakeRoom::default();
    let remote = remote_with_a_row("changed");
    crate::cluster_wiring::publish_merged(&room, &remote, &json!([{"id": "mine"}]));
    let sent = room.named("sessions.changed");
    assert_eq!(sent.len(), 1);
    let ids: Vec<&str> = sent[0]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["mine", "00000000000000a1/s1"]);
}

#[test]
fn the_daemons_scan_publishes_through_the_cluster_parts() {
    let root = std::env::temp_dir().join(format!("nl-wiring-scan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let layout = Layout::resolve(None, Some(root.as_os_str()), || None).unwrap();
    let room = Arc::new(FakeRoom::default());
    let w = Wired::build(
        &layout,
        Arc::new(Mutex::new(json!([]))),
        room.clone(),
        (0, 0),
    );
    w.cluster.local_changed(&json!([{"id": "mine"}]));
    assert_eq!(
        room.named("sessions.changed"),
        vec![json!([{"id": "mine"}])]
    );
    w.close();
    let _ = std::fs::remove_dir_all(&root);
}
