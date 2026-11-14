use super::jobs::{job_id, JobState};

#[test]
fn job_id_is_stable_and_short() {
    assert_eq!(job_id("one"), job_id("one"));
    assert_eq!(job_id("one").len(), 14);
}

#[test]
fn transitions_table() {
    use JobState::*;
    let all = [Dispatching, Running, Done, Failed, Interrupted, Lost];
    let allowed = [
        (Dispatching, Running),
        (Dispatching, Failed),
        (Running, Done),
        (Running, Failed),
        (Running, Interrupted),
        (Running, Lost),
    ];
    for from in all {
        for to in all {
            assert_eq!(
                from.may_become(to),
                allowed.contains(&(from, to)),
                "{from:?} {to:?}"
            );
        }
    }
}
