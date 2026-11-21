use crate::acting_ops_tests::FakeRoom;
use crate::commands::{COMMANDS, INTERNAL, PLANNED, USER_ONLY};
use crate::daemon::FirstScan;
use crate::daemon_wiring::{Ops, Wired};
use crate::layout::Layout;
use crate::op_kinds::{home, known_ops, Home, HOMES};
use crate::quiet_ops::QUIET_READS;
use notchling_events::registry;
use notchling_local_api::{ops, Handler, OpHome, OpSpec, Tier};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Root(PathBuf);

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn rig(tag: &str) -> (Root, Wired, Ops) {
    let root = std::env::temp_dir().join(format!("nl-kinds-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let layout = Layout::resolve(None, Some(root.as_os_str()), || None).unwrap();
    let sessions = Arc::new(Mutex::new(json!([])));
    let wired = Wired::build(
        &layout,
        sessions.clone(),
        Arc::new(FakeRoom::default()),
        (0, 0),
    );
    let ops = wired.ops(&layout, sessions, FirstScan::open(), Duration::ZERO);
    (Root(root), wired, ops)
}

const OWN_QUIET: [&str; 13] = [
    ops::ORGANIZATIONS_LIST,
    ops::ORGANIZATIONS_FOLDERS_PREVIEW,
    ops::CATALOG_ROLES,
    ops::GIT_STATUS,
    ops::GIT_DIFF,
    ops::GIT_BRANCH_DIFF_STAT,
    ops::GIT_BRANCH_DIFF_FILE,
    ops::GIT_BRANCHES,
    ops::GIT_WORKTREES,
    ops::GIT_BRANCH_LIST,
    ops::ACTIVITY_LIST,
    ops::CODEX_CHILDREN,
    ops::CONSENT_LIST,
];

const NEVER_QUIET: [&str; 141] = [
    ops::ORGANIZATIONS_CREATE,
    ops::ORGANIZATIONS_RENAME,
    ops::ORGANIZATIONS_COLOR_SET,
    ops::ORGANIZATIONS_FOLDERS_SET,
    ops::ORGANIZATIONS_DELETE,
    ops::HELLO,
    ops::LOG_VERIFY,
    ops::LOG_CHAIN_START,
    ops::LOG_AUDIT_DELETE,
    ops::DAEMON_STATUS,
    ops::DAEMON_CAPABILITIES,
    ops::SESSIONS_LIST,
    ops::EVENTS_SUBSCRIBE,
    ops::SESSION_REPLY,
    ops::SESSION_ANSWER,
    ops::SESSION_PERMISSION,
    ops::SESSION_INTERRUPT,
    ops::SESSION_STOP,
    ops::SESSION_REMOVE,
    ops::SESSION_TAKE_BACK,
    ops::SESSION_NEW,
    ops::FOLDER_TRUST,
    ops::SESSION_MOVE,
    ops::SESSION_COMMAND,
    ops::SESSION_HELD_DISCARD,
    ops::SESSION_DIALOG_PICK,
    ops::SESSION_DIALOG_DISMISS,
    ops::SESSION_RELOAD,
    ops::AGY_SIDECAR_TAKE,
    ops::AGY_SIDECAR_REPORT,
    ops::AGY_SIDECAR_SETUP,
    ops::AGY_HOOK,
    ops::SESSION_SEND,
    ops::OUTBOX_LIST,
    ops::OUTBOX_EDIT,
    ops::OUTBOX_REMOVE,
    ops::OUTBOX_HOLD,
    ops::OUTBOX_RESEND,
    ops::OUTBOX_DISCARD,
    ops::SETTINGS_SET_ACTING,
    ops::SETTINGS_GET_ACTING,
    ops::PROMPTS_LIST,
    ops::SESSION_QUESTION,
    ops::HOOK_REQUEST,
    ops::CODEX_SEND,
    ops::CODEX_STOP,
    ops::CODEX_DELETE,
    ops::AGY_SESSIONS,
    ops::AGY_STATE,
    ops::AGY_CONVERSATION,
    ops::AGY_OPEN,
    ops::AGY_SEND,
    ops::AGY_CANCEL,
    ops::AGY_SET_OPTION,
    ops::AGY_PERMISSION,
    ops::BRIDGE_SET,
    ops::BRIDGE_PAIR_BEGIN,
    ops::BRIDGE_PAIR_ANSWER,
    ops::BRIDGE_REVOKE,
    ops::SEARCH_QUERY,
    ops::PAGES_LIST,
    ops::PAGES_FIND,
    ops::INSTALL_PLAN,
    ops::INSTALL_APPLY,
    ops::INSTALL_REMOVE,
    ops::SETTINGS_SET_SWITCH,
    ops::SETTINGS_SWITCHES,
    ops::ACCOUNTS_SWITCH,
    ops::ACCOUNTS_ADD_BEGIN,
    ops::ACCOUNTS_ADD_FINISH,
    ops::ACCOUNTS_REMOVE,
    ops::ACCOUNTS_MOVE_ALL,
    ops::ACCOUNTS_RESULT,
    ops::LANES_CREATE,
    ops::LANES_REMOVE,
    ops::LANES_SWITCH,
    ops::PROVIDERS_SWITCH,
    ops::PROVIDERS_ADD_BEGIN,
    ops::PROVIDERS_ADD_CANCEL,
    ops::PROVIDERS_REMOVE,
    ops::VAULT_FIND,
    ops::VAULT_USE,
    ops::VAULT_REQUEST,
    ops::VAULT_LIST,
    ops::VAULT_SAVE,
    ops::VAULT_REMOVE,
    ops::VAULT_APPROVAL_ANSWER,
    ops::VAULT_REQUEST_CANCEL,
    ops::VAULT_PENDING,
    ops::VAULT_ADMIN,
    ops::CATALOG_GET,
    ops::MODELS_CHECK,
    ops::MODELS_UPDATE,
    ops::COST_PROFILE_GET,
    ops::COST_PROFILE_SET,
    ops::COST_SETUP_PLAN,
    ops::COST_SETUP_APPLY,
    ops::COST_SETUP_UNDO,
    ops::COST_SETUP_CHECK,
    ops::INBOX_LIST,
    ops::INBOX_ANSWER,
    ops::INBOX_SEEN,
    ops::SETTINGS_LIST,
    ops::SETTINGS_GET,
    ops::SETTINGS_SET,
    ops::SETTINGS_ASKED,
    ops::VERSIONS_GET,
    ops::VERSIONS_CHECK,
    ops::VERSIONS_UPDATE_LOCAL,
    ops::CLUSTER_STATUS,
    ops::CLUSTER_CREATE,
    ops::CLUSTER_JOIN_TOKEN,
    ops::CLUSTER_JOIN,
    ops::CLUSTER_MEMBER_SET,
    ops::CLUSTER_REMOVE,
    ops::CLUSTER_MAIN_SET,
    ops::CLUSTER_MOVE_HOME,
    ops::CLUSTER_MOVE_NOW,
    ops::CLUSTER_ALLOW_SET,
    ops::CLUSTER_FOLLOW,
    ops::CLUSTER_DISPATCH,
    ops::SHELL_STOP,
    ops::CLUSTER_RETURN,
    ops::CLUSTER_FAILOVER_SET,
    ops::CLUSTER_PROMOTE,
    ops::CLUSTER_TAKE_BACK,
    ops::CLUSTER_CONTINUE_HERE,
    ops::CLUSTER_SIDE_COPY_HIDE,
    ops::CLUSTER_INVITE,
    ops::ASK_OPEN,
    ops::ASK_WAIT,
    ops::ASK_MINE,
    ops::ASK_ANSWER,
    ops::ASK_DISMISS,
    ops::ASK_LIST,
    ops::TAILSCALE_SWITCH,
    ops::CONSENT_GET,
    ops::CONSENT_MINE,
    ops::CONSENT_CANCEL,
    ops::CONSENT_ANSWER,
    ops::CONSENT_OPENED,
];

const NEVER_QUIET_LIMITS: [&str; 3] = ["limits.set_mode", "cost.summary", "cost.report"];

fn every_op_name() -> Vec<String> {
    let wire = include_str!("../../local-api/src/wire.rs");
    let start = wire.find("pub mod ops {").unwrap();
    let end = wire[start..].find("\n}\n").unwrap() + start;
    let mut names: Vec<String> = wire[start..end]
        .lines()
        .filter(|l| l.trim_start().starts_with("pub const"))
        .filter_map(|l| l.split('"').nth(1).map(str::to_owned))
        .collect();
    for limits in [
        "limits.status",
        "limits.set_mode",
        "cost.summary",
        "cost.report",
        "limits.usage",
    ] {
        names.push(limits.to_owned());
    }
    names
}

#[test]
fn every_op_has_a_home() {
    let names = every_op_name();
    assert!(names.len() > 70, "{}", names.len());
    for name in &names {
        assert!(
            HOMES.iter().any(|(op, _)| op == name),
            "{name} has no home in HOMES"
        );
    }
    for (op, _) in HOMES {
        assert!(
            names.iter().any(|name| name == op),
            "{op} is in HOMES but not in wire.rs"
        );
    }
    let mut seen: Vec<&str> = HOMES.iter().map(|(op, _)| *op).collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), HOMES.len(), "an op is listed twice");
    assert_eq!(known_ops().len(), HOMES.len());
}

