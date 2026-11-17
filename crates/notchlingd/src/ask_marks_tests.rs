use crate::acting_ops_tests::SESSION;
use crate::ask_ops::{Mark, Taken};
use crate::ask_testkit::{base, changes, new_row, open, open_with, person, rig, ASKER, T0};
use crate::event_assert::of;
use notchling_engine::ask::{AskState, By, KEEP_CLOSED_MS};
use notchling_local_api::{ops, topics};
use serde_json::json;
use std::sync::Barrier;

fn probe(by: By) -> Mark {
    Mark {
        state: AskState::Answered,
        by: Some(by),
    }
}

#[test]
fn a_won_answer_keeps_its_mark() {
    let r = rig();
    let id = open(&r);
    assert_eq!(
        r.ops.answer(&id, None, Some(1), By::Mac).unwrap(),
        Taken::Won
    );
    let kept = r.ops.take(&id, probe(By::Phone)).unwrap();
    assert_eq!((kept.state, kept.by), (AskState::Answered, Some(By::Mac)));
}

#[test]
fn a_won_dismiss_keeps_its_mark() {
    let r = rig();
    let id = open(&r);
    assert!(r.ops.dismiss(&id).unwrap());
    let kept = r.ops.take(&id, probe(By::Phone)).unwrap();
    assert_eq!((kept.state, kept.by), (AskState::Dismissed, None));
}

#[test]
fn a_failed_or_lost_write_drops_its_mark() {
    let r = rig();
    let id = open(&r);
    r.store
        .close(&id, &[AskState::Open], AskState::Gone, T0)
        .unwrap();
    let row = r.store.get(&id).unwrap().unwrap();
    assert_eq!(row.state, AskState::Gone);
    let again = r
        .store
        .insert(&new_row(SESSION, "other", ASKER), T0)
        .unwrap();
    r.store
        .close(&again.id, &[AskState::Open], AskState::Gone, T0)
        .unwrap();
    let lost = r.ops.finish_answer(&again.id, "x", None, By::Mac).unwrap();
    assert!(matches!(lost, Taken::Lost { .. }));
    assert!(r.ops.take(&again.id, probe(By::Phone)).is_none());
}

#[test]
fn three_way_race_losers_name_the_winner() {
    let r = rig();
    for round in 0..1_500 {
        let row = r
            .store
            .insert(&new_row(SESSION, &format!("k{round}"), ASKER), T0)
            .unwrap();
        let id = row.id.clone();
        let gate = Barrier::new(3);
        let results: Vec<(By, Taken)> = std::thread::scope(|scope| {
            let handles: Vec<_> = [By::Mac, By::Phone, By::Main]
                .into_iter()
                .map(|by| {
                    let (id, gate, ops) = (&id, &gate, &r.ops);
                    scope.spawn(move || {
                        gate.wait();
                        (by, ops.answer(id, None, Some(1), by).unwrap())
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let stored = r.store.get(&id).unwrap().unwrap();
        let winners: Vec<By> = results
            .iter()
            .filter(|(_, taken)| *taken == Taken::Won)
            .map(|(by, _)| *by)
            .collect();
        assert_eq!(winners.len(), 1, "round {round}: {results:?}");
        assert_eq!(stored.by, Some(winners[0]), "round {round}");
        for (_, taken) in &results {
            if let Taken::Lost { state, by } = taken {
                assert_eq!(
                    (*state, *by),
                    (AskState::Answered, Some(winners[0])),
                    "round {round}: {results:?}"
                );
            }
        }
        r.store
            .close(&id, &[AskState::Answered], AskState::Delivered, T0)
            .unwrap();
    }
}

#[test]
fn a_dismiss_that_loses_to_an_answer_in_flight_names_it() {
    let r = rig();
    let id = open(&r);
    assert!(r.ops.take(&id, probe(By::Phone)).is_none());
    let reply = person(&r, ops::ASK_DISMISS, json!({"id": id})).unwrap();
    assert_eq!(reply["dismissed"], json!(false));
    assert_eq!(reply["state"], json!("answered"));
    assert_eq!(reply["by"], json!("phone"));
    assert_eq!(r.store.get(&id).unwrap().unwrap().state, AskState::Open);
    assert!(!r.ops.dismiss(&id).unwrap());
}

#[test]
fn a_dismiss_of_a_closed_question_says_its_state() {
    let r = rig();
    let id = open(&r);
    r.ops.answer(&id, None, Some(2), By::Main).unwrap();
    let reply = person(&r, ops::ASK_DISMISS, json!({"id": id})).unwrap();
    assert_eq!(
        reply,
        json!({"dismissed": false, "state": "answered", "by": "main"})
    );
}

#[test]
fn the_sweep_drops_the_marks_of_deleted_rows() {
    let r = rig();
    let id = open(&r);
    r.ops.answer(&id, None, Some(1), By::Mac).unwrap();
    r.store
        .close(
            &id,
            &[AskState::Answered],
            AskState::Delivered,
            T0 - KEEP_CLOSED_MS - 1,
        )
        .unwrap();
    r.ops.tick(T0);
    assert!(r.store.get(&id).unwrap().is_none());
    assert!(r.ops.take(&id, probe(By::Phone)).is_none());
}

#[test]
fn a_retried_open_finds_its_row_after_a_rekey() {
    let r = rig();
    let id = open(&r);
    r.store.rekey(SESSION, "moved").unwrap();
    r.ops.dismiss(&id).unwrap();
    let seen = changes(&r).len();
    let asked = of(&r.sink, "session.ask").len();
    let again = open_with(&r, base()).unwrap();
    assert_eq!(again, json!({"id": id, "state": "dismissed"}));
    assert_eq!(changes(&r).len(), seen);
    assert_eq!(of(&r.sink, "session.ask").len(), asked);
    assert_eq!(r.store.list(None).unwrap().len(), 1);
    let published = r
        .room
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _)| name == topics::ASK_CHANGED)
        .count();
    assert_eq!(published, seen);
}
