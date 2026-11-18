use super::jobs::{Cancelled, Job, Kind, Outcome, Patience, Queue, Stops};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const ID: &str = "457369a9-35a1-40f8-92b9-26b6398b78a1";
const SHORT: Duration = Duration::from_millis(30);

fn reply(id: &str, text: &str) -> Job {
    Job {
        id: id.into(),
        kind: Kind::Reply { session: ID.into() },
        text: text.into(),
    }
}

#[test]
fn a_job_is_taken_once_and_reported_once() {
    let q = Queue::new();
    q.submit(reply("a", "hi")).unwrap();
    assert_eq!(q.waiting(), 1);
    assert_eq!(q.take(SHORT), vec![reply("a", "hi")]);
    assert!(q.take(SHORT).is_empty());
    assert!(q.report("a", Outcome::Delivered { session: None }));
    assert!(!q.report("a", Outcome::Failed("late".into())));
    assert_eq!(
        q.wait("a", SHORT),
        Some(Outcome::Delivered { session: None })
    );
}

#[test]
fn submitting_the_same_id_twice_keeps_one_job() {
    let q = Queue::new();
    q.submit(reply("a", "hi")).unwrap();
    q.submit(reply("a", "hi")).unwrap();
    assert_eq!(q.take(SHORT).len(), 1);
    q.submit(reply("a", "hi")).unwrap();
    assert!(q.take(SHORT).is_empty());
}

#[test]
fn a_job_nobody_took_waits_and_can_be_withdrawn_but_a_taken_one_cannot() {
    let q = Queue::new();
    q.submit(reply("a", "hi")).unwrap();
    assert_eq!(q.wait("a", SHORT), None);
    assert_eq!(q.cancel("a"), Cancelled::Withdrawn);
    assert!(q.take(SHORT).is_empty());
    q.submit(reply("b", "hi")).unwrap();
    q.take(SHORT);
    assert_eq!(q.cancel("b"), Cancelled::Taken);
    assert_eq!(q.cancel("zzz"), Cancelled::Gone);
}

#[test]
fn a_waiting_taker_wakes_when_a_job_arrives_and_a_waiter_when_it_is_reported() {
    let q = Arc::new(Queue::new());
    let taker = {
        let q = q.clone();
        std::thread::spawn(move || q.take(Duration::from_secs(5)))
    };
    std::thread::sleep(Duration::from_millis(40));
    q.submit(reply("a", "hi")).unwrap();
    assert_eq!(taker.join().unwrap().len(), 1);
    let waiter = {
        let q = q.clone();
        std::thread::spawn(move || q.wait("a", Duration::from_secs(5)))
    };
    std::thread::sleep(Duration::from_millis(40));
    q.report("a", Outcome::Unconfirmed { session: None });
    assert_eq!(
        waiter.join().unwrap(),
        Some(Outcome::Unconfirmed { session: None })
    );
}

#[test]
fn closing_wakes_everyone_and_refuses_new_jobs() {
    let q = Arc::new(Queue::new());
    let taker = {
        let q = q.clone();
        std::thread::spawn(move || q.take(Duration::from_secs(5)))
    };
    std::thread::sleep(Duration::from_millis(30));
    q.close();
    assert!(taker.join().unwrap().is_empty());
    assert!(q.submit(reply("a", "hi")).is_err());
}

#[test]
fn the_sidecar_counts_as_running_only_while_it_keeps_asking() {
    let q = Queue::new();
    assert!(!q.alive(1_000, 15_000));
    q.seen(1_000);
    assert!(q.alive(10_000, 15_000));
    assert!(!q.alive(20_000, 15_000));
}

#[test]
fn too_many_waiting_messages_are_refused_visibly() {
    let q = Queue::new();
    for n in 0..64 {
        q.submit(reply(&format!("j{n}"), "x")).unwrap();
    }
    assert!(q.submit(reply("over", "x")).is_err());
}

