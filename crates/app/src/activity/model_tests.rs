use serde_json::{json, Value};

use super::model::{time_label, ActivityModel, Entry, Kind, Load};
use super::renders::{model, Shown, NOW_MS, UTC};
use super::wire;

fn reply(rows: Value) -> Value {
    json!({"rows": rows, "truncated": false})
}

fn row(time_ms: i64, kind: &str, op: &str, result: &str) -> Value {
    json!({"time_ms": time_ms, "source": "audit", "kind": kind, "op": op, "who": "app", "result": result})
}

#[test]
fn the_wire_names_round_trip_and_only_refusals_and_failures_stand_out() {
    for kind in Kind::ALL {
        assert_eq!(Kind::from_wire(kind.wire()), Some(kind));
    }
    assert_eq!(Kind::from_wire("nope"), None);
    let loud: Vec<Kind> = Kind::ALL.into_iter().filter(|k| k.stands_out()).collect();
    assert_eq!(loud, [Kind::Refused, Kind::Failed]);
    for kind in Kind::ALL {
        assert!(
            kind.label()
                .chars()
                .next()
                .is_some_and(|c| c.is_uppercase()),
            "{kind:?}"
        );
    }
}

#[test]
fn the_request_asks_for_the_newest_rows_with_a_limit() {
    let request = wire::request();
    assert_eq!(request.op, "activity.list");
    assert_eq!(request.args, json!({"limit": wire::LIMIT}));
}

#[test]
fn a_reply_becomes_entries_newest_first_and_a_bad_row_is_skipped() {
    let mut model = ActivityModel::default();
    assert!(model.start());
    assert!(!model.start());
    model.finish(Ok(json!({
        "rows": [
            row(1_000, "acted", "session.reply", "ok"),
            {"time_ms": 3_000, "kind": "weird", "op": "x", "who": "app", "source": "audit", "result": "ok"},
            row(2_000, "failed", "session.move", "failed"),
        ],
        "truncated": true
    })));
    assert_eq!(model.load, Load::Done);
    assert!(model.truncated);
    let times: Vec<i64> = model.entries.iter().map(|e| e.time_ms).collect();
    assert_eq!(times, [2_000, 1_000]);
    assert!(model.start());
}

#[test]
fn a_failed_load_keeps_the_rows_and_says_why() {
    let mut model = ActivityModel::default();
    model.finish(Ok(reply(json!([row(1, "acted", "a", "ok")]))));
    model.start();
    model.finish(Err("the daemon isn't running".into()));
    assert_eq!(model.entries.len(), 1);
    assert_eq!(model.load, Load::Failed("the daemon isn't running".into()));
    assert_eq!(
        model.empty_text().as_deref(),
        Some("the daemon isn't running")
    );
}

#[test]
fn chips_filter_by_kind_and_none_on_means_all() {
    let mut m = model(Shown::All);
    let all = m.visible().len();
    assert_eq!(all, m.entries.len());
    m.toggle(Kind::Refused);
    assert!(m.is_on(Kind::Refused));
    let refused = m.visible();
    assert!(!refused.is_empty() && refused.iter().all(|e| e.kind == Kind::Refused));
    assert_eq!(refused.len(), m.count(Kind::Refused));
    m.toggle(Kind::Failed);
    assert_eq!(
        m.visible().len(),
        m.count(Kind::Refused) + m.count(Kind::Failed)
    );
    m.toggle(Kind::Refused);
    m.toggle(Kind::Failed);
    assert_eq!(m.visible().len(), all);
}

#[test]
fn search_reads_the_operation_the_result_who_and_the_detail_without_case() {
    let mut m = model(Shown::All);
    m.search = "SESSION.MOVE".into();
    assert_eq!(m.visible().len(), 1);
    m.search = "  forbidden ".into();
    assert!(m.visible().iter().all(|e| e.result == "forbidden"));
    assert_eq!(m.visible().len(), 2);
    m.search = "not signed in".into();
    assert_eq!(m.visible()[0].op, "session.move");
    m.search = "curl".into();
    assert_eq!(m.visible()[0].op, "vault.pending");
    m.search = "zzz".into();
    assert!(m.visible().is_empty());
    assert_eq!(m.empty_text().as_deref(), Some("Nothing matches"));
}

#[test]
fn the_filtered_render_shows_the_refused_and_failed_session_rows() {
    let m = model(Shown::Filtered);
    let ops: Vec<&str> = m.visible().iter().map(|e| e.op.as_str()).collect();
    assert_eq!(ops, ["session.move", "session.stop"]);
}

#[test]
fn an_empty_model_says_what_it_is_waiting_for() {
    let mut m = ActivityModel::default();
    assert_eq!(m.empty_text().as_deref(), Some("No activity yet"));
    m.start();
    assert_eq!(m.empty_text().as_deref(), Some("Loading\u{2026}"));
    m.finish(Ok(reply(json!([]))));
    assert_eq!(m.empty_text().as_deref(), Some("No activity yet"));
    m.finish(Ok(reply(json!([row(1, "acted", "a", "ok")]))));
    assert_eq!(m.empty_text(), None);
}

#[test]
fn a_line_names_who_and_the_result_and_a_phone_row_says_phone() {
    let app = Entry::parse(&row(1, "failed", "session.move", "failed")).unwrap();
    assert_eq!(app.line(), "app \u{b7} failed");
    let phone = Entry::parse(&json!({
        "time_ms": 1, "source": "phone", "kind": "refused", "op": "inbox.answer",
        "who": "Pixel 9", "result": "refused: acting_off", "detail": "x"
    }))
    .unwrap();
    assert_eq!(
        phone.line(),
        "Pixel 9 (phone) \u{b7} refused: acting_off \u{b7} x"
    );
}

#[test]
fn times_read_as_a_clock_today_and_a_date_before() {
    let ago = |s: i64| NOW_MS - s * 1000;
    let now = time_label(NOW_MS, NOW_MS, UTC);
    assert!(now.ends_with("AM") || now.ends_with("PM"), "{now}");
    assert!(!now.contains(','));
    let old = time_label(NOW_MS, ago(3 * 86_400), UTC);
    assert!(old.contains(", "), "{old}");
    let years = time_label(NOW_MS, ago(400 * 86_400), UTC);
    assert!(years.matches(", ").count() == 2, "{years}");
    assert_eq!(time_label(NOW_MS, 0, UTC), "");
    assert_ne!(
        time_label(NOW_MS, NOW_MS, 3 * 3600),
        time_label(NOW_MS, NOW_MS, 0)
    );
}
