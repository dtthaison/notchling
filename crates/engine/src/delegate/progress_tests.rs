use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

fn system() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "nl-delegate-progress-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&root);
    let system = root.join(".system_generated");
    for sub in ["subagents", "messages", "logs"] {
        fs::create_dir_all(system.join(sub)).unwrap();
    }
    system
}

fn write(system: &Path, file: &str, text: &str) {
    fs::write(system.join(file), text).unwrap();
}

const ASKED: &str = r#"{"step_index":5,"type":"PLANNER_RESPONSE","status":"DONE","content":"I asked the image generator."}"#;
const WOKEN: &str = r#"{"step_index":6,"type":"SYSTEM_MESSAGE","status":"DONE","content":"x"}"#;
const ANSWER: &str = r#"{"step_index":7,"type":"PLANNER_RESPONSE","status":"DONE","content":"Saved: /x/teapot.jpg"}"#;

#[test]
fn it_waits_for_the_subagent_and_then_for_the_parents_answer() {
    let s = system();
    let since = SystemTime::now();
    assert_eq!(progress(&s, since), Progress::Finished);

    write(
        &s,
        "subagents/sub1.json",
        r#"{"conversationId":"sub1","state":"SUBAGENT_STATE_ALIVE"}"#,
    );
    write(&s, "logs/transcript.jsonl", ASKED);
    let waiting = Progress::Waiting {
        subagents: vec!["sub1".into()],
    };
    assert_eq!(progress(&s, since), waiting);

    write(
        &s,
        "messages/m1.json",
        r#"{"id":"m1","sender":"sub1","content":"Here is the image"}"#,
    );
    write(&s, "messages/read.json", r#"{"m1":true}"#);
    write(&s, "logs/transcript.jsonl", &format!("{ASKED}\n{WOKEN}"));
    assert_eq!(progress(&s, since), waiting, "woken, not answered yet");

    write(
        &s,
        "logs/transcript.jsonl",
        &format!("{ASKED}\n{WOKEN}\n{ANSWER}"),
    );
    assert_eq!(
        progress(&s, since),
        Progress::Answered {
            text: "Saved: /x/teapot.jpg".into(),
            subagents: vec!["sub1".into()]
        }
    );
    let _ = fs::remove_dir_all(s.parent().unwrap());
}

#[test]
fn a_subagent_from_before_the_run_is_not_this_runs() {
    let s = system();
    write(&s, "subagents/old.json", r#"{"conversationId":"old"}"#);
    let file = fs::OpenOptions::new()
        .write(true)
        .open(s.join("subagents/old.json"))
        .unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(3600))
        .unwrap();
    assert_eq!(progress(&s, SystemTime::now()), Progress::Finished);
    let _ = fs::remove_dir_all(s.parent().unwrap());
}

#[test]
fn an_unfinished_or_unusable_answer_keeps_waiting() {
    let s = system();
    let since = SystemTime::now();
    write(&s, "subagents/s.json", r#"{"conversationId":"s"}"#);
    write(&s, "subagents/broken.json", "not json");
    write(&s, "messages/m.json", r#"{"sender":"s"}"#);
    let waiting = Progress::Waiting {
        subagents: vec!["s".into()],
    };
    assert_eq!(progress(&s, since), waiting);
    for tail in [
        r#"{"type":"PLANNER_RESPONSE","status":"RUNNING","content":"x"}"#,
        r#"{"type":"PLANNER_RESPONSE","status":"DONE","content":"  "}"#,
        r#"{"type":"TOOL_CALL","status":"DONE","content":"x"}"#,
        "not json at all",
    ] {
        write(&s, "logs/transcript.jsonl", &format!("{WOKEN}\n{tail}\n"));
        assert_eq!(progress(&s, since), waiting, "{tail}");
    }
    let _ = fs::remove_dir_all(s.parent().unwrap());
}

#[test]
fn a_huge_transcript_is_read_from_its_end() {
    let s = system();
    let since = SystemTime::now();
    write(&s, "subagents/s.json", r#"{"conversationId":"s"}"#);
    write(&s, "messages/m.json", r#"{"sender":"s"}"#);
    let filler = format!(
        "{{\"type\":\"TOOL_CALL\",\"content\":\"{}\"}}\n",
        "x".repeat(1000)
    );
    let mut text = filler.repeat(6000);
    text.push_str(&format!("{WOKEN}\n{ANSWER}"));
    write(&s, "logs/transcript.jsonl", &text);
    assert!(matches!(
        progress(&s, since),
        Progress::Answered { ref text, .. } if text == "Saved: /x/teapot.jpg"
    ));
    let _ = fs::remove_dir_all(s.parent().unwrap());
}

#[test]
fn waiting_ends_when_the_files_say_so() {
    let s = system();
    let since = SystemTime::now();
    write(&s, "subagents/sub1.json", r#"{"conversationId":"sub1"}"#);
    write(&s, "logs/transcript.jsonl", ASKED);
    let writer = {
        let s = s.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            write(&s, "messages/m1.json", r#"{"sender":"sub1"}"#);
            std::thread::sleep(Duration::from_millis(400));
            write(
                &s,
                "logs/transcript.jsonl",
                &format!("{ASKED}\n{WOKEN}\n{ANSWER}"),
            );
        })
    };
    let mut told = Vec::new();
    let waited = wait(&s, since, &|| false, &mut |n| told.push(n));
    writer.join().unwrap();
    assert_eq!(told, [1], "announced once, with the count");
    assert_eq!(
        waited,
        Waited::Done {
            text: Some("Saved: /x/teapot.jpg".into()),
            subagents: vec!["sub1".into()]
        }
    );
    let _ = fs::remove_dir_all(s.parent().unwrap());
}

#[test]
fn nothing_to_wait_for_is_done_at_once_and_a_stop_ends_a_wait() {
    let s = system();
    let since = SystemTime::now();
    let mut told = 0;
    assert_eq!(
        wait(&s, since, &|| false, &mut |_| told += 1),
        Waited::Done {
            text: None,
            subagents: vec![]
        }
    );
    assert_eq!(told, 0);

    write(&s, "subagents/sub1.json", r#"{"conversationId":"sub1"}"#);
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        flag.store(true, Ordering::SeqCst);
    });
    let waited = wait(&s, since, &|| stop.load(Ordering::SeqCst), &mut |_| {});
    stopper.join().unwrap();
    assert_eq!(
        waited,
        Waited::Stopped {
            subagents: vec!["sub1".into()]
        }
    );
    let _ = fs::remove_dir_all(s.parent().unwrap());
}

#[test]
fn a_folder_that_vanishes_cannot_be_watched() {
    let s = system();
    let since = SystemTime::now();
    write(&s, "subagents/sub1.json", r#"{"conversationId":"sub1"}"#);
    let gone = s.clone();
    let waited = wait(&gone, since, &|| false, &mut |_| {
        let _ = fs::remove_dir_all(gone.parent().unwrap());
    });
    assert_eq!(waited, Waited::CannotWatch);
}

#[test]
fn only_plain_ids_get_a_media_folder_of_their_own() {
    let ids = media_sessions(
        "main",
        &["sub".into(), "../x".into(), "a/b".into(), "".into()],
    );
    assert_eq!(ids, ["main", "sub"]);
}