#[test]
fn jobs_and_outcomes_round_trip_through_json() {
    let jobs = [
        reply("a", "hi"),
        Job {
            id: "b".into(),
            kind: Kind::Start {
                model: Some("flash".into()),
                title: None,
            },
            text: "go".into(),
        },
    ];
    for job in jobs {
        assert_eq!(Job::from_json(&job.to_json()), Some(job));
    }
    assert_eq!(
        Job::from_json(&json!({"id": "a", "kind": "other", "text": "x"})),
        None
    );
    for outcome in [
        Outcome::Delivered {
            session: Some(ID.into()),
        },
        Outcome::Unconfirmed { session: None },
        Outcome::Failed("no".into()),
    ] {
        assert_eq!(Outcome::from_json(&outcome.to_json()), Some(outcome));
    }
    assert_eq!(Outcome::from_json(&json!({"result": "maybe"})), None);
}

#[test]
fn a_stop_is_used_once_and_forgotten_when_a_new_message_goes_in() {
    let stops = Stops::new();
    stops.ask(ID);
    assert!(stops.take(ID));
    assert!(!stops.take(ID));
    stops.ask(ID);
    stops.forget(ID);
    assert!(!stops.take(ID));
}

#[test]
fn a_retry_of_a_failed_job_runs_again_but_a_delivered_one_never_twice() {
    let q = Queue::new();
    q.submit(reply("a", "hi")).unwrap();
    q.take(SHORT);
    q.report("a", Outcome::Failed("no".into()));
    q.submit(reply("a", "hi")).unwrap();
    assert_eq!(q.take(SHORT).len(), 1);
    q.report("a", Outcome::Delivered { session: None });
    q.submit(reply("a", "hi")).unwrap();
    assert!(q.take(SHORT).is_empty());
    assert_eq!(
        q.wait("a", SHORT),
        Some(Outcome::Delivered { session: None })
    );
}

#[test]
fn one_take_can_be_limited_to_one_job_and_the_rest_wait() {
    let q = Queue::new();
    for id in ["a", "b", "c"] {
        q.submit(reply(id, "x")).unwrap();
    }
    assert_eq!(q.take_for(SHORT, 1, &|| true), vec![reply("a", "x")]);
    assert_eq!(q.waiting(), 2);
    assert_eq!(q.take_for(SHORT, 1, &|| true), vec![reply("b", "x")]);
}

#[test]
fn a_taker_whose_process_is_gone_takes_nothing_and_the_job_stays_for_the_next() {
    let q = Queue::new();
    q.submit(reply("a", "x")).unwrap();
    assert!(q.take_for(SHORT, 5, &|| false).is_empty());
    assert_eq!(q.waiting(), 1);
    assert_eq!(q.take(SHORT).len(), 1);
}

#[test]
fn a_taker_waiting_when_its_process_dies_lets_go_and_the_next_taker_gets_the_job() {
    let q = Arc::new(Queue::new());
    let up = Arc::new(AtomicBool::new(true));
    let old = {
        let (q, up) = (q.clone(), up.clone());
        std::thread::spawn(move || {
            q.take_for(Duration::from_secs(10), 5, &|| up.load(Ordering::SeqCst))
        })
    };
    std::thread::sleep(Duration::from_millis(60));
    up.store(false, Ordering::SeqCst);
    assert!(old.join().unwrap().is_empty());
    q.submit(reply("a", "x")).unwrap();
    assert_eq!(q.take(SHORT).len(), 1);
}

#[test]
fn a_newer_take_replaces_an_older_one_that_is_still_waiting() {
    let q = Arc::new(Queue::new());
    let old = {
        let q = q.clone();
        std::thread::spawn(move || q.take(Duration::from_secs(10)))
    };
    std::thread::sleep(Duration::from_millis(60));
    let new = {
        let q = q.clone();
        std::thread::spawn(move || q.take(Duration::from_secs(10)))
    };
    assert!(old.join().unwrap().is_empty());
    q.submit(reply("a", "x")).unwrap();
    assert_eq!(new.join().unwrap(), vec![reply("a", "x")]);
}

#[test]
fn the_sidecar_is_running_while_it_waits_or_works_and_for_a_moment_after() {
    let q = Arc::new(Queue::new());
    assert!(!q.alive(0, 1_000));
    let waiter = {
        let q = q.clone();
        std::thread::spawn(move || q.take(Duration::from_millis(400)))
    };
    std::thread::sleep(Duration::from_millis(60));
    assert!(q.alive(10_000_000, 1_000));
    waiter.join().unwrap();
    assert!(!q.alive(10_000_000, 1_000));
    q.submit(reply("a", "x")).unwrap();
    q.take(SHORT);
    assert!(q.alive(10_000_000, 1_000));
    q.report("a", Outcome::Delivered { session: None });
    q.seen_by(5_000, 77);
    assert_eq!(q.last_pid(), Some(77));
    assert!(q.alive(5_500, 1_000));
    assert!(!q.alive(7_000, 1_000));
}

