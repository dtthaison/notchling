use crate::log_audit::AuditSink;
use crate::log_audit_tests::{called, rig, Rig, T0};
use crate::log_bus::Sink;
use crate::log_retention::{deletable, months_to_prune, KeepAudit, PruneError, DAY_MS};
use notchling_engine::logs::{month_of, KeyStore};
use notchling_events::audit_check::{check, CheckInput, CheckResult};
use notchling_events::kinds::choices::PruneReason;
use notchling_events::Seal;
use serde_json::Value;

const MB: u64 = 1024 * 1024;

#[test]
fn pure_picks_months_older_than_a_year() {
    let months = [202501, 202512, 202606, 202611];
    let sizes = [MB; 4];
    let now = T0 - 60 * DAY_MS;
    assert_eq!(month_of(now), 202611);
    let picked = months_to_prune(&months, &sizes, now, KeepAudit::Year1, 500 * MB);
    assert_eq!(picked, vec![(202501, PruneReason::Retention)]);
    let shorter = months_to_prune(&months, &sizes, now, KeepAudit::Days90, 500 * MB);
    assert_eq!(
        shorter,
        vec![
            (202501, PruneReason::Retention),
            (202512, PruneReason::Retention),
            (202606, PruneReason::Retention)
        ]
    );
}

#[test]
fn pure_cap_drops_oldest_first() {
    let months = [202608, 202609, 202610, 202611];
    let sizes = [200 * MB, 200 * MB, 200 * MB, MB];
    let now = T0 - 60 * DAY_MS;
    let picked = months_to_prune(&months, &sizes, now, KeepAudit::Year1, 500 * MB);
    assert_eq!(picked, vec![(202608, PruneReason::SizeCap)]);
    let more = months_to_prune(&months, &sizes, now, KeepAudit::Year1, 300 * MB);
    assert_eq!(
        more,
        vec![
            (202608, PruneReason::SizeCap),
            (202609, PruneReason::SizeCap)
        ]
    );
}

#[test]
fn pure_never_picks_current_month() {
    let months = [202611];
    let now = T0 - 60 * DAY_MS;
    assert!(months_to_prune(&months, &[900 * MB], now, KeepAudit::Days90, MB).is_empty());
}

const LATER: i64 = T0 + 40 * DAY_MS;

fn two_months(rig: &Rig, sink: &mut AuditSink) -> (u32, u32) {
    sink.write(
        &[rig.stamp(called(T0)), rig.stamp(called(LATER))],
        &[None, None],
    );
    (month_of(T0), month_of(LATER))
}

fn check_with_anchor(rig: &Rig, with_anchor: bool) -> CheckResult {
    let dir = rig.dir();
    let texts: Vec<(u32, String)> = dir
        .months()
        .unwrap()
        .into_iter()
        .map(|m| (m, dir.read(m).unwrap()))
        .collect();
    let months: Vec<(u32, Option<&str>)> =
        texts.iter().map(|(m, t)| (*m, Some(t.as_str()))).collect();
    let key = rig.keys.key().unwrap();
    let anchor = KeyStore::read_head(&*rig.keys)
        .and_then(|c| c.prune_anchor)
        .and_then(|a| Seal::parse(&a));
    check(&CheckInput {
        key: Some(&key),
        months: &months,
        anchor: if with_anchor { anchor.as_ref() } else { None },
        checkpoint: None,
        from_n: None,
    })
    .result
}

#[test]
fn prune_writes_anchor_then_deletes() {
    let rig = rig("retention-anchor");
    let mut sink = rig.sink();
    let (old, _) = two_months(&rig, &mut sink);
    rig.clock.store(LATER, std::sync::atomic::Ordering::SeqCst);
    let n = sink.prune(old, PruneReason::Retention).unwrap();
    assert!(!rig.dir().months().unwrap().contains(&old));
    let lines = rig.lines();
    let pruned = lines.iter().find(|l| l["kind"] == "audit.pruned").unwrap();
    assert_eq!(pruned["n"].as_u64(), Some(n));
    assert_eq!(pruned["f"]["through_month"], old);
    assert_eq!(pruned["f"]["reason"], "retention");
    assert_eq!(check_with_anchor(&rig, true), CheckResult::Intact);
    assert_eq!(check_with_anchor(&rig, false), CheckResult::Unanchored);
    let head = KeyStore::read_head(&*rig.keys).unwrap();
    assert_eq!(head.n, n);
    assert!(head.prune_anchor.is_some());
}

