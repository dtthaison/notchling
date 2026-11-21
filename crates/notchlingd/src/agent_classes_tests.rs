use crate::acting_ops_tests::FakeRoom;
use crate::cost_report_ops::COST_REPORT;
use crate::daemon::FirstScan;
use crate::daemon_wiring::{Ops, Wired};
use crate::layout::Layout;
use crate::limits_ops::COST_SUMMARY;
use crate::log_audit_tests::rig as rig_log;
use crate::log_checks_tests::wire;
use crate::log_ops::LogOps;
use crate::op_kinds::known_ops;
use notchling_local_api::{ops, Handler, OpSpec, Tier};
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Root(PathBuf);

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn rig() -> (Root, Ops) {
    let root = std::env::temp_dir().join(format!("nl-classes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let layout = Layout::resolve(None, Some(root.as_os_str()), || None).unwrap();
    let sessions = Arc::new(Mutex::new(json!([])));
    let wired = Wired::build(
        &layout,
        sessions.clone(),
        Arc::new(FakeRoom::default()),
        (0, 0),
    );
    let logged = wire(rig_log("classes-log"));
    let log = LogOps::new(logged.audit.clone(), logged.checker.clone());
    let ops = wired
        .ops(&layout, sessions, FirstScan::open(), Duration::ZERO)
        .with_group(Arc::new(log));
    (Root(root), ops)
}

const READS: [&str; 27] = [
    ops::SESSIONS_LIST,
    ops::SETTINGS_LIST,
    ops::SETTINGS_GET,
    ops::SETTINGS_ASKED,
    ops::SETTINGS_SWITCHES,
    ops::SETTINGS_GET_ACTING,
    ops::CATALOG_GET,
    ops::MODELS_CHECK,
    ops::COST_PROFILE_GET,
    ops::COST_SETUP_PLAN,
    ops::COST_SETUP_CHECK,
    ops::VERSIONS_GET,
    ops::CLUSTER_STATUS,
    ops::PAGES_LIST,
    ops::PAGES_FIND,
    ops::SEARCH_QUERY,
    ops::AGY_SESSIONS,
    ops::ASK_WAIT,
    ops::ASK_MINE,
    ops::VAULT_FIND,
    ops::LOG_VERIFY,
    ops::DAEMON_CAPABILITIES,
    ops::CONSENT_GET,
    ops::CONSENT_MINE,
    ops::SHELLS_MINE,
    COST_SUMMARY,
    COST_REPORT,
];

const ACTS: [&str; 12] = [
    ops::SETTINGS_SET,
    ops::MODELS_UPDATE,
    ops::CLUSTER_JOIN,
    ops::CLUSTER_DISPATCH,
    ops::SESSION_RELOAD,
    ops::SESSION_QUESTION,
    ops::ASK_OPEN,
    ops::ASK_ANSWER,
    ops::VAULT_USE,
    ops::VAULT_REQUEST,
    ops::CONSENT_CANCEL,
    ops::SHELLS_STOP,
];

const HOOKS: [&str; 4] = [
    ops::HOOK_REQUEST,
    ops::AGY_HOOK,
    ops::AGY_SIDECAR_TAKE,
    ops::AGY_SIDECAR_REPORT,
];

#[test]
fn every_agent_tier_op_is_a_read_an_acting_op_or_a_hook_op() {
    let (_root, handler) = rig();
    let mut agent_ops = Vec::new();
    for op in known_ops() {
        let Some(spec) = handler.spec(op) else {
            continue;
        };
        if spec.tier != Tier::Agent {
            assert!(!spec.acts && !spec.hook, "{op}");
            continue;
        }
        agent_ops.push(*op);
        let classes = [
            spec == OpSpec::AGENT_READ,
            spec == OpSpec::AGENT_ACTS,
            spec == OpSpec::AGENT_HOOK,
        ];
        assert_eq!(classes.iter().filter(|c| **c).count(), 1, "{op}");
    }
    assert!(agent_ops.len() >= 30, "{agent_ops:?}");
    for op in READS {
        assert_eq!(handler.spec(op), Some(OpSpec::AGENT_READ), "{op}");
    }
    for op in ACTS {
        assert_eq!(handler.spec(op), Some(OpSpec::AGENT_ACTS), "{op}");
    }
    for op in HOOKS {
        assert_eq!(handler.spec(op), Some(OpSpec::AGENT_HOOK), "{op}");
    }
    for op in &agent_ops {
        let listed = READS.contains(op) || ACTS.contains(op) || HOOKS.contains(op);
        assert!(listed, "{op} is an agent op in no list");
    }
}

#[test]
fn no_op_that_writes_runs_or_touches_the_vault_is_an_agent_read() {
    let (_root, handler) = rig();
    for op in [
        ops::SETTINGS_SET,
        ops::MODELS_UPDATE,
        ops::CLUSTER_JOIN,
        ops::CLUSTER_DISPATCH,
        ops::SESSION_RELOAD,
        ops::SESSION_QUESTION,
        ops::ASK_OPEN,
        ops::ASK_ANSWER,
        ops::VAULT_USE,
        ops::VAULT_REQUEST,
        ops::CONSENT_CANCEL,
    ] {
        let spec = handler.spec(op).expect(op);
        assert!(spec.records_agent_call(), "{op}");
    }
}
