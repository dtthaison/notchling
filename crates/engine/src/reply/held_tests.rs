use super::*;
use crate::sessions::agent::sample;
use crate::sessions::{AgentGroup, AgentTarget};

fn calm_of(rows: &[Agent]) -> BTreeSet<String> {
    rows.iter()
        .filter(|row| row.takes_held_text())
        .map(|row| row.id.clone())
        .collect()
}

trait Settled {
    fn settled(&mut self, rows: &[Agent], now_ms: i64) -> Vec<(String, String)>;
}

impl Settled for Held {
    fn settled(&mut self, rows: &[Agent], now_ms: i64) -> Vec<(String, String)> {
        let calm = calm_of(rows);
        self.ready(rows, now_ms - SETTLE_MS, &calm);
        self.ready(rows, now_ms, &calm)
    }
}

fn row(id: &str, group: AgentGroup) -> Agent {
    let mut agent = sample(AgentTarget::Job("ab12".into()), None);
    agent.id = id.into();
    agent.group = group;
    agent
}

fn at_prompt(id: &str, group: AgentGroup) -> Agent {
    let mut agent = row(id, group);
    agent.at_prompt = true;
    agent
}

#[test]
fn an_idle_session_gets_the_message_at_once() {
    let mut held = Held::default();
    assert_eq!(held.route("a", "one", false, 0), Route::Send);
    assert_eq!(held.get("a"), None);
}

#[test]
fn messages_sent_while_it_works_join_one_held_message() {
    let mut held = Held::default();
    assert_eq!(held.route("a", "one", false, 0), Route::Send);
    assert_eq!(
        held.route("a", "two", true, 1_000),
        Route::Hold("two".into())
    );
    assert_eq!(
        held.route("a", "three", true, 2_000),
        Route::Hold("two\n\nthree".into())
    );
    assert_eq!(held.all().len(), 1);
}

#[test]
fn a_message_right_after_a_send_waits_even_before_the_row_says_working() {
    let mut held = Held::default();
    held.route("a", "one", false, 0);
    assert_eq!(
        held.route("a", "two", false, 500),
        Route::Hold("two".into())
    );
}

#[test]
fn the_held_message_goes_once_the_turn_ends() {
    let mut held = Held::default();
    held.route("a", "one", false, 0);
    held.route("a", "two", true, 1_000);
    held.route("a", "three", true, 1_500);
    assert!(held
        .settled(&[row("a", AgentGroup::Working)], 2_000)
        .is_empty());
    assert_eq!(
        held.settled(&[row("a", AgentGroup::Completed)], 9_000),
        vec![("a".to_string(), "two\n\nthree".to_string())]
    );
    assert_eq!(held.get("a"), None);
    assert_eq!(
        held.route("a", "four", false, 9_500),
        Route::Hold("four".into()),
        "the combined one was just sent, so the next waits for its turn"
    );
}

#[test]
fn it_waits_while_the_session_asks_something() {
    let mut held = Held::default();
    held.route("a", "two", true, 0);
    assert!(held
        .settled(&[row("a", AgentGroup::NeedsInput)], 1_000)
        .is_empty());
    assert!(held.get("a").is_some());
}

#[test]
fn a_turn_too_quick_to_be_seen_working_still_lets_it_go_after_the_grace() {
    let mut held = Held::default();
    held.route("a", "one", false, 0);
    held.route("a", "two", false, 100);
    assert!(held
        .settled(&[row("a", AgentGroup::Completed)], 1_000)
        .is_empty());
    assert_eq!(
        held.settled(&[row("a", AgentGroup::Completed)], START_GRACE_MS),
        vec![("a".to_string(), "two".to_string())]
    );
}

#[test]
fn a_failed_send_goes_back_in_front_and_waits_before_trying_again() {
    let mut held = Held::default();
    held.route("a", "two", true, 0);
    let out = held.settled(&[row("a", AgentGroup::Completed)], 1_000);
    held.route("a", "three", true, 1_100);
    held.put_back("a", &out[0].1, 1_200);
    assert_eq!(held.get("a"), Some("two\n\nthree"));
    assert!(held
        .settled(&[row("a", AgentGroup::Completed)], 2_000)
        .is_empty());
}

#[test]
fn a_direct_send_that_failed_does_not_hold_the_next() {
    let mut held = Held::default();
    held.route("a", "one", false, 0);
    held.not_sent("a");
    assert_eq!(held.route("a", "again", false, 100), Route::Send);
}

#[test]
fn held_text_goes_when_the_turn_ended_though_background_tasks_keep_the_row_working() {
    let mut held = Held::default();
    held.route("a", "two", true, 0);
    assert!(held
        .settled(&[row("a", AgentGroup::Working)], 1_000)
        .is_empty());
    assert_eq!(
        held.settled(&[at_prompt("a", AgentGroup::Working)], 2_000),
        vec![("a".to_string(), "two".to_string())]
    );
}

