use super::*;

fn pairs(items: &[(&str, u8)]) -> Vec<(String, u8)> {
    items.iter().map(|(p, v)| ((*p).to_owned(), *v)).collect()
}

#[test]
fn tick_every_15s() {
    let mut gate = SlowGate::default();
    assert!(gate.ask(Why::Tick, Some("/w"), true, 1_000));
    gate.done(false, 1_100);
    for at in [1_200, 5_000, 6_000, 15_999] {
        assert!(!gate.ask(Why::Tick, Some("/w"), true, at), "{at}");
    }
    assert!(gate.ask(Why::Tick, Some("/w"), true, 16_000));
    gate.done(false, 16_100);
    assert!(!gate.ask(Why::Tick, Some("/w"), true, 16_200));
    assert!(gate.ask(Why::Tick, Some("/new"), true, 16_300));
}

#[test]
fn opened_and_folder_at_once_with_1s_floor() {
    let mut gate = SlowGate::default();
    assert!(gate.ask(Why::Opened, Some("/w"), true, 0));
    gate.done(false, 10);
    assert!(!gate.ask(Why::Opened, Some("/w"), true, 500));
    assert!(!gate.ask(Why::Folder, Some("/w"), true, 999));
    assert!(gate.ask(Why::Folder, Some("/w"), true, 1_000));
    gate.done(false, 1_010);
    assert!(gate.ask(Why::Folder, Some("/other"), true, 1_020));
    gate.done(false, 1_030);
    assert!(gate.ask(Why::Opened, Some("/w"), true, 1_040));
}

#[test]
fn turn_end_1s_floor() {
    let mut gate = SlowGate::default();
    assert!(gate.ask(Why::TurnEnded, Some("/w"), true, 0));
    gate.done(false, 10);
    assert!(!gate.ask(Why::TurnEnded, Some("/w"), true, 999));
    assert!(gate.ask(Why::TurnEnded, Some("/w"), true, 1_000));
    gate.done(false, 1_010);
    assert!(!gate.ask(Why::TurnEnded, Some("/w"), true, 1_500));
    assert!(
        !gate.ask(Why::TurnEnded, Some("/other"), true, 1_600),
        "a turn end never waits on a folder change"
    );
}

#[test]
fn one_in_flight() {
    let mut gate = SlowGate::default();
    assert!(gate.ask(Why::Opened, Some("/w"), true, 0));
    assert!(!gate.ask(Why::Opened, Some("/other"), true, 20_000));
    assert!(!gate.ask(Why::TurnEnded, Some("/w"), true, 20_001));
    assert!(!gate.ask(Why::Tick, Some("/w"), true, 40_000));
    gate.done(false, 40_001);
    assert!(gate.ask(Why::Tick, Some("/w"), true, 40_002));
}

#[test]
fn nothing_while_hidden_or_without_folder() {
    let mut gate = SlowGate::default();
    assert!(!gate.ask(Why::Opened, None, true, 0));
    assert!(!gate.ask(Why::Opened, Some(""), true, 0));
    assert!(!gate.ask(Why::Opened, Some("/w"), false, 0));
    assert!(!gate.ask(Why::Tick, Some("/w"), false, 100_000));
    assert!(gate.ask(Why::Opened, Some("/w"), true, 100_001));
}

#[test]
fn pending_retry_after_2s() {
    let mut gate = SlowGate::default();
    assert!(gate.ask(Why::Tick, Some("/w"), true, 0));
    gate.done(true, 100);
    assert!(!gate.ask(Why::Tick, Some("/w"), true, 1_000));
    assert!(!gate.ask(Why::Tick, Some("/w"), true, 2_099));
    assert!(gate.ask(Why::Tick, Some("/w"), true, 2_100));
    gate.done(false, 2_200);
    assert!(
        !gate.ask(Why::Tick, Some("/w"), true, 5_000),
        "a complete reply ends the retries"
    );
    assert!(gate.ask(Why::Tick, Some("/w"), true, 17_100));
    gate.done(true, 17_200);
    assert!(!gate.ask(Why::Tick, Some("/w"), false, 20_000));
    assert!(gate.ask(Why::Tick, Some("/w"), true, 20_001));
    gate.done(true, 20_100);
    assert!(gate.ask(Why::Opened, Some("/w"), true, 21_200));
    gate.done(false, 21_300);
    assert!(
        !gate.ask(Why::Tick, Some("/w"), true, 24_000),
        "an ask that went clears the retry"
    );
}

