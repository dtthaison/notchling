use super::poller::{health, LastReport};
use super::status::{parse_status_date, Job, NeedsYouEntry, ReadyEntry, StuckEntry, WorkerStatus};

const FULL: &str =
    include_str!("../../../../legacy/macos/Tests/NotchlingTests/Fixtures/worker-status.json");
const OLD: &str =
    include_str!("../../../../legacy/macos/Tests/NotchlingTests/Fixtures/worker-status-old.json");
const PAUSED: &str = include_str!(
    "../../../../legacy/macos/Tests/NotchlingTests/Fixtures/worker-status-paused.json"
);

const NOW: f64 = 1_800_000_000.0;

fn status(json: &str) -> WorkerStatus {
    WorkerStatus::decode(json.as_bytes()).expect("a status")
}

fn date(text: &str) -> Option<f64> {
    parse_status_date(text)
}

fn ok(s: WorkerStatus) -> LastReport {
    LastReport::ok(s, NOW)
}

fn failed() -> LastReport {
    LastReport::failed("ssh: timed out", NOW)
}

fn busy(current: bool, ready: usize, needs_you: usize) -> WorkerStatus {
    let mut s = WorkerStatus {
        machine: "m1".into(),
        ..WorkerStatus::default()
    };
    if current {
        s.current = Some(Job {
            repo: "o/r".into(),
            issue: 1,
            stage: "implementation".into(),
            ..Job::default()
        });
    }
    s.ready = vec![
        ReadyEntry {
            repo: "o/r".into(),
            issue: 2,
            ..ReadyEntry::default()
        };
        ready
    ];
    s.needs_you = vec![
        NeedsYouEntry {
            repo: "o/r".into(),
            issue: 3,
            ..NeedsYouEntry::default()
        };
        needs_you
    ];
    s
}

#[test]
fn decodes_full_status() {
    let s = WorkerStatus::decode(FULL.as_bytes()).expect("a status");
    assert_eq!(s.machine, "vps-sing-1");
    assert_eq!(s.version, "1.19.1");
    assert_eq!(s.paused, None);
    assert_eq!(
        s.health.get("github").map(String::as_str),
        Some("error: 1 outbox items failed")
    );
    assert_eq!(s.health.get("claude").map(String::as_str), Some("ok"));
    let current = s.current.as_ref().expect("a current job");
    assert_eq!(current.issue, 214);
    assert_eq!(current.stage, "implementation");
    assert_eq!(current.started_at, date("2026-11-06T01:02:03Z"));
    assert_eq!(s.ready.len(), 1);
    assert_eq!(s.ready[0].commit, "9f3c2a1");
    assert_eq!(s.ready[0].evidence_dir, "/home/agent/evidence/201/verify");
    assert_eq!(s.ready[0].ready_at, date("2026-11-06T00:15:09Z"));
    assert_eq!(s.needs_you.len(), 2);
    assert_eq!(
        s.needs_you[0].question.as_deref(),
        Some("Cap at 50 or 100?")
    );
    assert_eq!(s.needs_you[0].recommend.as_deref(), Some("50"));
    assert_eq!(s.needs_you[0].options, vec!["50", "100"]);
    assert_eq!(s.stuck.len(), 1);
    assert_eq!(s.stuck[0].report_tail, "line one\nline two");
    assert_eq!(s.jobs_tonight, 3);
    assert_eq!(s.weekly_pct, Some(42.5));
    assert_eq!(s.token_expires_in_days, Some(14));
    assert_eq!(s.booted_at, date("2026-11-05T22:10:31Z"));
    assert_eq!(s.since, date("2026-11-04T18:00:00Z"));
    assert_eq!(s.unrouted, vec!["ios"]);
    let tf = s.testflight.as_ref().expect("testflight");
    let failed = tf.failed.as_ref().expect("a failure");
    assert_eq!(failed.count, 1);
    assert_eq!(failed.newest_id, "sub1");
    assert!(!tf.quiet);
}

#[test]
fn decodes_triage_needs_you_without_branch_or_worktree() {
    let s = status(FULL);
    let triage = &s.needs_you[1];
    assert_eq!(triage.issue, 230);
    assert_eq!(triage.branch, None);
    assert_eq!(triage.worktree, None);
    assert_eq!(triage.question, None);
    assert_eq!(triage.recommend.as_deref(), Some("split it"));
    assert!(triage.options.is_empty());
}

