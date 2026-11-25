use super::*;

fn quiet(at: u64) -> Sample {
    Sample {
        at,
        cpu_ms: 5_000,
        out_bytes: 100,
    }
}

fn filled(from: u64, to: u64, step: u64, make: impl Fn(u64) -> Sample) -> Tracker {
    let mut tracker = Tracker::default();
    let mut at = from;
    while at <= to {
        tracker.record("s", make(at));
        at += step;
    }
    tracker
}

#[test]
fn cpu_time_reads_every_ps_format() {
    assert_eq!(parse_cpu_time("0:00.12"), Some(120));
    assert_eq!(parse_cpu_time("12:34.50"), Some(754_500));
    assert_eq!(parse_cpu_time("1:02:03"), Some(3_723_000));
    assert_eq!(parse_cpu_time("1-02:03:04"), Some(93_784_000));
    assert_eq!(parse_cpu_time("0:00.00"), Some(0));
    assert_eq!(parse_cpu_time("junk"), None);
    assert_eq!(parse_cpu_time("1:2:3:4"), None);
    assert_eq!(parse_cpu_time("a:b"), None);
}

#[test]
fn the_table_skips_lines_it_cannot_read() {
    let text = "    1     0   0:01.00\n  200     1  12:00.50\nbad line\n  300   200 1-00:00:01\n";
    let table = parse_cpu_table(text);
    assert_eq!(table.len(), 3);
    assert_eq!(table[2].cpu_ms, 86_401_000);
}

#[test]
fn a_tree_adds_the_roots_and_every_descendant_and_nothing_else() {
    let p = |pid, ppid, cpu_ms| CpuProc { pid, ppid, cpu_ms };
    let table = [
        p(1, 0, 1000),
        p(10, 1, 100),
        p(11, 10, 200),
        p(12, 11, 400),
        p(20, 1, 9000),
    ];
    assert_eq!(tree_cpu_ms(&table, &[10]), 700);
    assert_eq!(tree_cpu_ms(&table, &[10, 20]), 9700);
    assert_eq!(tree_cpu_ms(&table, &[99]), 0);
    assert_eq!(tree_cpu_ms(&table, &[]), 0);
}

#[test]
fn a_shell_with_nothing_changing_for_ten_minutes_has_no_progress() {
    let tracker = filled(0, 1500, 30, quiet);
    let verdict = tracker.verdict("s", 1500, Some(25 * 60)).expect("stuck");
    assert!(verdict.minutes() >= 10);
}

#[test]
fn a_shell_running_twenty_minutes_or_less_is_never_flagged() {
    let tracker = filled(0, 1500, 30, quiet);
    assert_eq!(tracker.verdict("s", 1500, Some(RUNNING_AFTER_S)), None);
    assert_eq!(tracker.verdict("s", 1500, None), None);
}

#[test]
fn growing_output_is_progress() {
    let tracker = filled(0, 1500, 30, |at| Sample {
        out_bytes: 100 + at / 600,
        ..quiet(at)
    });
    assert_eq!(tracker.verdict("s", 1500, Some(3600)), None);
}

#[test]
fn output_that_grew_then_stopped_waits_a_full_window() {
    let tracker = filled(0, 1500, 30, |at| Sample {
        out_bytes: if at < 1000 { 100 + at } else { 5000 },
        ..quiet(at)
    });
    assert_eq!(tracker.verdict("s", 1500, Some(3600)), None);
    let later = filled(0, 1700, 30, |at| Sample {
        out_bytes: if at < 1000 { 100 + at } else { 5000 },
        ..quiet(at)
    });
    assert!(later.verdict("s", 1700, Some(3600)).is_some());
}

#[test]
fn cpu_use_is_progress_and_a_trickle_is_not() {
    let busy = filled(0, 1500, 30, |at| Sample {
        cpu_ms: at * 100,
        ..quiet(at)
    });
    assert_eq!(busy.verdict("s", 1500, Some(3600)), None);
    let trickle = filled(0, 1500, 30, |at| Sample {
        cpu_ms: at,
        ..quiet(at)
    });
    assert!(trickle.verdict("s", 1500, Some(3600)).is_some());
}

