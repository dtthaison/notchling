use super::*;
use crate::cluster_testkit::Dir;

const JOB: &str = "j-0123456789ab";

fn keys(list: &[&str]) -> Vec<String> {
    list.iter().map(|k| k.to_string()).collect()
}

#[test]
fn a_mark_keeps_the_keys_of_its_job_and_gains_later_ones() {
    let dir = Dir::new("marks");
    let marks = GrantMarks::new(dir.path());
    marks.mark_with(JOB, &keys(&["c1"])).expect("marked");
    marks.mark_with(JOB, &keys(&["c1", "s9"])).expect("marked");
    marks.mark_with(JOB, &[]).expect("marked");
    let found = marks.marked();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].job, JOB);
    assert_eq!(found[0].keys, keys(&["c1", "s9"]));
    assert_eq!(found[0].ended_ms, None);
    assert_eq!(marks.list(), [JOB.to_string()]);
}

#[test]
fn a_mark_from_the_first_version_reads_with_no_keys() {
    let dir = Dir::new("marks");
    let marks = GrantMarks::new(dir.path());
    marks.mark(JOB).expect("marked");
    std::fs::write(dir.path().join(MARKS_DIR).join(JOB), b"granted\n").unwrap();
    let found = marks.marked();
    assert_eq!(found.len(), 1, "the job is still listed");
    assert!(found[0].keys.is_empty());
    marks.mark_with(JOB, &keys(&["c1"])).expect("marked");
    assert_eq!(marks.marked()[0].keys, keys(&["c1"]));
}

#[test]
fn the_first_end_time_is_the_one_kept() {
    let dir = Dir::new("marks");
    let marks = GrantMarks::new(dir.path());
    marks.mark_with(JOB, &keys(&["c1"])).expect("marked");
    marks.stamp_end(JOB, 1_000).expect("stamped");
    marks.stamp_end(JOB, 9_000).expect("stamped");
    assert_eq!(marks.marked()[0].ended_ms, Some(1_000));
    marks.mark_with(JOB, &keys(&["c2"])).expect("marked");
    let found = marks.marked();
    assert_eq!(found[0].ended_ms, Some(1_000), "a mark keeps the end");
    assert_eq!(found[0].keys, keys(&["c1", "c2"]));
}

#[test]
fn a_mark_holds_names_and_never_a_value() {
    let dir = Dir::new("marks");
    let marks = GrantMarks::new(dir.path());
    marks.mark_with(JOB, &keys(&["c1"])).expect("marked");
    let text = std::fs::read_to_string(dir.path().join(MARKS_DIR).join(JOB)).unwrap();
    assert!(text.contains("c1"));
    assert!(text.len() < 200);
}

#[test]
fn a_mark_with_a_hostile_key_is_refused() {
    let dir = Dir::new("marks");
    let marks = GrantMarks::new(dir.path());
    let long = "x".repeat(300);
    assert!(marks.mark_with(JOB, &[long]).is_err());
    assert!(marks.mark_with(JOB, &keys(&["a\nb"])).is_err());
    assert!(marks.marked().is_empty());
}

#[test]
fn removing_a_mark_forgets_it() {
    let dir = Dir::new("marks");
    let marks = GrantMarks::new(dir.path());
    marks.mark_with(JOB, &keys(&["c1"])).expect("marked");
    marks.remove(JOB);
    assert!(marks.marked().is_empty());
    assert!(marks.stamp_end(JOB, 5).is_err(), "nothing to stamp");
}