#[test]
fn named_kinds_exist() {
    for (op, home) in HOMES {
        if let Home::Kind(kind) = home {
            assert!(registry::find(kind).is_some(), "{op} names {kind}");
        }
    }
}

#[test]
fn own_quiet_ops_are_quiet() {
    let (_root, wired, ops) = rig("own");
    for op in OWN_QUIET {
        assert_eq!(home(op), OpHome::Quiet, "{op}");
        assert_eq!(ops.spec(op), Some(OpSpec::APP_QUIET), "{op}");
    }
    wired.close();
}

#[test]
fn quiet_reads_match_quiet_ops_rs() {
    let (_root, wired, ops) = rig("reads");
    for op in QUIET_READS {
        assert_eq!(home(op), OpHome::Quiet, "{op}");
        assert_eq!(ops.spec(op), Some(OpSpec::APP_QUIET), "{op}");
    }
    wired.close();
}

#[test]
fn never_quiet_ops_are_not_quiet() {
    let (_root, wired, ops) = rig("never");
    for op in NEVER_QUIET.iter().chain(NEVER_QUIET_LIMITS.iter()) {
        assert!(!ops.spec(op).is_some_and(|s| s.quiet_ok), "{op}");
    }
    wired.close();
}

#[test]
fn an_op_with_its_own_kind_is_never_quiet_or_called() {
    for (op, kind) in HOMES {
        if matches!(kind, Home::Kind(_)) {
            assert_eq!(home(op), OpHome::Kind, "{op}");
        }
    }
    assert_eq!(home("not.an.op"), OpHome::Called);
}

