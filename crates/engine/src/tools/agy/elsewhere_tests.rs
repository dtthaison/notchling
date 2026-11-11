use super::*;
use crate::accounts::procs::FakeProcs;
use crate::sessions::testutil::TempDir;

const SESSION: &str = "a9394b3f-f90c-4789-8e2d-dac1775db79a";

/// The shapes `agy` 1.5 writes (2026-11-11): a delegation's prompt, tool steps, the answer, then a
/// second prompt through another process that hasn't answered yet.
fn log() -> String {
    [
        r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-11-11T16:43:10Z","content":"<USER_REQUEST>\nReview the script, read-only.\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\nThe current local time is: 2026-11-11T23:43:10+07:00.\n</ADDITIONAL_METADATA>"}"#,
        r#"{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-11T16:43:10Z","thinking":"…","tool_calls":[{"name":"view_file"}]}"#,
        r#"{"step_index":2,"source":"MODEL","type":"GENERIC","status":"DONE","created_at":"2026-11-11T16:44:07Z","content":"Created At: 2026-11-11T23:44:07+07:00"}"#,
        r#"{"step_index":18,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-11T16:44:54Z","content":"VERDICT: FIX\n\n1. S26, S27"}"#,
        "not json",
        r#"{"step_index":3,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-11-11T16:51:50Z","content":"<USER_REQUEST>\nare you working?\n</USER_REQUEST>"}"#,
        r#"{"step_index":4,"source":"SYSTEM","type":"SYSTEM_MESSAGE","status":"DONE","created_at":"2026-11-11T16:51:50Z","content":"server restart"}"#,
    ]
    .join("\n")
}

fn texts(c: &Conversation) -> Vec<String> {
    c.items
        .iter()
        .map(|i| match &i.kind {
            ItemKind::User { text, .. } => format!("you: {text}"),
            ItemKind::Assistant { text } => format!("agy: {text}"),
            ItemKind::Notice { text, .. } => format!("note: {text}"),
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn the_log_shows_your_messages_and_its_replies_and_the_note() {
    let tmp = TempDir::new("agy-elsewhere-log");
    tmp.write(
        &format!("brain/{SESSION}/.system_generated/logs/transcript.jsonl"),
        &log(),
    );
    let (c, working) = from_log(tmp.root(), SESSION, DELEGATION_NOTE);
    assert_eq!(
        texts(&c),
        [
            "you: Review the script, read-only.",
            "agy: VERDICT: FIX\n\n1. S26, S27",
            "you: are you working?",
            &format!("note: {DELEGATION_NOTE}"),
        ]
    );
    assert!(working, "the last message has no reply yet");
    assert_eq!(c.activity.as_deref(), Some("Working elsewhere…"));
    assert_eq!(c.items[0].date, Some(1_794_415_390.0));
    let ids: Vec<&str> = c.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(
        ids,
        ["agy-log-0", "agy-log-3", "agy-log-5", "agy-elsewhere"]
    );
}

#[test]
fn an_answered_turn_is_not_working_and_a_missing_log_is_only_the_note() {
    let lines: Vec<String> = log().lines().take(4).map(str::to_owned).collect();
    assert!(!from_lines(&lines).1);
    let tmp = TempDir::new("agy-elsewhere-none");
    let (c, working) = from_log(tmp.root(), SESSION, OTHER_NOTE);
    assert_eq!(texts(&c), [format!("note: {OTHER_NOTE}")]);
    assert!(!working);
}

#[test]
fn lsof_names_the_agy_processes_only() {
    let out = "p64817\ncagy\nf87\np65070\ncagy\nf90\np999\ncmdworker\np64817\ncagy\n";
    assert_eq!(agy_pids(out), [64817, 65070]);
    assert!(agy_pids("").is_empty());
}

#[test]
fn a_delegation_is_known_by_its_parent() {
    assert!(is_delegation(
        "/Applications/Notchling.app/Contents/MacOS/notchling delegate agy --model gemini-4-argon -- review"
    ));
    assert!(is_delegation(
        "/Applications/Notchling.app/Contents/MacOS/Notchling --delegate agy -- x"
    ));
    assert!(!is_delegation(
        "/Applications/Notchling.app/Contents/MacOS/notchling daemon --socket s"
    ));
    assert!(!is_delegation("-zsh"));
    assert!(!is_delegation("/bin/echo delegate"));
}

fn holder_rig(tmp: &TempDir) -> (FakeProcs, String) {
    let dir = brain_dir(tmp.root(), SESSION);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.to_string_lossy().into_owned();
    let procs = FakeProcs::new()
        .answer(
            &format!("/usr/sbin/lsof -nP -F pc -- {dir}"),
            "p65070\ncagy\np64817\ncagy\n",
        )
        .answer("/bin/ps -o ppid= -p 65070", "  17468\n")
        .answer("/bin/ps -o ppid= -p 64817", "  64800\n")
        .answer(
            "/bin/ps -o command= -p 64800",
            "/Applications/Notchling.app/Contents/MacOS/notchling delegate agy -- review\n",
        );
    (procs, dir)
}

#[test]
fn the_daemons_own_agy_is_not_another_holder() {
    let tmp = TempDir::new("agy-elsewhere-find");
    let (procs, _) = holder_rig(&tmp);
    assert_eq!(
        find(&procs, tmp.root(), SESSION, 17468),
        Some(Holder {
            pid: 64817,
            delegation: true
        })
    );
    assert_eq!(
        find(&procs, tmp.root(), SESSION, 17468).unwrap().note(),
        DELEGATION_NOTE
    );
}

#[test]
fn only_the_daemons_own_or_none_means_nothing_else_holds_it() {
    let tmp = TempDir::new("agy-elsewhere-own");
    let dir = brain_dir(tmp.root(), SESSION);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.to_string_lossy().into_owned();
    let procs = FakeProcs::new()
        .answer(
            &format!("/usr/sbin/lsof -nP -F pc -- {dir}"),
            "p65070\ncagy\n",
        )
        .answer("/bin/ps -o ppid= -p 65070", "17468\n");
    assert_eq!(find(&procs, tmp.root(), SESSION, 17468), None);
    assert_eq!(find(&FakeProcs::new(), tmp.root(), SESSION, 17468), None);
    // No folder: nothing is asked at all.
    let calls = FakeProcs::new();
    assert_eq!(find(&calls, tmp.root(), "never-made", 1), None);
    assert!(calls.calls().is_empty());
}