#[test]
fn user_delete_is_carried_forward() {
    let rig = rig("retention-user");
    let mut sink = rig.sink();
    let months = [T0 - 200 * DAY_MS, T0 - 170 * DAY_MS, T0 - 140 * DAY_MS];
    for ts in months {
        rig.clock.store(ts, std::sync::atomic::Ordering::SeqCst);
        sink.write(&[rig.stamp(called(ts))], &[None]);
    }
    rig.clock.store(T0, std::sync::atomic::Ordering::SeqCst);
    sink.write(&[rig.stamp(called(T0))], &[None]);
    let first = month_of(months[0]);
    let second = month_of(months[1]);
    let third = month_of(months[2]);
    sink.prune(first, PruneReason::User).unwrap();
    sink.prune(second, PruneReason::User).unwrap();
    sink.prune(third, PruneReason::Retention).unwrap();
    let lines = rig.lines();
    let last: &Value = lines.iter().rfind(|l| l["kind"] == "audit.pruned").unwrap();
    assert_eq!(last["f"]["user_deleted_count"], 2);
    assert_eq!(
        last["f"]["user_deleted_months"],
        serde_json::json!([first, second])
    );
    let head = KeyStore::read_head(&*rig.keys).unwrap();
    assert_eq!(head.user_deleted.count, 2);
    assert_eq!(head.user_deleted.months, vec![first, second]);
}

#[test]
fn delete_refuses_recent_month() {
    let rig = rig("retention-recent");
    let mut sink = rig.sink();
    let recent = T0 - 10 * DAY_MS;
    let current = T0;
    sink.write(
        &[rig.stamp(called(recent)), rig.stamp(called(current))],
        &[None, None],
    );
    let result = sink.delete_through(month_of(recent));
    if month_of(recent) == month_of(current) {
        assert!(matches!(result, Err(PruneError::Current)));
    } else {
        assert!(matches!(result, Err(PruneError::Current)), "{result:?}");
    }
    assert!(!deletable(month_of(recent), current));
}

#[test]
fn delete_refuses_current_month() {
    let rig = rig("retention-current");
    let mut sink = rig.sink();
    sink.write(&[rig.stamp(called(T0))], &[None]);
    let result = sink.delete_through(month_of(T0));
    assert!(matches!(result, Err(PruneError::Current)));
    assert_eq!(rig.dir().months().unwrap(), vec![month_of(T0)]);
}

#[test]
fn delete_through_removes_old_months_and_reports_them() {
    let rig = rig("retention-delete");
    let mut sink = rig.sink();
    let old_a = T0 - 200 * DAY_MS;
    let old_b = T0 - 120 * DAY_MS;
    for ts in [old_a, old_b, T0] {
        rig.clock.store(ts, std::sync::atomic::Ordering::SeqCst);
        sink.write(&[rig.stamp(called(ts))], &[None]);
    }
    rig.clock.store(T0, std::sync::atomic::Ordering::SeqCst);
    let (removed, last) = sink.delete_through(month_of(old_b)).unwrap();
    assert_eq!(removed, 2);
    assert!(last.is_some());
    assert_eq!(rig.dir().months().unwrap(), vec![month_of(T0)]);
    assert_eq!(check_with_anchor(&rig, true), CheckResult::Intact);
}

#[test]
fn retention_runs_in_the_first_tick() {
    let rig = rig("retention-tick");
    let mut sink = rig.sink();
    let old = T0 - 500 * DAY_MS;
    rig.clock.store(old, std::sync::atomic::Ordering::SeqCst);
    sink.write(&[rig.stamp(called(old))], &[None]);
    rig.clock.store(T0, std::sync::atomic::Ordering::SeqCst);
    sink.write(&[rig.stamp(called(T0))], &[None]);
    assert_eq!(rig.dir().months().unwrap().len(), 2);
    let ticked = sink.tick(T0);
    assert_eq!(rig.dir().months().unwrap(), vec![month_of(T0)]);
    assert!(ticked
        .extra
        .iter()
        .any(|e| e.stamped.event.spec.name == "audit.pruned"));
}
