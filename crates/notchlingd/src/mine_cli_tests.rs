use crate::mine_cli::{activity_args, lines, parse, Read};
use serde_json::json;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn delivered() -> serde_json::Value {
    json!({
        "rows": [],
        "others": {"messages": 3, "sessions": 2},
        "events": [{"time_ms": 1234, "event": "session.outbox", "result": "ok"}],
    })
}

#[test]
fn receipts_in_plain_text_show_the_delivered_events() {
    let out = lines(Read::Receipts, &delivered());
    assert_eq!(out, vec!["1234  session.outbox  ok".to_owned()]);
}

#[test]
fn receipts_never_report_other_sessions() {
    let mut reply = delivered();
    reply["rows"] = json!([{"id": "m1", "state": "queued", "why": "turn"}]);
    let out = lines(Read::Receipts, &reply);
    assert_eq!(out[0], "m1  queued  turn");
    assert!(out.iter().all(|l| !l.contains("Other sessions")), "{out:?}");
}

#[test]
fn receipts_with_nothing_say_so() {
    let reply = json!({"rows": [], "others": {"messages": 0, "sessions": 0}, "events": []});
    assert_eq!(lines(Read::Receipts, &reply), vec!["Nothing.".to_owned()]);
}

#[test]
fn outbox_still_reports_other_sessions() {
    let reply = json!({
        "rows": [{"id": "m1", "state": "queued", "text": "hello\nworld"}],
        "others": {"messages": 3, "sessions": 2},
    });
    assert_eq!(
        lines(Read::Outbox, &reply),
        vec![
            "m1  queued  hello".to_owned(),
            "Other sessions: 3 messages in 2 sessions.".to_owned()
        ]
    );
}

#[test]
fn a_session_less_receipts_read_says_it_has_none() {
    let reply = json!({
        "rows": [],
        "others": {"messages": 0, "sessions": 0},
        "reason": "no_session",
        "events": [],
    });
    let out = lines(Read::Receipts, &reply);
    assert_eq!(out.len(), 1);
    assert!(out[0].contains("none here"), "{out:?}");
}

#[test]
fn since_travels_to_activity_mine() {
    let parsed = parse(Read::Activity, &args(&["--since", "5001", "--limit", "7"])).unwrap();
    let sent = activity_args(&parsed, &[]);
    assert_eq!(sent["since"], json!(5001));
    assert_eq!(sent["limit"], json!(7));
    assert!(parse(Read::Outbox, &args(&["--since", "1"])).is_err());
}
