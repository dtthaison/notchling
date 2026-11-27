use super::{
    import_account_log, import_legacy_audit, import_remote_log, rebuild_from_months, ActivityRow,
};
use notchling_events::audit_line::sealed_line;
use notchling_events::envelope::{Actor, ActorTier, Envelope, Via};
use notchling_events::kinds::choices::CallerTier;
use notchling_events::kinds::security::{ApiRefused, RefusedWhy, StopCause};
use notchling_events::spec::{Outcome, Proc};
use notchling_events::{registry, Event, Exe, Hex, Millis, Op, RefMode, Salt};
use serde_json::Value;

const AUDIT: &str = include_str!("../../tests/fixtures/activity/audit.jsonl");
const REMOTE: &str = include_str!("../../tests/fixtures/activity/remote-log.jsonl");
const ACCOUNT: &str = include_str!("../../tests/fixtures/activity/account-log.json");
const HOST: &str = "3f9a0c1b7d2e4a56";

fn at(rows: &[ActivityRow], ts: i64) -> &ActivityRow {
    rows.iter()
        .find(|r| r.ts == ts)
        .unwrap_or_else(|| panic!("no row at {ts}"))
}

fn f(row: &ActivityRow) -> Value {
    row.fields()
}

#[test]
fn legacy_audit_maps_each_shape() {
    let imported = import_legacy_audit(AUDIT, HOST);
    assert_eq!(imported.skipped, 1);
    let rows = &imported.rows;
    assert!(rows
        .iter()
        .all(|r| r.legacy && r.host == HOST && r.seq == 0));

    let called = at(rows, 1_000);
    assert_eq!(called.kind, "op.called");
    assert_eq!(f(called)["op"], "session.reply");
    assert_eq!(called.actor_tier.as_deref(), Some("app"));

    let refused = at(rows, 2_000);
    assert_eq!(refused.kind, "api.refused");
    assert_eq!(refused.outcome, "refused");
    assert_eq!(refused.actor_exe.as_deref(), Some("curl"));
    assert_eq!(f(refused)["why"], "forbidden");
    assert_eq!(f(refused)["tier"], "agent");

    let failed = at(rows, 3_000);
    assert_eq!(failed.kind, "op.failed");
    assert_eq!(failed.outcome, "failed");
    assert!(!failed.f.contains("not signed in"));

    let switched = at(rows, 4_000);
    assert_eq!(switched.kind, "switch.changed");
    assert_eq!(f(switched)["switch"], "accounts");
    assert_eq!(f(switched)["on"], true);

    let peer = at(rows, 7_000);
    assert_eq!(peer.kind, "api.unknown_peer");
    assert_eq!(f(peer)["why"], "peer_unknown");

    let ignored = at(rows, 13_000);
    assert_eq!(ignored.kind, "switch.changed");
    assert_eq!(f(ignored)["cause"], "file_said_on_ignored");
    assert_eq!(f(ignored)["switch"], "acting");

    let approval = at(rows, 14_000);
    assert_eq!(approval.kind, "api.refused");
    assert_eq!(f(approval)["why"], "needs_approval");
}

#[test]
fn vault_use_names_are_kept() {
    let rows = import_legacy_audit(AUDIT, HOST).rows;
    let used = at(&rows, 8_000);
    assert_eq!(used.kind, "vault.secret_used");
    assert_eq!(
        f(used)["secrets"],
        serde_json::json!(["GITHUB_TOKEN", "NPM_TOKEN"])
    );
    assert_eq!(f(used)["result"], "given");
    assert_eq!(used.actor_exe.as_deref(), Some("claude"));
    let denied = at(&rows, 9_000);
    assert_eq!(f(denied)["result"], "denied");
    assert_eq!(denied.outcome, "refused");
}

#[test]
fn settings_set_value_is_never_imported() {
    let rows = import_legacy_audit(AUDIT, HOST).rows;
    assert!(rows.iter().all(|r| !r.f.contains("SECRET-MARKER")));
    let unknown = at(&rows, 11_000);
    assert_eq!(unknown.kind, "settings.agent");
    assert_eq!(f(unknown)["key"], "unknown");
    assert_eq!(f(unknown)["result"], "set");
    assert!(f(unknown).get("value_chars").is_none());
    let known = at(&rows, 12_000);
    assert_eq!(f(known)["key"], "appearance");
    assert!(!known.f.contains("dark"));
}

#[test]
fn remote_log_maps_each_op() {
    let imported = import_remote_log(REMOTE, HOST);
    assert_eq!(imported.skipped, 1);
    let rows = &imported.rows;
    let message = at(rows, 3_500);
    assert_eq!(
        (message.kind.as_str(), f(message)["op"].clone()),
        ("phone.acted", "message".into())
    );
    assert_eq!(f(message)["chars"], 42);
    assert_eq!(message.actor_tier.as_deref(), Some("phone"));
    assert_eq!(message.device.as_deref(), Some("Pixel 9"));
    assert_eq!(message.session, None);
    assert!(rows.iter().all(|r| !r.f.contains("sess-secret-id")));
    assert!(rows.iter().all(|r| !r.f.contains("192.168")));
    let taken = at(rows, 4_500);
    assert_eq!(taken.kind, "phone.answered");
    assert_eq!(f(taken)["result"], "taken");
    assert_eq!(f(taken)["action"], "allow_once");
    let refused = at(rows, 5_500);
    assert_eq!(refused.outcome, "refused");
    assert_eq!(f(refused)["result"], "acting_off");
    assert_eq!(f(at(rows, 6_500))["action"], "revoked");
    assert_eq!(f(at(rows, 7_500))["action"], "paired");
    assert_eq!(f(at(rows, 8_500))["op"], "send_now");
    assert_eq!(f(at(rows, 9_500))["op"], "new_session");
    assert_eq!(f(at(rows, 10_500))["op"], "other");
    assert_eq!(f(at(rows, 11_500))["op"], "image");
    assert_eq!(f(at(rows, 12_500))["op"], "answer");
}

