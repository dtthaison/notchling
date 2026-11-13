use super::*;
use crate::sessions::testutil::TempDir;

const APP: &str = "/Applications/Notchling.app/Contents/MacOS/notchling-app";

fn audit(
    time_ms: u64,
    op: &str,
    tier: &str,
    peer: Option<&str>,
    result: &str,
    reason: Option<&str>,
) -> String {
    let mut v = json!({"time_ms": time_ms, "op": op, "tier": tier, "peer": peer, "result": result});
    if let Some(reason) = reason {
        v["reason"] = json!(reason);
    }
    v.to_string()
}

fn fixture(dir: &TempDir) -> Files {
    let old = [
        audit(1_000, "session.reply", "app", Some(APP), "ok", None),
        audit(
            2_000,
            "vault.pending",
            "agent",
            Some("/usr/bin/curl"),
            "forbidden",
            None,
        ),
    ]
    .join("\n");
    let now = [
        audit(
            3_000,
            "session.move",
            "app",
            Some(APP),
            "failed",
            Some("the target\naccount   is not signed in"),
        ),
        audit(
            4_000,
            "settings.set_switch accounts",
            "app",
            Some(APP),
            "on",
            None,
        ),
        audit(5_000, "inbox.answer", "app", Some(APP), "ok", None),
        audit(6_000, "accounts.switch", "app", Some(APP), "ok", None),
        audit(7_000, "connect", "agent", None, "peer_unknown", None),
        audit(
            8_000,
            "vault.use",
            "agent",
            Some("/opt/homebrew/bin/claude"),
            "ok:GITHUB_TOKEN,NPM_TOKEN",
            None,
        ),
        audit(
            9_000,
            "vault.use",
            "agent",
            Some("/opt/homebrew/bin/claude"),
            "denied:GITHUB_TOKEN",
            None,
        ),
        "not json".to_owned(),
        audit(
            10_000,
            "session.reply",
            "app",
            Some(APP),
            "ok",
            Some("ignored on a success"),
        ),
    ]
    .join("\n");
    dir.write("audit.jsonl.1", &old);
    dir.write("audit.jsonl", &format!("{now}\n"));
    let phone = [
        r#"{"time":"1970-01-01T00:00:03.500Z","device":"Pixel 9","op":"message","session":"sess-secret-id","length":42,"address":"192.168.1.9"}"#,
        r#"{"time":"1970-01-01T00:00:04.500Z","device":"Pixel 9","op":"inbox.answer:allow_once"}"#,
        r#"{"time":"1970-01-01T00:00:05.500Z","device":"Pixel 9","op":"inbox.answer:refused:acting_off"}"#,
        r#"{"time":"1970-01-01T00:00:06.500Z","device":"Pixel 9","op":"revoke"}"#,
    ]
    .join("\n");
    dir.write("bridge/remote-log.jsonl", &phone);
    dir.write(
        "account-log.json",
        r#"[{"at": 10.0, "id": "6f1c2d3e-aaaa-bbbb-cccc-dddddddddddd"}, {"at": "1970-01-01T00:00:07.700Z", "id": "short"}]"#,
    );
    Files {
        audit: dir.path("audit.jsonl"),
        remote_log: dir.path("bridge/remote-log.jsonl"),
        account_log: dir.path("account-log.json"),
    }
}

fn query() -> Query {
    Query {
        kinds: Vec::new(),
        since_ms: 0,
        limit: DEFAULT_LIMIT,
    }
}

fn find<'a>(rows: &'a [Row], op: &str, time_ms: u64) -> &'a Row {
    rows.iter()
        .find(|r| r.op == op && r.time_ms == time_ms)
        .unwrap_or_else(|| panic!("no {op} at {time_ms} in {rows:?}"))
}

#[test]
fn every_kind_is_classified_from_the_audit_the_phone_log_and_the_account_log() {
    let dir = TempDir::new("activity-kinds");
    let rows = rows(&fixture(&dir), &query());
    assert_eq!(find(&rows, "session.reply", 1_000).kind, "acted");
    let refused = find(&rows, "vault.pending", 2_000);
    assert_eq!((refused.kind, refused.who.as_str()), ("refused", "curl"));
    let failed = find(&rows, "session.move", 3_000);
    assert_eq!((failed.kind, failed.who.as_str()), ("failed", "app"));
    assert_eq!(failed.result, "failed");
    assert_eq!(
        failed.detail.as_deref(),
        Some("the target account is not signed in")
    );
    assert_eq!(
        find(&rows, "settings.set_switch accounts", 4_000).kind,
        "setting"
    );
    assert_eq!(find(&rows, "inbox.answer", 5_000).kind, "inbox");
    assert_eq!(find(&rows, "accounts.switch", 6_000).kind, "account_switch");
    let connect = find(&rows, "connect", 7_000);
    assert_eq!((connect.kind, connect.who.as_str()), ("refused", "unknown"));
    let used = find(&rows, "vault.use", 8_000);
    assert_eq!(
        (used.kind, used.result.as_str(), used.who.as_str()),
        ("acted", "ok", "claude")
    );
    let denied = find(&rows, "vault.use", 9_000);
    assert_eq!((denied.kind, denied.result.as_str()), ("refused", "denied"));
    let reply = find(&rows, "session.reply", 10_000);
    assert_eq!(reply.detail, None);

    let sent = find(&rows, "message", 3_500);
    assert_eq!(
        (sent.source, sent.kind, sent.who.as_str()),
        ("phone", "acted", "Pixel 9")
    );
    let answered = find(&rows, "inbox.answer", 4_500);
    assert_eq!(
        (answered.source, answered.kind, answered.result.as_str()),
        ("phone", "inbox", "allow_once")
    );
    let refused = find(&rows, "inbox.answer", 5_500);
    assert_eq!(
        (refused.kind, refused.result.as_str()),
        ("refused", "refused: acting_off")
    );
    assert_eq!(find(&rows, "revoke", 6_500).kind, "acted");

    let swift = find(&rows, "account.active", 978_307_210_000);
    assert_eq!((swift.source, swift.kind), ("account", "account_switch"));
    assert_eq!(swift.detail.as_deref(), Some("6f1c2d3e"));
    assert_eq!(
        find(&rows, "account.active", 7_700).detail.as_deref(),
        Some("short")
    );
}

