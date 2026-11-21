use super::*;
use notchling_engine::transcript::model::Conversation;

fn shell(id: &str, command: &str, started: Option<f64>) -> BackgroundShell {
    BackgroundShell {
        id: id.into(),
        command: command.into(),
        title: String::new(),
        output_path: format!("/tmp/{id}.output"),
        started,
    }
}

fn none(_: &str) -> Vec<i32> {
    Vec::new()
}

fn one(_: &str) -> Vec<i32> {
    vec![1]
}

#[test]
fn a_running_shell_row_has_its_id_and_start() {
    let mut c = Conversation::default();
    c.shells.push(shell("b1", "sleep 5", Some(1000.0)));
    let rows = shell_rows(&c, None, &one);
    assert_eq!(rows[0].id, "b1");
    assert_eq!(rows[0].started, Some(1000));
    assert!(rows[0].running);
}

#[test]
fn the_running_word_counts_minutes_hours_and_days() {
    assert_eq!(running_word(None, 5000), "Running");
    assert_eq!(running_word(Some(1000), 1030), "Running");
    assert_eq!(running_word(Some(1000), 1000 + 47 * 60), "Running 47 min");
    assert_eq!(
        running_word(Some(1000), 1000 + 125 * 60),
        "Running 2 h 5 min"
    );
    assert_eq!(
        running_word(Some(1000), 1000 + 3 * 86_400 + 4 * 3600),
        "Running 3 d 4 h"
    );
    assert_eq!(running_word(Some(5000), 1000), "Running");
}

#[test]
fn only_a_running_row_can_look_stuck() {
    let looping = "while pgrep -f worker-name; do sleep 5; done";
    let mut c = Conversation::default();
    c.shells.push(shell("b1", looping, Some(1000.0)));
    c.shells.push(shell("b2", "pgrep vite; sleep 1", Some(1000.0)));
    c.shells
        .push(shell("b3", "while pgrep vite; do sleep 5; done", Some(1000.0)));
    let rows = shell_rows(&c, None, &one);
    let by = |id: &str| rows.iter().find(|r| r.id == id).unwrap();
    assert_eq!(stuck_of(by("b1"), 1010), Some(Stuck::Itself));
    assert_eq!(stuck_of(by("b2"), 99_999), None);
    assert_eq!(stuck_of(by("b3"), 1000 + 29 * 60), None);
    assert_eq!(stuck_of(by("b3"), 1000 + 31 * 60), Some(Stuck::LongLoop));
    let ended = shell_rows(&c, None, &none);
    assert_eq!(stuck_of(&ended[0], 99_999), None);
}

#[test]
fn a_stopped_shell_reads_stopped() {
    let (status, word, _) = ended_look("stopped");
    assert_eq!((status, word.as_str()), (Status::Needs, "Stopped"));
}
