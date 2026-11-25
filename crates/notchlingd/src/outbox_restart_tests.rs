use crate::outbox_postman::{Postman, TICK_MS};
use crate::outbox_testkit::{rig, Rig, DRAFT, IDLE, T0};
use notchling_engine::outbox::{Row, State, Verdict, Why, TURN_SETTLE_MS};
use notchling_engine::reply::keys::{self, ENTER};
use std::sync::atomic::Ordering;

const RULE: &str = "────────────────────────────────────────";
const ID: &str = "job-idle";

fn ignored(r: &Rig, text: &str, tried_at: i64) -> (Postman, Row) {
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    r.deaf.store(true, Ordering::SeqCst);
    let head = r.send(ID, text);
    let mut p = r.postmen.postman(IDLE);
    p.pass(r.at(T0));
    p.pass(r.at(tried_at));
    let tried = r.row(&head.id).unwrap();
    assert_eq!(tried.tries, 1);
    assert_eq!((tried.state, tried.why), (State::Waiting, Some(Why::Retry)));
    assert!(tried.unsent, "the ignored Enter is kept on the row");
    r.deaf.store(false, Ordering::SeqCst);
    (p, tried)
}

fn forget_the_tries(r: &Rig) {
    assert!(
        !r.acting.attempts.recent(ID, T0 + TURN_SETTLE_MS).is_empty(),
        "the ignored try was noted in memory"
    );
    r.acting.attempts.forget(ID);
    assert!(r.acting.attempts.recent(ID, T0 + TURN_SETTLE_MS).is_empty());
}

fn restart(r: &Rig, p: Postman) -> Postman {
    drop(p);
    forget_the_tries(r);
    r.restarted().postman(IDLE)
}

fn show_box(r: &Rig, text: &str) {
    let rows: Vec<String> = text
        .lines()
        .enumerate()
        .map(|(n, line)| match n {
            0 => format!("❯ {line}"),
            _ => format!("  {line}"),
        })
        .collect();
    let mut screen = vec![RULE];
    screen.extend(rows.iter().map(String::as_str));
    screen.extend([RULE, "  ? for shortcuts"]);
    r.show(&screen);
}

fn retry_passes(r: &Rig, p: &mut Postman, from: i64) {
    for step in 0..=4 {
        p.pass(r.at(from + step * TICK_MS));
    }
}

fn a_restarted_daemon_clears_and_types_the_marked_row_once(text: &str, tried_at: i64) {
    let r = rig();
    let (p, tried) = ignored(&r, text, tried_at);
    let mut p = restart(&r, p);
    show_box(&r, text);
    let writes = r.writes.lock().unwrap().len();
    retry_passes(&r, &mut p, tried.next_ms.unwrap());
    assert_eq!(r.row(&tried.id), None, "the retry sent it");
    let again = r.writes.lock().unwrap()[writes..].to_vec();
    assert_eq!(
        again.iter().filter(|w| **w == keys::clear_input()).count(),
        1
    );
    assert_eq!(
        again
            .iter()
            .filter(|w| **w == keys::bracketed(text))
            .count(),
        1,
        "the text goes in once"
    );
    assert_eq!(again.iter().filter(|w| *w == ENTER).count(), 1);
    assert_eq!(again.first(), Some(&keys::clear_input()));
}

#[test]
fn a_restarted_daemon_clears_a_plain_message_an_ignored_enter_left_and_types_it_once() {
    a_restarted_daemon_clears_and_types_the_marked_row_once("is it right", T0);
}

#[test]
fn a_restarted_daemon_clears_a_many_line_message_an_ignored_enter_left_and_types_it_once() {
    a_restarted_daemon_clears_and_types_the_marked_row_once(
        "first the build\nthen the tests\n\nand tell me what failed",
        T0,
    );
}

#[test]
fn a_restarted_daemon_never_clears_other_text_in_the_box_of_a_marked_row() {
    let r = rig();
    let (p, tried) = ignored(&r, "is it right", T0);
    let mut p = restart(&r, p);
    r.show(DRAFT);
    r.verifier.waits.lock().unwrap().push_back(Verdict::Unsure);
    let writes = r.writes.lock().unwrap().len();
    retry_passes(&r, &mut p, tried.next_ms.unwrap());
    let row = r.row(&tried.id).unwrap();
    assert_eq!((row.state, row.why), (State::Unsure, Some(Why::NotFound)));
    assert_eq!(
        r.writes.lock().unwrap().len(),
        writes,
        "not a key over a draft that is not the row's"
    );
}
