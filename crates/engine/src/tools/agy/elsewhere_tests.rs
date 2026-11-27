use super::*;
use crate::accounts::procs::FakeProcs;
use crate::sessions::testutil::TempDir;

const SESSION: &str = "a9394b3f-f90c-4789-8e2d-dac1775db79a";

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
            ItemKind::Thinking { text, .. } => format!("thought: {text}"),
            ItemKind::Tools(calls) => format!(
                "steps: {}",
                calls
                    .iter()
                    .map(|c| c.title.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn the_log_shows_your_messages_thinking_steps_and_replies_and_the_note() {
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
            "thought: …",
            "steps: view_file",
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
        [
            "agy-log-0",
            "agy-log-1-thinking",
            "agy-log-1-tools",
            "agy-log-3",
            "agy-log-5",
            "agy-elsewhere"
        ]
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
    let calls = FakeProcs::new();
    assert_eq!(find(&calls, tmp.root(), "never-made", 1), None);
    assert!(calls.calls().is_empty());
}

fn step(index: u32, kind: &str, status: &str, at: &str, extra: &str) -> String {
    format!(
        r#"{{"step_index":{index},"source":"MODEL","type":"{kind}","status":"{status}","created_at":"{at}"{extra}}}"#
    )
}

fn user(index: u32, at: &str, text: &str) -> String {
    let content = format!("<USER_REQUEST>\\n{text}\\n</USER_REQUEST>");
    step(
        index,
        "USER_INPUT",
        "DONE",
        at,
        &format!(r#","content":"{content}""#),
    )
}

fn done(index: u32, at: &str, output: &str) -> String {
    let header =
        "Created At: 2026-11-11T23:00:05+07:00\\nCompleted At: 2026-11-11T23:00:05+07:00\\n";
    step(
        index,
        "GENERIC",
        "DONE",
        at,
        &format!(r#","content":"{header}{output}""#),
    )
}

const T0: &str = "2026-11-11T16:00:00Z";
const T1: &str = "2026-11-11T16:00:04Z";
const T2: &str = "2026-11-11T16:00:20Z";
const READ_AND_LIST: &str = r#","thinking":"Open the notes, then list the folder.","output_tokens":120,"input_tokens":900,"cache_read_tokens":0,"tool_calls":[{"name":"read_file","args":{"Path":"\"/work/notes/NOTES.md\""}},{"name":"run_command","args":{"CommandLine":"\"ls -la\"","WaitMsBeforeAsync":"5000","toolSummary":"\"List the folder\""}}]"#;
const SIPS: &str = r#","thinking":"","output_tokens":80,"tool_calls":[{"name":"run_command","args":{"CommandLine":"\"sips -g pixelWidth logo.png\"","WaitMsBeforeAsync":"5000"}}]"#;

fn held(raw: &[String]) -> (Conversation, bool) {
    from_lines(raw)
}

fn calls(c: &Conversation) -> Vec<&crate::transcript::model::ToolCall> {
    c.items
        .iter()
        .flat_map(|i| match &i.kind {
            ItemKind::Tools(calls) => calls.iter().collect(),
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn a_held_turn_shows_its_thinking_and_steps_with_the_unfinished_one_open() {
    use crate::transcript::model::ToolKind;
    let (c, working) = held(&[
        user(0, T0, "Summarize the notes."),
        step(1, "PLANNER_RESPONSE", "DONE", T1, READ_AND_LIST),
        done(2, T1, "Showing lines 1-2 of 2\\nfirst\\nsecond"),
        done(
            3,
            T1,
            "Command exited with code 0.\\n\\nCommand output:\\nREADME",
        ),
        step(4, "PLANNER_RESPONSE", "DONE", T2, SIPS),
    ]);
    assert!(working, "the last call has no result yet");
    let kinds: Vec<&str> = c
        .items
        .iter()
        .map(|i| match &i.kind {
            ItemKind::User { .. } => "user",
            ItemKind::Thinking { .. } => "thinking",
            ItemKind::Tools(_) => "tools",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["user", "thinking", "tools"]);
    match &c.items[1].kind {
        ItemKind::Thinking { text, .. } => {
            assert_eq!(text, "Open the notes, then list the folder.")
        }
        other => panic!("{other:?}"),
    }
    let calls = calls(&c);
    assert_eq!(calls.len(), 3);
    let read = calls[0];
    assert_eq!(
        (read.title.as_str(), read.kind, read.input.as_str()),
        ("Read NOTES.md", ToolKind::Read, "/work/notes/NOTES.md")
    );
    assert!(read.finished && !read.is_error);
    assert_eq!(
        read.output.as_deref(),
        Some("Showing lines 1-2 of 2\nfirst\nsecond"),
        "the Created/Completed header is not part of the output"
    );
    let list = calls[1];
    assert_eq!(
        (list.title.as_str(), list.kind, list.input.as_str()),
        ("List the folder", ToolKind::Shell, "ls -la")
    );
    assert_eq!(
        list.output.as_deref(),
        Some("Command exited with code 0.\n\nCommand output:\nREADME")
    );
    let open = calls[2];
    assert_eq!(
        (open.title.as_str(), open.kind, open.input.as_str()),
        (
            "Ran sips -g pixelWidth logo.png",
            ToolKind::Shell,
            "sips -g pixelWidth logo.png"
        )
    );
    assert!(!open.finished && open.output.is_none());
    assert_eq!(c.turn_start, Some(1_794_412_800.0));
    assert_eq!(c.turn_tokens, 200);
}

#[test]
fn a_denied_command_is_a_failed_call_with_its_error() {
    let error = r#"permission check failed for unsandboxed \"sips -g all logo.png\": user denied permission to run command"#;
    let (c, working) = held(&[
        user(0, T0, "Check the image."),
        step(
            1,
            "PLANNER_RESPONSE",
            "DONE",
            T1,
            r#","tool_calls":[{"name":"run_command","args":{"CommandLine":"\"sips -g all logo.png\"","toolSummary":"\"Check the image\""}}]"#,
        ),
        step(
            2,
            "GENERIC",
            "ERROR",
            T1,
            &format!(
                r#","content":"Created At: 2026-11-11T23:00:05+07:00\nCompleted At: 2026-11-11T23:00:05+07:00\n","error":"{error}""#
            ),
        ),
        step(
            3,
            "PLANNER_RESPONSE",
            "DONE",
            T2,
            r#","content":"I could not run it.""#,
        ),
    ]);
    assert!(!working, "a reply with no calls ends the turn");
    let calls = calls(&c);
    assert_eq!(calls.len(), 1);
    assert!(calls[0].finished && calls[0].is_error);
    assert_eq!(
        calls[0].output.as_deref(),
        Some(
            r#"permission check failed for unsandboxed "sips -g all logo.png": user denied permission to run command"#
        )
    );
    assert!(matches!(
        &c.items.last().unwrap().kind,
        ItemKind::Assistant { text } if text == "I could not run it."
    ));
    assert_eq!(c.turn_start, None, "no turn goes on");
    assert_eq!(c.turn_tokens, 0);
}

#[test]
fn a_planner_step_with_text_and_calls_is_not_the_end_of_the_turn() {
    let (c, working) = held(&[
        user(0, T0, "Look around."),
        step(
            1,
            "PLANNER_RESPONSE",
            "DONE",
            T1,
            r#","content":"Let me look.","tool_calls":[{"name":"read_file","args":{"Path":"\"/work/a.txt\""}}]"#,
        ),
    ]);
    assert!(working);
    assert!(matches!(&c.items[1].kind, ItemKind::Assistant { text } if text == "Let me look."));
    assert!(matches!(&c.items[2].kind, ItemKind::Tools(calls) if !calls[0].finished));
}

#[test]
fn calls_the_model_moved_past_are_closed_and_a_running_step_keeps_its_output_open() {
    let (c, working) = held(&[
        user(0, T0, "Two things."),
        step(
            1,
            "PLANNER_RESPONSE",
            "DONE",
            T1,
            r#","tool_calls":[{"name":"read_file","args":{"Path":"\"/work/a.txt\""}},{"name":"read_file","args":{"Path":"\"/work/b.txt\""}}]"#,
        ),
        done(2, T1, "alpha"),
        step(
            3,
            "PLANNER_RESPONSE",
            "DONE",
            T2,
            r#","thinking":"Now the long one.","tool_calls":[{"name":"run_command","args":{"CommandLine":"\"sleep 60\""}}]"#,
        ),
        step(
            4,
            "GENERIC",
            "RUNNING",
            T2,
            r#","content":"Created At: 2026-11-11T23:00:20+07:00\nTool is running as a background task\n""#,
        ),
    ]);
    assert!(working);
    let calls = calls(&c);
    assert!(
        calls[0].finished && calls[1].finished,
        "closed when the model moved on"
    );
    assert_eq!(calls[1].output, None);
    assert!(!calls[2].finished, "a running step is not a result");
    assert_eq!(
        calls[2].output.as_deref(),
        Some("Tool is running as a background task")
    );
}

#[test]
fn unknown_tools_keep_their_name_and_long_output_is_cut() {
    use crate::transcript::model::ToolKind;
    let long = "x".repeat(5000);
    let (c, _) = held(&[
        user(0, T0, "Go."),
        step(
            1,
            "PLANNER_RESPONSE",
            "DONE",
            T1,
            r#","tool_calls":[{"name":"manage_subagents","args":{"Action":"\"list\""}},{"name":"read_file","args":{"Path":"/plain/path.txt"}}]"#,
        ),
        done(2, T1, "ok"),
        done(3, T1, &long),
    ]);
    let calls = calls(&c);
    assert_eq!(
        (
            calls[0].title.as_str(),
            calls[0].kind,
            calls[0].name.as_str()
        ),
        ("manage_subagents", ToolKind::Other, "manage_subagents")
    );
    assert_eq!(
        calls[1].input, "/plain/path.txt",
        "a value that isn't JSON text is used as it is"
    );
    let out = calls[1].output.as_deref().unwrap();
    assert!(out.chars().count() <= 3000 && out.ends_with('…'));
}

#[test]
fn the_log_view_ends_with_the_note_and_names_the_open_call() {
    let tmp = TempDir::new("agy-elsewhere-steps");
    let raw = [
        user(0, T0, "Summarize the notes."),
        step(1, "PLANNER_RESPONSE", "DONE", T1, READ_AND_LIST),
        done(2, T1, "Showing lines 1-2 of 2\\nfirst\\nsecond"),
    ];
    tmp.write(
        &format!("brain/{SESSION}/.system_generated/logs/transcript.jsonl"),
        &raw.join("\n"),
    );
    let (c, working) = from_log(tmp.root(), SESSION, DELEGATION_NOTE);
    assert!(working);
    let ids: Vec<&str> = c.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "agy-log-0",
            "agy-log-1-thinking",
            "agy-log-1-tools",
            "agy-elsewhere"
        ]
    );
    assert_eq!(c.activity.as_deref(), Some("List the folder"));
    assert!(matches!(
        &c.items.last().unwrap().kind,
        ItemKind::Notice { symbol, .. } if symbol == "lock"
    ));
    let ids = calls(&c).iter().map(|c| c.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids, ["agy-log-1-call-0", "agy-log-1-call-1"]);
}
