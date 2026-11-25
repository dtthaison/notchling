use super::*;
use crate::search::chunk::hash;
use crate::search::embed::ModelId;

fn model() -> ModelId {
    ModelId("granite".to_string())
}

fn rows(from: usize, n: usize) -> Vec<([u8; 16], String)> {
    (from..from + n)
        .map(|i| {
            let text = format!("chunk {i}");
            (hash(&text), text)
        })
        .collect()
}

#[test]
fn releases_at_sixteen() {
    let q = Queue::new();
    q.push(&model(), rows(0, 15), 0.0);
    assert!(!q.due(1.0));
    q.push(&model(), rows(15, 1), 1.0);
    assert!(q.due(1.0));
    assert_eq!(q.len(), 16);
}

#[test]
fn releases_after_five_minutes_on_a_fake_clock() {
    let q = Queue::new();
    q.push(&model(), rows(0, 3), 100.0);
    assert!(!q.due(100.0 + WAKE_AFTER - 1.0));
    assert!(q.due(100.0 + WAKE_AFTER));
}

#[test]
fn the_age_is_the_oldest_row_not_the_newest() {
    let q = Queue::new();
    q.push(&model(), rows(0, 1), 0.0);
    q.push(&model(), rows(1, 1), 290.0);
    assert!(q.due(WAKE_AFTER));
}

#[test]
fn a_backlog_releases_at_once() {
    let q = Queue::new();
    assert!(!q.due(0.0), "an empty queue is never due");
    q.set_backlog(true);
    assert!(q.backlog());
    assert!(!q.due(0.0), "a backlog with nothing queued is not due");
    q.push(&model(), rows(0, 1), 0.0);
    assert!(q.due(0.0));
    q.set_backlog(false);
    assert!(!q.due(0.0));
}

#[test]
fn a_row_is_queued_once_even_while_in_flight() {
    let q = Queue::new();
    assert_eq!(q.push(&model(), rows(0, 4), 0.0), 4);
    assert_eq!(q.push(&model(), rows(0, 4), 1.0), 0);
    let taken = q.take(BATCH);
    assert_eq!(taken.len(), 4);
    assert_eq!(q.len(), 0);
    assert_eq!(q.held(&model()), 4);
    assert_eq!(
        q.push(&model(), rows(0, 4), 2.0),
        0,
        "in flight rows are not queued again"
    );
    let hashes: Vec<[u8; 16]> = taken.iter().map(|(h, _)| *h).collect();
    q.release(&model(), &hashes);
    assert_eq!(q.held(&model()), 0);
    assert_eq!(q.push(&model(), rows(0, 4), 3.0), 4);
}

#[test]
fn take_is_fifo_and_at_most_n() {
    let q = Queue::new();
    q.push(&model(), rows(0, 20), 0.0);
    let first = q.take(BATCH);
    assert_eq!(first.len(), BATCH);
    assert_eq!(first[0].1, "chunk 0");
    assert_eq!(first[7].1, "chunk 7");
    assert_eq!(q.take(100).len(), 12);
    assert!(q.is_empty());
}

#[test]
fn take_serves_one_model_at_a_time() {
    let q = Queue::new();
    let other = ModelId("other".to_string());
    q.push(&model(), rows(0, 2), 0.0);
    q.push(&other, rows(0, 2), 1.0);
    assert_eq!(q.next_model(), Some(model()));
    assert_eq!(q.take(BATCH).len(), 2);
    assert_eq!(q.next_model(), Some(other.clone()));
    q.drop_model(&other);
    assert_eq!(q.next_model(), None);
    assert_eq!(q.held(&other), 0);
}

#[test]
fn prune_drops_dead_pending_rows_and_leaves_in_flight_alone() {
    let q = Queue::new();
    q.push(&model(), rows(0, 2), 0.0);
    let in_flight = q.take(BATCH);
    assert_eq!(in_flight.len(), 2);
    q.push(&model(), rows(2, 3), 1.0);
    assert_eq!(q.pending_hashes().len(), 3);
    let live: std::collections::HashSet<[u8; 16]> = [hash("chunk 3")].into();
    assert_eq!(q.prune(&live), 2);
    assert_eq!(q.len(), 1);
    assert_eq!(q.held(&model()), 3);
    assert_eq!(q.push(&model(), rows(0, 1), 2.0), 0);
    assert_eq!(q.push(&model(), rows(2, 1), 2.0), 1);
    let taken = q.take(BATCH);
    let texts: Vec<String> = taken.into_iter().map(|(_, t)| t).collect();
    assert_eq!(texts, vec!["chunk 3".to_string(), "chunk 2".to_string()]);
}