#[test]
fn a_taken_job_that_never_reports_is_dropped_after_its_bound() {
    let q = Queue::keeping_taken(Duration::from_millis(40));
    q.submit(reply("a", "x")).unwrap();
    q.take(SHORT);
    assert_eq!(q.cancel("a"), Cancelled::Taken);
    std::thread::sleep(Duration::from_millis(80));
    q.submit(reply("b", "x")).unwrap();
    assert_eq!(q.cancel("a"), Cancelled::Gone);
    assert!(!q.report("a", Outcome::Delivered { session: None }));
    q.submit(reply("a", "x")).unwrap();
    assert_eq!(q.waiting(), 2);
}

#[test]
fn a_job_waits_for_a_taker_by_one_limit_and_for_a_report_by_another() {
    let q = Arc::new(Queue::new());
    q.submit(reply("a", "x")).unwrap();
    let patience = Patience {
        pickup: Duration::from_millis(60),
        run: Duration::from_secs(5),
    };
    assert_eq!(q.wait_patient("a", patience, &|| true), None);
    assert_eq!(q.cancel("a"), Cancelled::Withdrawn);
    q.submit(reply("b", "x")).unwrap();
    q.take(SHORT);
    let waiter = {
        let q = q.clone();
        std::thread::spawn(move || {
            q.wait_patient(
                "b",
                Patience {
                    pickup: Duration::from_millis(1),
                    run: Duration::from_secs(5),
                },
                &|| true,
            )
        })
    };
    std::thread::sleep(Duration::from_millis(120));
    q.report("b", Outcome::Delivered { session: None });
    assert_eq!(
        waiter.join().unwrap(),
        Some(Outcome::Delivered { session: None })
    );
    q.submit(reply("c", "x")).unwrap();
    q.take(SHORT);
    let short_run = Patience {
        pickup: Duration::from_secs(5),
        run: Duration::from_millis(60),
    };
    assert_eq!(q.wait_patient("c", short_run, &|| true), None);
    assert_eq!(q.cancel("c"), Cancelled::Taken);
}

#[test]
fn a_waiting_job_is_given_up_at_once_when_the_sidecar_is_no_longer_there() {
    let q = Queue::new();
    q.submit(reply("a", "x")).unwrap();
    let patience = Patience {
        pickup: Duration::from_secs(30),
        run: Duration::from_secs(30),
    };
    let began = std::time::Instant::now();
    assert_eq!(q.wait_patient("a", patience, &|| false), None);
    assert!(began.elapsed() < Duration::from_secs(2));
    assert_eq!(q.cancel("a"), Cancelled::Withdrawn);
}

#[test]
fn a_take_that_a_newer_one_replaced_never_writes_its_process_over_the_newer_ones() {
    let q = Arc::new(Queue::new());
    let old = {
        let q = q.clone();
        std::thread::spawn(move || {
            q.take_noting(Duration::from_secs(10), 5, &|| true, Some((&|| 1_000, 111)))
        })
    };
    std::thread::sleep(Duration::from_millis(60));
    assert_eq!(q.last_pid(), Some(111));
    let new = {
        let q = q.clone();
        std::thread::spawn(move || {
            q.take_noting(Duration::from_secs(10), 5, &|| true, Some((&|| 2_000, 222)))
        })
    };
    assert!(old.join().unwrap().is_empty());
    assert_eq!(q.last_pid(), Some(222));
    q.submit(reply("a", "x")).unwrap();
    assert_eq!(new.join().unwrap().len(), 1);
    assert_eq!(q.last_pid(), Some(222));
}

#[test]
fn a_take_whose_process_is_gone_does_not_note_itself_as_seen() {
    let q = Queue::new();
    let got = q.take_noting(SHORT, 5, &|| false, Some((&|| 5_000, 9)));
    assert!(got.is_empty());
    assert_eq!(q.last_pid(), None);
    let none = Queue::new();
    none.take_noting(SHORT, 5, &|| true, None);
    assert_eq!(none.last_pid(), None);
}