#[test]
fn held_text_goes_to_a_session_that_ended_its_turn_asking_in_text() {
    let mut held = Held::default();
    held.route("a", "two", true, 0);
    assert_eq!(
        held.settled(&[at_prompt("a", AgentGroup::NeedsInput)], 1_000),
        vec![("a".to_string(), "two".to_string())]
    );
}

#[test]
fn a_row_working_only_for_background_tasks_does_not_end_the_grace_after_a_send() {
    let mut held = Held::default();
    assert_eq!(held.route("a", "one", false, 0), Route::Send);
    assert!(held
        .settled(&[at_prompt("a", AgentGroup::Working)], 500)
        .is_empty());
    assert_eq!(
        held.route("a", "two", false, 1_000),
        Route::Hold("two".into()),
        "the sent message has not started its turn yet"
    );
    assert!(held
        .settled(&[row("a", AgentGroup::Working)], 1_500)
        .is_empty());
    assert_eq!(
        held.route("a", "three", false, 2_000),
        Route::Hold("two\n\nthree".into())
    );
}

#[test]
fn a_message_that_trails_blank_lines_joins_as_one_paragraph_break() {
    let mut held = Held::default();
    held.route("a", "padding bottom\n\n", true, 0);
    assert_eq!(
        held.route("a", "\n\nhi\n", true, 1_000),
        Route::Hold("padding bottom\n\nhi\n".into())
    );
    assert_eq!(
        held.route("a", "again", true, 2_000),
        Route::Hold("padding bottom\n\nhi\n\nagain".into())
    );
}

#[test]
fn a_single_held_message_keeps_its_own_text() {
    let mut held = Held::default();
    assert_eq!(
        held.route("a", "  fn x() {\n    1\n  }\n", true, 0),
        Route::Hold("  fn x() {\n    1\n  }\n".into())
    );
}

fn ids(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| n.to_string()).collect()
}

#[test]
fn held_text_goes_only_after_the_row_has_stood_calm_for_the_settle_time() {
    let mut held = Held::default();
    held.route("a", "two", true, 0);
    let rows = [row("a", AgentGroup::Completed)];
    assert!(held.ready(&rows, 1_000, &ids(&["a"])).is_empty());
    assert!(held
        .ready(&rows, 1_000 + SETTLE_MS - 1, &ids(&["a"]))
        .is_empty());
    assert_eq!(
        held.ready(&rows, 1_000 + SETTLE_MS, &ids(&["a"])),
        vec![("a".to_string(), "two".to_string())]
    );
}

#[test]
fn a_reading_that_is_not_calm_starts_the_settle_over() {
    let mut held = Held::default();
    held.route("a", "two", true, 0);
    let rows = [row("a", AgentGroup::Completed)];
    assert!(held.ready(&rows, 1_000, &ids(&["a"])).is_empty());
    assert!(held.ready(&rows, 2_000, &ids(&[])).is_empty());
    assert!(held.ready(&rows, 3_000, &ids(&["a"])).is_empty());
    assert!(held
        .ready(&rows, 3_000 + SETTLE_MS - 1, &ids(&["a"]))
        .is_empty());
    assert_eq!(held.ready(&rows, 3_000 + SETTLE_MS, &ids(&["a"])).len(), 1);
}

#[test]
fn a_row_the_cache_still_calls_working_gets_nothing_though_a_fresh_reading_was_calm() {
    let mut held = Held::default();
    held.route("a", "two", true, 0);
    let rows = [row("a", AgentGroup::Working)];
    assert!(held.ready(&rows, 1_000, &ids(&["a"])).is_empty());
    assert!(held.ready(&rows, 9_000, &ids(&["a"])).is_empty());
    assert_eq!(held.get("a"), Some("two"));
}

#[test]
fn a_put_back_text_must_settle_again() {
    let mut held = Held::default();
    held.route("a", "two", true, 0);
    let rows = [row("a", AgentGroup::Completed)];
    held.ready(&rows, 1_000, &ids(&["a"]));
    let out = held.ready(&rows, 1_000 + SETTLE_MS, &ids(&["a"]));
    held.put_back("a", &out[0].1, 5_000);
    let later = 5_000 + START_GRACE_MS;
    assert!(held.ready(&rows, later, &ids(&["a"])).is_empty());
    assert_eq!(held.ready(&rows, later + SETTLE_MS, &ids(&["a"])).len(), 1);
}

#[test]
fn only_ids_with_held_text_are_waiting() {
    let mut held = Held::default();
    assert!(held.waiting().is_empty());
    held.route("b", "x", true, 0);
    held.route("a", "y", true, 0);
    assert_eq!(held.waiting(), ["a", "b"]);
}

const BOX: &str = "Waiting — the terminal box has other text";

fn flushed(held: &mut Held, rows: &[Agent], now_ms: i64) -> String {
    let out = held.settled(rows, now_ms);
    assert_eq!(out.len(), 1, "it was released");
    out[0].1.clone()
}