#[test]
fn account_log_maps_to_switches() {
    let imported = import_account_log(ACCOUNT, HOST);
    assert_eq!(imported.rows.len(), 2);
    let swift = at(&imported.rows, 978_307_210_000);
    assert_eq!(swift.kind, "account.switched");
    assert_eq!(f(swift)["to"], "6f1c2d3e-aaaa-bbbb-cccc-dddddddddddd");
    assert_eq!(f(swift)["provider"], "claude");
    assert_eq!(
        swift.account.as_deref(),
        Some("6f1c2d3e-aaaa-bbbb-cccc-dddddddddddd")
    );
    assert_eq!(f(at(&imported.rows, 7_700))["to"], "short");
}

#[test]
fn unreadable_lines_are_counted_and_skipped() {
    assert_eq!(
        import_legacy_audit("not json\n{\"time_ms\":1}\n\n", HOST).skipped,
        2
    );
    assert_eq!(
        import_remote_log("nope\n{\"time\":\"bad\",\"op\":\"x\"}\n", HOST).skipped,
        2
    );
    assert_eq!(import_account_log("garbage", HOST).skipped, 1);
    assert_eq!(
        import_account_log("[{\"id\":\"a\"},{\"at\":1.0}]", HOST).skipped,
        2
    );
    assert_eq!(import_account_log("", HOST).skipped, 0);
}

#[test]
fn imported_rows_use_only_registered_fields() {
    let mut rows = import_legacy_audit(AUDIT, HOST).rows;
    rows.extend(import_remote_log(REMOTE, HOST).rows);
    rows.extend(import_account_log(ACCOUNT, HOST).rows);
    for row in &rows {
        let spec = registry::find(&row.kind).expect("registered kind");
        assert_eq!(row.grp, spec.group.name());
        let fields = f(row);
        for key in fields.as_object().expect("object").keys() {
            assert!(
                spec.fields.iter().any(|field| field.name == key),
                "{} has no field {key}",
                row.kind
            );
        }
        assert!(row.expires > row.ts);
    }
}

fn envelope(seq: u64) -> Envelope {
    Envelope {
        ts_rx: None,
        seq,
        host: Hex::new(HOST).expect("hex"),
        proc: Proc::Daemon,
        pid: 1,
        boot: Hex::new("b7e1c2d4").expect("hex"),
        op_id: Some(Hex::new("9f2c41d07ab3e512").expect("hex")),
        actor: Some(Actor {
            tier: ActorTier::Agent,
            via: Some(Via::Cli),
            exe: Some(Exe::named("curl")),
            device: None,
            session: None,
        }),
        repeat: None,
        spooled: false,
    }
}

#[test]
fn rebuild_from_months_restores_audited_rows() {
    let key = [9u8; 32];
    let mode = RefMode::Plain(Salt([1; 32]));
    let refused = Event::new(
        &ApiRefused {
            op: Op::named("vault.use"),
            exe: Some(Exe::named("curl")),
            tier: CallerTier::Agent,
            why: RefusedWhy::Forbidden,
            switch: None,
        },
        Outcome::Refused,
        5_000,
    );
    let stopped = Event::new(
        &notchling_events::kinds::security::DaemonStopped {
            cause: StopCause::Signal,
            uptime_ms: Millis(5),
        },
        Outcome::Ok,
        6_000,
    );
    let (first, seal) = sealed_line(&key, 41, None, &envelope(100), &refused, &mode);
    let (second, _) = sealed_line(&key, 42, Some(&seal), &envelope(101), &stopped, &mode);
    let text = format!("{first}\n{second}\nnot a line\n");
    let rebuilt = rebuild_from_months(&[(202611, text)], HOST);
    assert_eq!(rebuilt.skipped, 1);
    assert_eq!(rebuilt.rows.len(), 1);
    let row = &rebuilt.rows[0];
    assert_eq!(row.kind, "api.refused");
    assert_eq!(row.seq, 100);
    assert_eq!(row.audit_n, Some(41));
    assert_eq!(row.ts, 5_000);
    assert!(!row.legacy);
    assert_eq!(row.outcome, "refused");
    assert_eq!(row.actor_tier.as_deref(), Some("agent"));
    assert_eq!(row.actor_exe.as_deref(), Some("curl"));
    assert_eq!(row.op_id.as_deref(), Some("9f2c41d07ab3e512"));
    assert_eq!(f(row)["op"], "vault.use");
    assert_eq!(f(row)["why"], "forbidden");
}
