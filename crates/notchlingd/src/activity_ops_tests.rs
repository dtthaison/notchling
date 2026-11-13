use crate::activity_ops::ActivityOps;
use crate::layout::Layout;
use crate::limits_ops_tests::TempDir;
use crate::op_group::OpGroup;
use notchling_local_api::{ops, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::json;

fn app() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::App,
        peer: None,
        pid: None,
    }
}

fn rig(tag: &str) -> (TempDir, ActivityOps) {
    let dir = TempDir::new(tag);
    let layout = Layout::resolve(None, Some(dir.root().as_os_str()), || None).unwrap();
    dir.write(
        "notchling/audit.jsonl",
        &[
            r#"{"time_ms":2000,"op":"session.reply","tier":"app","peer":"/x/notchling-app","result":"ok"}"#,
            r#"{"time_ms":3000,"op":"vault.pending","tier":"agent","peer":"/usr/bin/curl","result":"forbidden"}"#,
        ]
        .join("\n"),
    );
    dir.write(
        "AgentConsole/bridge/remote-log.jsonl",
        r#"{"time":"1970-01-01T00:00:04.000Z","device":"Pixel","op":"message","length":5}"#,
    );
    dir.write(
        "notchling/account-log.json",
        r#"[{"at":1.0,"id":"abcdef0123456789"}]"#,
    );
    (dir, ActivityOps::for_daemon(&layout))
}

#[test]
fn it_is_an_app_tier_quiet_read_and_nothing_else_is_served() {
    let (_dir, activity) = rig("activity-spec");
    let spec = activity.spec(ops::ACTIVITY_LIST).unwrap();
    assert_eq!(spec, OpSpec::APP_QUIET);
    assert_eq!(spec.tier, Tier::App);
    assert!(!spec.approval);
    assert_eq!(activity.spec(ops::SESSION_REPLY), None);
    assert_eq!(activity.spec(ops::INBOX_LIST), None);
}

#[test]
fn it_reads_the_audit_the_phone_log_and_the_account_log_under_the_data_root() {
    let (_dir, activity) = rig("activity-read");
    let reply = activity
        .call(&app(), ops::ACTIVITY_LIST, &json!({}))
        .unwrap();
    let rows = reply["rows"].as_array().unwrap();
    let sources: Vec<&str> = rows.iter().map(|r| r["source"].as_str().unwrap()).collect();
    assert_eq!(sources, ["account", "phone", "audit", "audit"]);
    assert_eq!(reply["truncated"], false);
    assert_eq!(rows[2]["kind"], "refused");
    assert_eq!(rows[2]["who"], "curl");
}

#[test]
fn kinds_since_and_limit_are_passed_through_and_bad_ones_are_refused() {
    let (_dir, activity) = rig("activity-args");
    let reply = activity
        .call(
            &app(),
            ops::ACTIVITY_LIST,
            &json!({"kinds": ["refused", "acted"], "since": 2000, "limit": 5}),
        )
        .unwrap();
    let rows = reply["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r["time_ms"].as_u64().unwrap() > 2000));
    let err = activity
        .call(&app(), ops::ACTIVITY_LIST, &json!({"kinds": ["nope"]}))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
}

#[test]
fn a_data_root_with_no_logs_answers_an_empty_list() {
    let dir = TempDir::new("activity-empty");
    let layout = Layout::resolve(None, Some(dir.root().as_os_str()), || None).unwrap();
    let reply = ActivityOps::for_daemon(&layout)
        .call(&app(), ops::ACTIVITY_LIST, &json!({}))
        .unwrap();
    assert_eq!(reply["rows"], json!([]));
}