#[test]
fn a_failed_flush_keeps_its_text_and_says_why_and_tries_again_later_than_a_plain_retry() {
    let mut held = Held::default();
    held.route("a", "two", true, 0);
    let rows = [row("a", AgentGroup::Completed)];
    let text = flushed(&mut held, &rows, 1_000);
    assert!(held.failed("a", &text, 5_000, false, Some(BOX)));
    assert_eq!(held.get("a"), Some("two"));
    assert_eq!(held.note("a"), Some(BOX));
    assert_eq!(held.tried("a"), None, "nothing of it was typed");
    let plain = 5_000 + START_GRACE_MS + SETTLE_MS;
    assert!(held.settled(&rows, plain).is_empty(), "not yet");
    let later = 5_000 + BLOCKED_RETRY_MS;
    let calm = calm_of(&rows);
    assert!(held.ready(&rows, later - 1, &calm).is_empty());
    assert_eq!(held.ready(&rows, later, &calm).len(), 1);
}

#[test]
fn a_text_that_went_into_the_box_remembers_how_much_and_when_even_through_a_later_refusal() {
    let mut held = Held::default();
    held.route("a", "two", true, 0);
    let rows = [row("a", AgentGroup::Completed)];
    let text = flushed(&mut held, &rows, 1_000);
    held.failed("a", &text, 5_000, true, Some("Typed along with other text"));
    assert_eq!(held.tried("a"), Some(Tried { len: 3, at: 1_000 }));
    held.route("a", "three", true, 6_000);
    assert_eq!(held.get("a"), Some("two\n\nthree"));
    let text = flushed(&mut held, &rows, 5_000 + BLOCKED_RETRY_MS + SETTLE_MS);
    assert_eq!(text, "two\n\nthree");
    held.failed("a", &text, 50_000, false, Some(BOX));
    assert_eq!(
        held.tried("a"),
        Some(Tried { len: 3, at: 1_000 }),
        "a refusal before typing changes nothing about what was tried"
    );
    assert_eq!(held.note("a"), Some(BOX));
    let tried = held.tried("a").unwrap();
    assert_eq!(after_tried(held.get("a").unwrap(), tried), "three");
}

#[test]
fn what_was_tried_is_cut_off_the_front_with_its_blank_line() {
    let tried = Tried { len: 3, at: 0 };
    assert_eq!(after_tried("two\n\nthree", tried), "three");
    assert_eq!(after_tried("two", tried), "");
    assert_eq!(after_tried("tw", tried), "");
}

#[test]
fn a_flush_that_went_in_clears_the_note_and_what_was_tried() {
    let mut held = Held::default();
    held.route("a", "two", true, 0);
    let rows = [row("a", AgentGroup::Completed)];
    let text = flushed(&mut held, &rows, 1_000);
    held.failed("a", &text, 5_000, true, Some("Typed along with other text"));
    let text = flushed(&mut held, &rows, 5_000 + BLOCKED_RETRY_MS + SETTLE_MS);
    held.settled_ok("a");
    assert_eq!(text, "two");
    assert_eq!(held.tried("a"), None);
    assert_eq!(held.note("a"), None);
}

#[test]
fn a_discard_forgets_the_text_its_note_and_what_was_tried() {
    let mut held = Held::default();
    held.route("a", "two", true, 0);
    let rows = [row("a", AgentGroup::Completed)];
    let text = flushed(&mut held, &rows, 1_000);
    held.failed("a", &text, 5_000, true, Some("Typed along with other text"));
    assert!(held.discard("a"));
    assert_eq!(held.get("a"), None);
    assert_eq!(held.note("a"), None);
    assert_eq!(held.tried("a"), None);
    assert!(!held.discard("a"), "nothing left to discard");
}

#[test]
fn a_discard_while_the_flush_is_typing_means_a_failure_is_not_put_back() {
    let mut held = Held::default();
    held.route("a", "two", true, 0);
    let rows = [row("a", AgentGroup::Completed)];
    let text = flushed(&mut held, &rows, 1_000);
    assert!(held.discard("a"), "it is in flight, the user sees it held");
    assert!(!held.failed("a", &text, 5_000, true, Some(BOX)));
    assert_eq!(held.get("a"), None);
    assert_eq!(held.tried("a"), None);
}

#[test]
fn the_text_a_person_sends_while_one_is_blocked_joins_it_and_the_note_stays() {
    let mut held = Held::default();
    held.route("a", "two", true, 0);
    let rows = [row("a", AgentGroup::Completed)];
    let text = flushed(&mut held, &rows, 1_000);
    held.failed("a", &text, 5_000, false, Some(BOX));
    held.route("a", "three", false, 6_000);
    assert_eq!(held.get("a"), Some("two\n\nthree"));
    assert_eq!(held.note("a"), Some(BOX));
}