#[test]
fn decodes_missing_and_extra_fields() {
    let s = status(OLD);
    assert_eq!(s.machine, "m1");
    assert_eq!(s.version, "1.13.0");
    assert_eq!(s.current, None);
    assert_eq!(s.ready.len(), 1);
    assert_eq!(s.ready[0].evidence_dir, "");
    assert_eq!(s.ready[0].ready_at, None);
    assert_eq!(s.needs_you[0].reason, "verify stopped");
    assert_eq!(s.needs_you[0].question, None);
    assert!(s.needs_you[0].options.is_empty());
    assert!(s.stuck.is_empty());
    assert_eq!(s.jobs_tonight, 1);
    assert_eq!(s.weekly_pct, None);
    assert_eq!(s.token_expires_in_days, None);
    assert_eq!(s.booted_at, None);
    assert_eq!(s.since, None);
    assert_eq!(s.outbox_failed, 0);
    assert!(s.unrouted.is_empty());
    assert_eq!(s.testflight, None);
}

#[test]
fn empty_object_is_an_empty_status() {
    assert_eq!(status("{}"), WorkerStatus::default());
}

#[test]
fn wrong_types_become_defaults_and_bad_entries_are_dropped() {
    let s = status(
        r#"{"machine": 5, "jobs_tonight": "many", "paused": null, "ready": "none",
         "needs_you": [{"repo": "o/r", "issue": 7, "options": "x"}, 42, {"repo": "o/r", "issue": 8}],
         "booted_at": "yesterday", "token_expires_in_days": 3.5}"#,
    );
    assert_eq!(s.machine, "");
    assert_eq!(s.jobs_tonight, 0);
    assert!(s.ready.is_empty());
    assert_eq!(
        s.needs_you.iter().map(|e| e.issue).collect::<Vec<_>>(),
        vec![7, 8]
    );
    assert!(s.needs_you[0].options.is_empty());
    assert_eq!(s.booted_at, None);
    assert_eq!(s.token_expires_in_days, None);
}

