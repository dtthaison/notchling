use super::*;
use crate::sessions::testutil::TempDir;
use serde_json::{json, Value};

const AUDIT: &str = include_str!("../../tests/fixtures/activity/audit.jsonl");
const REMOTE: &str = include_str!("../../tests/fixtures/activity/remote-log.jsonl");
const ACCOUNT: &str = include_str!("../../tests/fixtures/activity/account-log.json");

fn no_names(_: &str, _: &str) -> Option<String> {
    None
}

fn load(dir: &TempDir) -> EventsDb {
    let mut lines = AUDIT.lines();
    let old: Vec<&str> = lines.by_ref().take(2).collect();
    let now: Vec<&str> = lines.collect();
    dir.write("audit.jsonl.1", &format!("{}\n", old.join("\n")));
    dir.write("audit.jsonl", &format!("{}\n", now.join("\n")));
    dir.write("bridge/remote-log.jsonl", REMOTE);
    dir.write("account-log.json", ACCOUNT);
    let mut db = EventsDb::open(&dir.path("daemon/events.db")).expect("opens");
    let files = ImportFiles {
        audit: vec![dir.path("audit.jsonl.1"), dir.path("audit.jsonl")],
        remote_log: dir.path("bridge/remote-log.jsonl"),
        account_log: dir.path("account-log.json"),
    };
    import_once(&mut db, &files, "3f9a0c1b7d2e4a56").expect("imports");
    db
}

fn query() -> Query {
    Query::plain(DEFAULT_LIMIT)
}

fn rows_of(db: &EventsDb, q: &Query) -> Vec<Value> {
    let out = list(db, q, &no_names);
    out["rows"].as_array().expect("rows").clone()
}

fn find<'a>(rows: &'a [Value], op: &str, time_ms: u64) -> &'a Value {
    rows.iter()
        .find(|r| r["op"] == op && r["time_ms"] == time_ms)
        .unwrap_or_else(|| panic!("no {op} at {time_ms} in {rows:?}"))
}

#[test]
fn every_kind_is_classified_from_the_audit_the_phone_log_and_the_account_log() {
    let dir = TempDir::new("activity-kinds");
    let rows = rows_of(&load(&dir), &query());
    assert_eq!(find(&rows, "session.reply", 1_000)["kind"], "acted");
    let refused = find(&rows, "vault.pending", 2_000);
    assert_eq!(
        (refused["kind"].as_str(), refused["who"].as_str()),
        (Some("refused"), Some("curl"))
    );
    let failed = find(&rows, "session.move", 3_000);
    assert_eq!(
        (failed["kind"].as_str(), failed["who"].as_str()),
        (Some("failed"), Some("app"))
    );
    assert_eq!(failed["result"], "failed");
    assert!(
        failed.get("detail").is_none(),
        "an outside message is never kept"
    );
    assert_eq!(find(&rows, "switch.changed", 4_000)["kind"], "setting");
    assert_eq!(find(&rows, "switch.changed", 4_000)["result"], "on");
    assert_eq!(find(&rows, "inbox.answer", 5_000)["kind"], "inbox");
    assert_eq!(
        find(&rows, "accounts.switch", 6_000)["kind"],
        "account_switch"
    );
    let connect = find(&rows, "api.unknown_peer", 7_000);
    assert_eq!(
        (connect["kind"].as_str(), connect["who"].as_str()),
        (Some("refused"), Some("unknown"))
    );
    let used = find(&rows, "vault.secret_used", 8_000);
    assert_eq!(
        (
            used["kind"].as_str(),
            used["result"].as_str(),
            used["who"].as_str()
        ),
        (Some("acted"), Some("given"), Some("claude"))
    );
    let denied = find(&rows, "vault.secret_used", 9_000);
    assert_eq!(
        (denied["kind"].as_str(), denied["result"].as_str()),
        (Some("refused"), Some("denied"))
    );
    assert!(find(&rows, "session.reply", 10_000).get("detail").is_none());

    let sent = find(&rows, "message", 3_500);
    assert_eq!(
        (
            sent["source"].as_str(),
            sent["kind"].as_str(),
            sent["who"].as_str()
        ),
        (Some("legacy"), Some("acted"), Some("Pixel 9"))
    );
    let answered = find(&rows, "phone.answered", 4_500);
    assert_eq!(
        (
            answered["source"].as_str(),
            answered["kind"].as_str(),
            answered["result"].as_str()
        ),
        (Some("legacy"), Some("inbox"), Some("allow_once"))
    );
    let refused = find(&rows, "phone.answered", 5_500);
    assert_eq!(
        (refused["kind"].as_str(), refused["result"].as_str()),
        (Some("refused"), Some("acting_off"))
    );
    assert_eq!(find(&rows, "phone.paired", 6_500)["kind"], "acted");

    let swift = find(&rows, "account.switched", 978_307_210_000);
    assert_eq!(
        (swift["source"].as_str(), swift["kind"].as_str()),
        (Some("legacy"), Some("account_switch"))
    );
    assert_eq!(
        swift["refs"]["account"],
        "6f1c2d3e-aaaa-bbbb-cccc-dddddddddddd"
    );
    assert_eq!(
        find(&rows, "account.switched", 7_700)["refs"]["account"],
        "short"
    );
}

#[test]
fn rows_are_newest_first_and_the_old_audit_copy_is_read_too() {
    let dir = TempDir::new("activity-order");
    let rows = rows_of(&load(&dir), &query());
    let times: Vec<u64> = rows
        .iter()
        .map(|r| r["time_ms"].as_u64().expect("time"))
        .collect();
    let mut sorted = times.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(times, sorted);
    assert!(times.contains(&1_000));
    assert_eq!(times.len(), 14 + 10 + 2);
}

