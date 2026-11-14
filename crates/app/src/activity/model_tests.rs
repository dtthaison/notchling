use serde_json::{json, Value};

use super::model::{time_label, ActivityModel, Check, Entry, Kind, LineCheck, Load};
use super::renders::{model, Shown, NOW_MS, OPEN_SEQ, UTC};
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

fn sealed_row() -> Value {
    json!({
        "time_ms": 5_000, "source": "audit", "kind": "acted", "op": "session.reply",
        "who": "app", "result": "ok", "event": "session.reply", "group": "agents",
        "title": "Replies Sent", "outcome": "ok", "refs": {}, "seq": 7, "audit_n": 41,
        "spooled": false
    })
}

fn line(sealed: bool, seal_ok: bool) -> Value {
    json!({"line": {"n": 41, "sealed": sealed, "seal_ok": seal_ok, "kind": "session.reply",
        "outcome": "ok", "ts": 5_000, "f": {}}})
}

#[test]
fn parses_new_fields_when_present() {
    let e = Entry::parse(&sealed_row()).unwrap();
    assert_eq!(e.event.as_deref(), Some("session.reply"));
    assert_eq!(e.title.as_deref(), Some("Replies Sent"));
    assert_eq!(e.outcome.as_deref(), Some("ok"));
    assert_eq!((e.seq, e.audit_n, e.spooled), (Some(7), Some(41), false));
}

#[test]
fn old_reply_still_parses() {
    let e = Entry::parse(&row(1, "acted", "session.reply", "ok")).unwrap();
    assert_eq!((&e.event, &e.title, &e.outcome), (&None, &None, &None));
    assert_eq!((e.seq, e.audit_n, e.spooled), (None, None, false));
    assert_eq!(e.heading(), "session.reply");
}

#[test]
fn title_wins_over_op_label() {
    let e = Entry::parse(&sealed_row()).unwrap();
    assert_eq!(e.heading(), "Replies Sent");
    let mut m = ActivityModel::loaded(vec![e]);
    m.search = "replies sent".into();
    assert_eq!(m.visible().len(), 1);
}

#[test]
fn spooled_row_says_so() {
    let mut value = sealed_row();
    value["spooled"] = json!(true);
    let e = Entry::parse(&value).unwrap();
    assert!(e.spooled);
    assert_eq!(
        notchling_locale::settings::activity_spooled(),
        "Recorded by the app while the daemon was down"
    );
}

#[test]
fn audit_check_compares_fields() {
    let e = Entry::parse(&sealed_row()).unwrap();
    assert_eq!(e.compare(&line(true, true)), Some(LineCheck::Matches));
    assert_eq!(LineCheck::Matches.text(), "Matches the sealed entry");
    assert_eq!(e.compare(&line(true, false)), Some(LineCheck::Differs));
    assert_eq!(e.compare(&line(false, false)), Some(LineCheck::Unsealed));
    let mut other = line(true, true);
    other["line"]["outcome"] = json!("failed");
    assert_eq!(e.compare(&other), Some(LineCheck::Differs));
    assert_eq!(LineCheck::Differs.text(), "Doesn't match the sealed entry");
    let mut later = line(true, true);
    later["line"]["ts"] = json!(6_000);
    assert_eq!(e.compare(&later), Some(LineCheck::Differs));
    assert_eq!(e.compare(&json!({})), None);
}

#[test]
fn a_check_runs_once_and_lands_on_its_row() {
    let mut m = ActivityModel::loaded(vec![Entry::parse(&sealed_row()).unwrap()]);
    assert!(m.begin_check(7));
    assert!(!m.begin_check(7));
    m.finish_check(7, Ok(line(true, true)));
    assert_eq!(m.checks[&7], Check::Done(LineCheck::Matches));
    assert!(m.begin_check(7));
    m.finish_check(7, Err("the daemon isn't running".into()));
    assert_eq!(
        m.checks[&7],
        Check::Failed("the daemon isn't running".into())
    );
    m.toggle_open(7);
    assert_eq!(m.open, Some(7));
    m.toggle_open(7);
    assert_eq!(m.open, None);
}

#[test]
fn the_open_render_has_a_sealed_row_open_with_its_check_done() {
    let m = model(Shown::Open);
    assert_eq!(m.open, Some(OPEN_SEQ));
    assert_eq!(m.checks[&OPEN_SEQ], Check::Done(LineCheck::Matches));
    assert!(m
        .entries
        .iter()
        .any(|e| e.seq == Some(OPEN_SEQ) && e.audit_n.is_some()));
    assert!(m.entries.iter().any(|e| e.spooled));
    assert!(m.entries.iter().any(|e| e.title.is_some()));
}

#[test]
fn the_verify_request_asks_for_one_line() {
    let r = wire::verify_request(41);
    assert_eq!(r.op, "log.verify");
    assert_eq!(r.args, json!({"n": 41}));
}
