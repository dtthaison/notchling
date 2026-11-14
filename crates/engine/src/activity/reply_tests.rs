use super::{reply, six_kind, ActivityRow, Query, MAX_BYTES};
use serde_json::{json, Value};

fn row(seq: u64, ts: i64, kind: &str, grp: &str, outcome: &str, f: Value) -> ActivityRow {
    let mut row = ActivityRow::blank(seq, ts, kind);
    row.grp = grp.to_owned();
    row.outcome = outcome.to_owned();
    row.host = "3f9a0c1b7d2e4a56".to_owned();
    row.f = f.to_string();
    row
}

fn no_names(_: &str, _: &str) -> Option<String> {
    None
}

#[test]
fn reply_shape_is_unchanged() {
    let mut used = row(
        1,
        1_000,
        "vault.secret_used",
        "secrets",
        "ok",
        json!({"secrets": ["GITHUB_TOKEN", "NPM_TOKEN"], "result": "given"}),
    );
    used.actor_tier = Some("agent".into());
    used.actor_exe = Some("claude".into());
    used.audit_n = Some(12);
    let out = reply(&[used], &Query::plain(200), &no_names);
    assert_eq!(out["truncated"], false);
    let first = &out["rows"][0];
    for key in ["time_ms", "source", "kind", "op", "who", "result"] {
        assert!(first.get(key).is_some(), "{key}");
    }
    assert_eq!(first["time_ms"], 1_000);
    assert_eq!(first["source"], "audit");
    assert_eq!(first["kind"], "acted");
    assert_eq!(first["op"], "vault.secret_used");
    assert_eq!(first["who"], "claude");
    assert_eq!(first["result"], "given");
    assert_eq!(first["detail"], "GITHUB_TOKEN, NPM_TOKEN");
    assert_eq!(first["event"], "vault.secret_used");
    assert_eq!(first["group"], "secrets");
    assert_eq!(first["title"], "Secrets Given to Commands");
    assert_eq!(first["outcome"], "ok");
    assert_eq!(first["seq"], 1);
    assert_eq!(first["audit_n"], 12);
    assert_eq!(first["spooled"], false);
}

#[test]
fn six_kind_mapping() {
    let kind = |r: &ActivityRow| six_kind(r, &r.fields());
    let refused = row(1, 1, "api.refused", "security", "refused", json!({}));
    let failed = row(2, 2, "op.failed", "diagnostics", "failed", json!({}));
    let setting = row(3, 3, "settings.agent", "settings", "ok", json!({}));
    let switch = row(4, 4, "switch.changed", "security", "ok", json!({}));
    let inbox = row(5, 5, "inbox.closed", "inbox", "ok", json!({}));
    let account = row(6, 6, "account.switched", "accounts", "ok", json!({}));
    let acted = row(7, 7, "session.reply", "agents", "ok", json!({}));
    let legacy_inbox = row(
        8,
        8,
        "op.called",
        "diagnostics",
        "ok",
        json!({"op": "inbox.answer"}),
    );
    let legacy_switch = row(
        9,
        9,
        "op.called",
        "diagnostics",
        "ok",
        json!({"op": "accounts.switch"}),
    );
    assert_eq!(kind(&refused), "refused");
    assert_eq!(kind(&failed), "failed");
    assert_eq!(kind(&setting), "setting");
    assert_eq!(kind(&switch), "setting");
    assert_eq!(kind(&inbox), "inbox");
    assert_eq!(kind(&account), "account_switch");
    assert_eq!(kind(&acted), "acted");
    assert_eq!(kind(&legacy_inbox), "inbox");
    assert_eq!(kind(&legacy_switch), "account_switch");
}