#[test]
fn kinds_since_and_limit_narrow_the_rows() {
    let dir = TempDir::new("activity-filter");
    let db = load(&dir);
    let q = Query::from_args(&json!({"kinds": ["refused"], "since": 2_000})).expect("query");
    let rows = rows_of(&db, &q);
    assert!(rows
        .iter()
        .all(|r| r["kind"] == "refused" && r["time_ms"].as_u64() > Some(2_000)));
    assert_eq!(rows.len(), 4);
    let q = Query::from_args(&json!({"limit": 2})).expect("query");
    assert_eq!(rows_of(&db, &q).len(), 2);
    let q = Query::from_args(&json!({"limit": 0})).expect("query");
    assert_eq!(q.limit, 1);
    let q = Query::from_args(&json!({"limit": 9_999_999})).expect("query");
    assert_eq!(q.limit, MAX_LIMIT);
    assert_eq!(Query::from_args(&json!({})).expect("query"), query());
}

#[test]
fn groups_event_kinds_and_paging_narrow_the_rows() {
    let dir = TempDir::new("activity-new-args");
    let db = load(&dir);
    let q = Query::from_args(&json!({"groups": ["phone"]})).expect("query");
    let rows = rows_of(&db, &q);
    assert_eq!(rows.len(), 10);
    assert!(rows.iter().all(|r| r["group"] == "phone"));
    let q = Query::from_args(&json!({"event_kinds": ["vault.secret_used"]})).expect("query");
    assert_eq!(rows_of(&db, &q).len(), 2);
    let all = rows_of(&db, &query());
    let seq = all[5]["seq"].as_u64().expect("seq");
    let q = Query::from_args(&json!({"before_seq": seq})).expect("query");
    assert!(rows_of(&db, &q)
        .iter()
        .all(|r| r["seq"].as_u64() < Some(seq)));
}

#[test]
fn a_bad_query_is_refused() {
    assert!(Query::from_args(&json!({"kinds": ["nope"]})).is_err());
    assert!(Query::from_args(&json!({"kinds": "refused"})).is_err());
    assert!(Query::from_args(&json!({"since": "yesterday"})).is_err());
    assert!(Query::from_args(&json!({"limit": -1})).is_err());
}

#[test]
fn importing_twice_adds_nothing() {
    let dir = TempDir::new("activity-twice");
    let mut db = load(&dir);
    let before = db.count();
    let files = ImportFiles {
        audit: vec![dir.path("audit.jsonl")],
        remote_log: dir.path("bridge/remote-log.jsonl"),
        account_log: dir.path("account-log.json"),
    };
    let report = import_once(&mut db, &files, "3f9a0c1b7d2e4a56").expect("imports");
    assert_eq!(report, ImportReport::default());
    assert_eq!(db.count(), before);
}

#[test]
fn nothing_secret_leaves_in_a_row() {
    let dir = TempDir::new("activity-redact");
    let db = load(&dir);
    let out = list(&db, &query(), &no_names).to_string();
    for secret in [
        "sess-secret-id",
        "192.168.1.9",
        "ignored on a success",
        "SECRET-MARKER",
        "not signed in",
    ] {
        assert!(!out.contains(secret), "{secret} in {out}");
    }
    assert!(
        out.contains("GITHUB_TOKEN"),
        "secret names show (decision 1)"
    );
    let allowed = [
        "time_ms", "source", "kind", "op", "who", "result", "detail", "event", "group", "title",
        "outcome", "refs", "op_id", "seq", "audit_n", "spooled",
    ];
    for row in rows_of(&db, &query()) {
        for key in row.as_object().expect("object").keys() {
            assert!(allowed.contains(&key.as_str()), "{key}");
        }
    }
}

#[test]
fn a_missing_file_gives_no_rows() {
    let dir = TempDir::new("activity-missing");
    let mut db = EventsDb::open(&dir.path("daemon/events.db")).expect("opens");
    let files = ImportFiles {
        audit: vec![dir.path("none.jsonl")],
        remote_log: dir.path("none-phone.jsonl"),
        account_log: dir.path("none.json"),
    };
    let report = import_once(&mut db, &files, "3f9a0c1b7d2e4a56").expect("imports");
    assert_eq!(report, ImportReport::default());
    assert!(rows_of(&db, &query()).is_empty());
}

#[test]
fn the_payload_stays_under_the_cap_however_many_rows_there_are() {
    let dir = TempDir::new("activity-cap");
    let wide = "\u{1f600}".repeat(130);
    let mut db = EventsDb::open(&dir.path("daemon/events.db")).expect("opens");
    let rows: Vec<ActivityRow> = (0..1_200u64)
        .map(|n| {
            let mut row = ActivityRow::blank(n + 1, 1_000 + n as i64, "vault.secret_used");
            row.grp = "secrets".into();
            row.actor_tier = Some("agent".into());
            row.actor_exe = Some(wide.clone());
            row.f = json!({"secrets": vec![wide.clone(); 16], "result": "given"}).to_string();
            row
        })
        .collect();
    db.insert(&rows).expect("inserts");
    let q = Query::plain(MAX_LIMIT);
    let out = list(&db, &q, &no_names);
    let size = serde_json::to_vec(&out).expect("json").len();
    assert_eq!(out["truncated"], true);
    assert!(size <= MAX_BYTES + 64, "{size}");
    assert!(size < 1024 * 1024);
    let shown = out["rows"].as_array().expect("rows");
    assert!(!shown.is_empty() && shown.len() < MAX_LIMIT);
    assert_eq!(shown[0]["time_ms"], 1_000 + 1_199);
    assert!(shown[0]["detail"].as_str().expect("detail").chars().count() <= 201);
    assert!(shown[0]["who"].as_str().expect("who").chars().count() <= 61);
}