#[test]
fn fresh_after_names_changed_worktrees_at_most_two() {
    let before = pairs(&[("/a", 0), ("/b", 0), ("/c", 0), ("/d", 1)]);
    let same = before.clone();
    assert!(fresh_after(&before, &same).is_empty());
    let one = pairs(&[("/a", 0), ("/b", 2), ("/c", 0), ("/d", 1)]);
    assert_eq!(fresh_after(&before, &one), vec!["/b".to_owned()]);
    let many = pairs(&[("/a", 3), ("/b", 3), ("/c", 3), ("/d", 3)]);
    assert_eq!(
        fresh_after(&before, &many),
        vec!["/a".to_owned(), "/b".to_owned()]
    );
    let added = pairs(&[("/a", 0), ("/b", 0), ("/c", 0), ("/d", 1), ("/e", 1)]);
    assert!(fresh_after(&before, &added).is_empty());
    assert!(fresh_after(&before, &[]).is_empty());
    assert!(fresh_after::<u8>(&[], &before).is_empty());
}

#[test]
fn reset_lets_the_next_ask_through() {
    let mut gate = SlowGate::default();
    assert!(gate.ask(Why::Opened, Some("/w"), true, 0));
    gate.done(true, 10);
    gate.reset();
    assert!(gate.ask(Why::Tick, Some("/w"), true, 20));
    gate.done(false, 30);
    gate.reset();
    assert!(gate.ask(Why::TurnEnded, Some("/w"), true, 40));
    gate.done(false, 50);
    gate.reset();
    assert!(gate.ask(Why::Opened, Some("/w"), true, 60));
}

#[test]
fn status_answers_every_5s_ask_worktrees_every_15s() {
    let mut gate = SlowGate::default();
    let mut asked = Vec::new();
    let mut have_rows = false;
    for step in 0..=12_i64 {
        let now = step * 5_000;
        if gate.ask(Why::Tick, Some("/w"), true, now) {
            asked.push(now);
            gate.done(false, now);
            have_rows = true;
        }
        if opened_wanted(true, step > 0, have_rows) && gate.ask(Why::Opened, Some("/w"), true, now)
        {
            asked.push(now);
            gate.done(false, now);
            have_rows = true;
        }
    }
    assert_eq!(asked, [0, 15_000, 30_000, 45_000, 60_000]);
}

#[test]
fn answers_alone_never_repeat_the_worktrees_call() {
    let mut gate = SlowGate::default();
    let mut rows = false;
    let mut asked = 0;
    for step in 0..=12_i64 {
        let now = step * 5_000;
        if opened_wanted(true, step > 0, rows) && gate.ask(Why::Opened, Some("/w"), true, now) {
            asked += 1;
            gate.done(false, now);
            rows = true;
        }
    }
    assert_eq!(asked, 1);
}

#[test]
fn the_first_answer_for_a_folder_asks_at_once() {
    let mut gate = SlowGate::default();
    assert!(gate.ask(Why::Tick, Some("/w"), true, 0));
    gate.done(false, 10);
    assert!(opened_wanted(true, false, true));
    assert!(gate.ask(Why::Opened, Some("/other"), true, 20));
    gate.done(false, 30);
    assert!(opened_wanted(true, true, false));
    assert!(!opened_wanted(true, true, true));
    assert!(!opened_wanted(false, false, false));
}

#[test]
fn the_minute_ticks_once() {
    let mut gate = MinuteGate::default();
    assert!(!gate.due(false, 0));
    assert!(gate.due(true, 30_000));
    assert!(!gate.due(true, 59_999));
    assert!(gate.due(true, 60_000));
    assert!(!gate.due(true, 119_999));
    assert!(!gate.due(false, 180_000));
    assert!(gate.due(true, 180_001));
    assert!(!gate.due(true, 181_000));
}