#[test]
fn one_bad_health_value_drops_only_that_key() {
    let s = status(r#"{"health": {"github": "ok", "claude": 3, "host": "paused: load"}}"#);
    let want: std::collections::BTreeMap<String, String> = [
        ("github".to_string(), "ok".to_string()),
        ("host".to_string(), "paused: load".to_string()),
    ]
    .into();
    assert_eq!(s.health, want);
    assert!(status(r#"{"health": "ok"}"#).health.is_empty());
}

#[test]
fn entry_types_are_top_level() {
    let ready = ReadyEntry {
        repo: "o/r".into(),
        issue: 1,
        ..ReadyEntry::default()
    };
    let needs = NeedsYouEntry {
        repo: "o/r".into(),
        issue: 2,
        ..NeedsYouEntry::default()
    };
    let stuck = StuckEntry {
        repo: "o/r".into(),
        issue: 3,
        ..StuckEntry::default()
    };
    assert_eq!([ready.issue, needs.issue, stuck.issue], [1, 2, 3]);
}

#[test]
fn not_an_object_does_not_decode() {
    assert_eq!(WorkerStatus::decode(b"[1, 2]"), None);
    assert_eq!(WorkerStatus::decode(b"not json"), None);
}

#[test]
fn dates_with_fractional_seconds() {
    let s = status(r#"{"booted_at": "2026-11-05T22:10:31.250Z", "since": "2026-11-05T22:10:31Z"}"#);
    let (booted, since) = (s.booted_at.expect("booted"), s.since.expect("since"));
    assert!((booted - since - 0.25).abs() < 0.001);
}

#[test]
fn outbox_failed_counts_a_list_or_takes_a_number() {
    assert_eq!(
        status(r#"{"outbox_failed": [{"item": {}, "error": "x"}, {"item": {}}]}"#).outbox_failed,
        2
    );
    assert_eq!(status(r#"{"outbox_failed": 3}"#).outbox_failed, 3);
    assert_eq!(status(r#"{"outbox_failed": null}"#).outbox_failed, 0);
}

#[test]
fn test_flight_error_and_quiet() {
    let s = status(
        r#"{"testflight": {"quiet": true, "last_new_at": null, "error": "cannot read state"}}"#,
    );
    let tf = s.testflight.expect("testflight");
    assert!(tf.quiet);
    assert_eq!(tf.last_new_at, None);
    assert_eq!(tf.error.as_deref(), Some("cannot read state"));
    assert_eq!(tf.failed, None);
}

#[test]
fn paused_fixture() {
    let s = status(PAUSED);
    assert_eq!(s.paused.as_deref(), Some("load5 6.4 is above 5.0"));
    assert_eq!(
        s.health.get("host").map(String::as_str),
        Some("paused: load5 6.4 is above 5.0")
    );
}

#[test]
fn hostile_values_stay_plain_data() {
    let s = status(
        r#"{"ready": [{"repo": "x; rm -rf /", "issue": -5, "branch": "--upload-pack=sh",
                       "worktree": "$(touch /tmp/x)", "evidence_dir": "../../etc", "commit": "`id`"}]}"#,
    );
    let e = &s.ready[0];
    assert_eq!(e.issue, -5);
    assert_eq!(e.branch, "--upload-pack=sh");
    assert_eq!(e.worktree, "$(touch /tmp/x)");
    assert_eq!(e.evidence_dir, "../../etc");
}

#[test]
fn health_online() {
    let s = status(FULL);
    assert_eq!(health::word(true, &ok(s.clone()), None, NOW), "online");
    assert_eq!(
        health::word(false, &ok(s.clone()), Some(&ok(s)), NOW),
        "online"
    );
}

#[test]
fn health_offline_no_jobs() {
    let idle = busy(false, 0, 0);
    assert_eq!(
        health::word(true, &failed(), Some(&ok(idle)), NOW),
        "offline"
    );
    assert_eq!(health::word(true, &failed(), None, NOW), "offline");
    assert_eq!(
        health::word(true, &failed(), Some(&failed()), NOW),
        "offline"
    );
}

#[test]
fn health_restarted_when_unreachable_with_jobs() {
    for before in [busy(true, 0, 0), busy(false, 1, 0), busy(false, 0, 1)] {
        assert_eq!(
            health::word(true, &failed(), Some(&ok(before)), NOW),
            "restarted, waiting for unlock"
        );
    }
}

#[test]
fn health_offline_for_non_file_vault() {
    assert_eq!(
        health::word(false, &failed(), Some(&ok(busy(true, 0, 0))), NOW),
        "offline"
    );
}

#[test]
fn health_restarted_when_booted_at_moves_while_working() {
    let mut before = busy(true, 0, 0);
    before.booted_at = Some(1_000.0);
    let mut after = busy(true, 0, 0);
    after.booted_at = Some(2_000.0);
    assert_eq!(
        health::word(true, &ok(after.clone()), Some(&ok(before.clone())), NOW),
        "restarted, waiting for unlock"
    );
    assert_eq!(
        health::word(false, &ok(after.clone()), Some(&ok(before.clone())), NOW),
        "online"
    );
    assert_eq!(
        health::word(true, &ok(before.clone()), Some(&ok(before.clone())), NOW),
        "online"
    );
    after.current = None;
    assert_eq!(
        health::word(true, &ok(after), Some(&ok(before.clone())), NOW),
        "online"
    );
    assert_eq!(
        health::word(true, &ok(busy(true, 0, 0)), Some(&ok(before)), NOW),
        "online"
    );
}

#[test]
fn health_paused() {
    let s = status(PAUSED);
    assert_eq!(
        health::word(true, &ok(s.clone()), None, NOW),
        "production busy, paused"
    );
    assert_eq!(
        health::word(false, &ok(s), None, NOW),
        "production busy, paused"
    );
}

#[test]
fn a_failed_poll_is_not_online_even_with_a_status_attached() {
    let both = LastReport {
        status: Some(busy(true, 0, 0)),
        at: Some(NOW),
        error: Some("bad json".into()),
    };
    assert_eq!(health::word(false, &both, None, NOW), "offline");
}

#[test]
fn token_warning_at_14_not_at_15() {
    let mut s = WorkerStatus {
        machine: "m1".into(),
        ..WorkerStatus::default()
    };
    s.token_expires_in_days = Some(14);
    assert_eq!(
        health::token_warning(&s).as_deref(),
        Some("GitHub token on m1 expires in 14 days")
    );
    s.token_expires_in_days = Some(15);
    assert_eq!(health::token_warning(&s), None);
    s.token_expires_in_days = None;
    assert_eq!(health::token_warning(&s), None);
    s.token_expires_in_days = Some(1);
    assert_eq!(
        health::token_warning(&s).as_deref(),
        Some("GitHub token on m1 expires in 1 day")
    );
    s.token_expires_in_days = Some(-2);
    assert_eq!(
        health::token_warning(&s).as_deref(),
        Some("GitHub token on m1 has expired")
    );
}
