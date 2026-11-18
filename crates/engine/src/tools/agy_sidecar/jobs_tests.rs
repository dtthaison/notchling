use super::jobs::{Cancelled, Job, Kind, Outcome, Queue, Stops};
use serde_json::json;
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