#[test]
fn who_resolves_device_names() {
    let mut phone = row(1, 1, "phone.acted", "phone", "ok", json!({"op": "message"}));
    phone.actor_tier = Some("phone".into());
    phone.device = Some("d41c".into());
    let names =
        |kind: &str, id: &str| (kind == "device" && id == "d41c").then(|| "Pixel 9".to_owned());
    assert_eq!(
        reply(&[phone.clone()], &Query::plain(10), &names)["rows"][0]["who"],
        "Pixel 9"
    );
    assert_eq!(
        reply(&[phone], &Query::plain(10), &no_names)["rows"][0]["who"],
        "d41c"
    );
    let app = {
        let mut r = row(2, 2, "op.called", "diagnostics", "ok", json!({}));
        r.actor_tier = Some("app".into());
        r
    };
    let daemon = row(3, 3, "daemon.started", "security", "ok", json!({}));
    let mut agent = row(4, 4, "api.refused", "security", "refused", json!({}));
    agent.actor_tier = Some("agent".into());
    let out = reply(&[app, daemon, agent], &Query::plain(10), &no_names);
    let who: Vec<&str> = out["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|r| r["who"].as_str().expect("who"))
        .collect();
    assert_eq!(who, vec!["unknown", "daemon", "app"]);
}

#[test]
fn rows_filter_by_six_kind_and_sort_newest_first() {
    let rows = vec![
        row(1, 1_000, "api.refused", "security", "refused", json!({})),
        row(2, 3_000, "session.reply", "agents", "ok", json!({})),
        row(3, 2_000, "api.refused", "security", "refused", json!({})),
    ];
    let all = reply(&rows, &Query::plain(10), &no_names);
    let times: Vec<i64> = all["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|r| r["time_ms"].as_i64().expect("time"))
        .collect();
    assert_eq!(times, vec![3_000, 2_000, 1_000]);
    let mut q = Query::plain(10);
    q.kinds = vec!["refused".into()];
    assert_eq!(
        reply(&rows, &q, &no_names)["rows"]
            .as_array()
            .expect("rows")
            .len(),
        2
    );
    assert_eq!(
        reply(&rows, &Query::plain(1), &no_names)["rows"]
            .as_array()
            .expect("rows")
            .len(),
        1
    );
}

#[test]
fn query_args_are_checked() {
    let q = Query::from_args(&json!({
        "groups": ["secrets"],
        "event_kinds": ["api.refused"],
        "session": "s1",
        "before_seq": 7,
        "kinds": ["refused"],
        "since": 5,
        "limit": 9,
    }))
    .expect("parses");
    assert_eq!(q.groups, vec!["secrets"]);
    assert_eq!(q.event_kinds, vec!["api.refused"]);
    assert_eq!(q.session.as_deref(), Some("s1"));
    assert_eq!(q.before_seq, Some(7));
    assert!(Query::from_args(&json!({"groups": ["nope"]})).is_err());
    assert!(Query::from_args(&json!({"event_kinds": ["nope.kind"]})).is_err());
    assert!(Query::from_args(&json!({"session": 4})).is_err());
    assert!(Query::from_args(&json!({"before_seq": "x"})).is_err());
}

#[test]
fn reply_is_capped_at_900k() {
    let wide = "\u{1f600}".repeat(130);
    let rows: Vec<ActivityRow> = (0..1_200u64)
        .map(|n| {
            let mut r = row(
                n + 1,
                1_000 + n as i64,
                "vault.secret_used",
                "secrets",
                "ok",
                json!({"secrets": vec![wide.clone(); 16], "result": "given"}),
            );
            r.actor_tier = Some("agent".into());
            r.actor_exe = Some(wide.clone());
            r
        })
        .collect();
    let out = reply(&rows, &Query::plain(1_000), &no_names);
    let size = serde_json::to_vec(&out).expect("json").len();
    assert_eq!(out["truncated"], true);
    assert!(size <= MAX_BYTES + 64, "{size}");
    let shown = out["rows"].as_array().expect("rows");
    assert!(!shown.is_empty() && shown.len() < 1_000);
    assert_eq!(shown[0]["time_ms"], 1_000 + 1_199);
    assert!(shown[0]["detail"].as_str().expect("detail").chars().count() <= 201);
}