#[test]
fn rows_are_newest_first_and_the_old_audit_copy_is_read_too() {
    let dir = TempDir::new("activity-order");
    let rows = rows(&fixture(&dir), &query());
    let times: Vec<u64> = rows.iter().map(|r| r.time_ms).collect();
    let mut sorted = times.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(times, sorted);
    assert!(times.contains(&1_000));
    assert_eq!(times.len(), 10 + 4 + 2);
}

#[test]
fn kinds_since_and_limit_narrow_the_rows() {
    let dir = TempDir::new("activity-filter");
    let files = fixture(&dir);
    let q = Query::from_args(&json!({"kinds": ["refused"], "since": 2_000})).unwrap();
    let rows = rows(&files, &q);
    assert!(rows
        .iter()
        .all(|r| r.kind == "refused" && r.time_ms > 2_000));
    assert_eq!(rows.len(), 3);
    let q = Query::from_args(&json!({"limit": 2})).unwrap();
    assert_eq!(super::rows(&files, &q).len(), 2);
    let q = Query::from_args(&json!({"limit": 0})).unwrap();
    assert_eq!(q.limit, 1);
    let q = Query::from_args(&json!({"limit": 9_999_999})).unwrap();
    assert_eq!(q.limit, MAX_LIMIT);
    assert_eq!(Query::from_args(&json!({})).unwrap(), query());
}

#[test]
fn a_bad_query_is_refused() {
    assert!(Query::from_args(&json!({"kinds": ["nope"]})).is_err());
    assert!(Query::from_args(&json!({"kinds": "refused"})).is_err());
    assert!(Query::from_args(&json!({"since": "yesterday"})).is_err());
    assert!(Query::from_args(&json!({"limit": -1})).is_err());
}

#[test]
fn nothing_secret_leaves_in_a_row() {
    let dir = TempDir::new("activity-redact");
    let out = list(&fixture(&dir), &query()).to_string();
    for secret in [
        "GITHUB_TOKEN",
        "NPM_TOKEN",
        "sess-secret-id",
        "192.168.1.9",
        "6f1c2d3e-aaaa",
        "ignored on a success",
    ] {
        assert!(!out.contains(secret), "{secret} in {out}");
    }
    let rows: Vec<Row> = rows(&fixture(&dir), &query());
    for row in &rows {
        let keys: Vec<String> = row.to_json().as_object().unwrap().keys().cloned().collect();
        for key in keys {
            assert!(
                ["time_ms", "source", "kind", "op", "who", "result", "detail"]
                    .contains(&key.as_str()),
                "{key}"
            );
        }
    }
}

#[test]
fn a_missing_file_gives_no_rows_and_a_cut_first_line_is_dropped() {
    let dir = TempDir::new("activity-missing");
    let files = Files {
        audit: dir.path("none.jsonl"),
        remote_log: dir.path("none-phone.jsonl"),
        account_log: dir.path("none.json"),
    };
    assert!(rows(&files, &query()).is_empty());
    let line = audit(1, "session.reply", "app", Some(APP), "ok", None);
    let text = format!("{line}\n{line}\n");
    let path = dir.write("cut.jsonl", &text);
    let cut = tail(&path, line.len() as u64 + 5);
    assert_eq!(cut.lines().count(), 1);
    assert_eq!(tail(&path, 1_000_000).lines().count(), 2);
}

#[test]
fn the_payload_stays_under_the_cap_however_many_rows_there_are() {
    let dir = TempDir::new("activity-cap");
    let wide = "\u{1f600}".repeat(130);
    let mut lines = Vec::new();
    for n in 0..1_200u64 {
        lines.push(audit(
            1_000 + n,
            &wide,
            "agent",
            Some(&format!("/p/{wide}")),
            "failed",
            Some(&wide),
        ));
    }
    dir.write("audit.jsonl", &lines.join("\n"));
    let files = Files {
        audit: dir.path("audit.jsonl"),
        remote_log: dir.path("a"),
        account_log: dir.path("b"),
    };
    let q = Query {
        kinds: Vec::new(),
        since_ms: 0,
        limit: MAX_LIMIT,
    };
    let out = list(&files, &q);
    let size = serde_json::to_vec(&out).unwrap().len();
    assert_eq!(out["truncated"], true);
    assert!(size <= MAX_BYTES + 64, "{size}");
    assert!(size < 1024 * 1024);
    let shown = out["rows"].as_array().unwrap();
    assert!(!shown.is_empty() && shown.len() < MAX_LIMIT);
    assert_eq!(shown[0]["time_ms"], 1_000 + 1_199);
    assert!(shown[0]["detail"].as_str().unwrap().chars().count() <= 121);
    assert!(shown[0]["op"].as_str().unwrap().chars().count() <= 81);
}