#[test]
fn a_child_that_ended_makes_the_cpu_total_drop_and_counts_as_activity() {
    let tracker = filled(0, 1500, 30, |at| Sample {
        cpu_ms: if at < 900 { 9000 } else { 5000 },
        ..quiet(at)
    });
    assert_eq!(tracker.verdict("s", 1500, Some(3600)), None);
}

fn sparse(first: u64, last: u64) -> Tracker {
    let mut tracker = Tracker::default();
    let mut at = first;
    while at < last {
        tracker.record("s", quiet(at));
        at += 60;
    }
    tracker.record("s", quiet(last));
    tracker
}

#[test]
fn nine_minutes_of_history_never_claims_ten() {
    for last in [610, 640, 690, 699] {
        let tracker = sparse(100, last);
        assert_eq!(
            tracker.verdict("s", last, Some(3600)),
            None,
            "{} s observed",
            last - 100
        );
    }
    let tracker = sparse(100, 700);
    let verdict = tracker.verdict("s", 700, Some(3600)).expect("ten minutes");
    assert_eq!(verdict.quiet_s, 600);
}

#[test]
fn the_cpu_budget_follows_the_span_actually_observed() {
    let idle = filled(0, 660, 60, |at| Sample {
        cpu_ms: at * 47 / 10,
        ..quiet(at)
    });
    let verdict = idle.verdict("s", 660, Some(3600)).expect("282 ms a minute");
    assert_eq!(verdict.quiet_s, 660);
    let busy = filled(0, 660, 60, |at| Sample {
        cpu_ms: at * 6,
        ..quiet(at)
    });
    assert_eq!(busy.verdict("s", 660, Some(3600)), None);
}

#[test]
fn too_little_history_proves_nothing() {
    let tracker = filled(900, 1000, 30, quiet);
    assert_eq!(tracker.verdict("s", 1000, Some(3600)), None);
}

#[test]
fn a_gap_in_sampling_forgets_what_came_before() {
    let mut tracker = filled(0, 600, 30, quiet);
    tracker.record("s", quiet(900));
    assert_eq!(tracker.verdict("s", 900, Some(3600)), None);
    for at in (930..=1400).step_by(30) {
        tracker.record("s", quiet(at));
    }
    assert_eq!(tracker.verdict("s", 1400, Some(3600)), None);
    for at in (1430..=1520).step_by(30) {
        tracker.record("s", quiet(at));
    }
    assert!(tracker.verdict("s", 1520, Some(3600)).is_some());
}

#[test]
fn old_samples_are_not_judged_after_sampling_stops() {
    let tracker = filled(0, 1500, 30, quiet);
    assert!(tracker.verdict("s", 1500 + MAX_GAP_S, Some(3600)).is_some());
    assert_eq!(tracker.verdict("s", 1500 + MAX_GAP_S + 1, Some(3600)), None);
}

#[test]
fn history_stays_short() {
    let tracker = filled(0, 100_000, 30, quiet);
    assert!(tracker.history["s"].len() < 60);
    assert!(tracker.verdict("s", 100_000, Some(200_000)).is_some());
}

#[test]
fn keep_waiting_hides_the_shell_for_thirty_minutes_only() {
    let mut tracker = Tracker::default();
    assert!(!tracker.snoozed("s", 100));
    tracker.snooze("s", 100);
    assert!(tracker.snoozed("s", 100 + SNOOZE_S - 1));
    assert!(!tracker.snoozed("s", 100 + SNOOZE_S));
    assert!(!tracker.snoozed("other", 100));
}

#[test]
fn a_shell_that_is_gone_is_forgotten() {
    let mut tracker = filled(0, 600, 30, quiet);
    tracker.snooze("s", 0);
    tracker.forget_except(&HashSet::from(["other".to_owned()]));
    assert!(tracker.history.is_empty());
    assert!(!tracker.snoozed("s", 1));
}