#[test]
fn every_op_is_classified_once_by_the_old_lists() {
    let names = every_op_name();
    for name in &names {
        let op = name.as_str();
        let in_quiet = QUIET_READS.contains(&op);
        let in_own = OWN_QUIET.contains(&op);
        let in_never = NEVER_QUIET.contains(&op) || NEVER_QUIET_LIMITS.contains(&op);
        assert_eq!(
            [in_quiet, in_own, in_never].iter().filter(|x| **x).count(),
            1,
            "{op} must be in exactly one of QUIET_READS, OWN_QUIET and NEVER_QUIET"
        );
    }
}

#[test]
fn a_quiet_read_poll_that_syncs_is_recorded_when_it_asks_for_the_sync() {
    let (_root, wired, ops) = rig("loud");
    for op in ["accounts.list", "providers.list"] {
        assert!(!ops.loud(op, &json!({"sync": false})), "{op}");
        assert!(ops.loud(op, &json!({"sync": true})), "{op}");
    }
    for op in ["providers.usage", "limits.usage"] {
        assert!(!ops.loud(op, &json!({})), "{op}");
        assert!(ops.loud(op, &json!({"refresh": true})), "{op}");
    }
    assert!(!ops.loud("lanes.list", &json!({"sync": true})));
    assert!(!ops.loud("bridge.status", &Value::Null));
    wired.close();
}

const SERVED_ELSEWHERE: [&str; 6] = [
    ops::LOG_CHAIN_START,
    ops::HELLO,
    ops::DAEMON_STATUS,
    ops::EVENTS_SUBSCRIBE,
    ops::LOG_VERIFY,
    ops::LOG_AUDIT_DELETE,
];

#[test]
fn the_registry_and_the_tiers_agree() {
    let (_root, wired, ops) = rig("tiers");
    let mut wrong: Vec<String> = Vec::new();
    let mut check = |op: &str, want: Option<Tier>, what: &str| {
        let Some(spec) = ops.spec(op) else {
            if !SERVED_ELSEWHERE.contains(&op) {
                wrong.push(format!("{op} ({what}) has no spec"));
            }
            return;
        };
        if want.is_some_and(|tier| spec.tier != tier) {
            wrong.push(format!("{op} ({what}) is {:?}", spec.tier));
        }
    };
    for row in COMMANDS {
        for used in row.ops {
            let want = if used.tier == "app" {
                Tier::App
            } else {
                Tier::Agent
            };
            check(used.op, Some(want), &format!("command {}", row.command));
        }
    }
    for user in USER_ONLY {
        check(user.op, Some(Tier::App), "user only");
    }
    for planned in PLANNED {
        check(planned.op, None, "planned");
    }
    for internal in INTERNAL {
        let want = match internal.why {
            "app_ui" => Some(Tier::App),
            _ => None,
        };
        check(internal.op, want, internal.why);
    }
    wired.close();
    assert!(wrong.is_empty(), "{wrong:#?}");
}
