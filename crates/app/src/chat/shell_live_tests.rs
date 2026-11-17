use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::transcript::conversation;
use notchling_engine::transcript::model::ItemKind;
use serde_json::json;

use super::data::Tool;
use super::rows::{rows, Row};
use super::shell_live::{
    lines_of, pending, readable, Live, Pending, EVERY_MS, FAILED_MS, STEADY_MS,
};

fn transcript(tag: &str, lines: &[&str]) -> String {
    let dir = std::env::temp_dir().join(format!("nl-shell-live-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.jsonl");
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    path.to_string_lossy().into_owned()
}

fn user(at: &str, text: &str) -> String {
    json!({"type": "user", "timestamp": format!("2026-11-17T16:15:{at}.000Z"),
           "uuid": format!("u-{at}"), "message": {"role": "user", "content": text}})
    .to_string()
}

const LOGIN: &str = "gcloud auth application-default login";

#[test]
fn a_command_whose_output_the_transcript_has_not_written_yet_is_pending() {
    let path = transcript(
        "start",
        &[
            &user("10", "hello"),
            &user("16", &format!("<bash-input>{LOGIN}</bash-input>")),
        ],
    );
    let c = conversation::read(&path);
    let has_output = c
        .items
        .iter()
        .any(|i| matches!(i.kind, ItemKind::CommandOutput { .. }));
    assert!(
        !has_output,
        "the transcript has no output until the command ends"
    );
    assert!(
        !rows(&c)
            .iter()
            .any(|r| matches!(r, Row::CommandOutput { .. })),
        "Chat from the transcript alone shows only the command"
    );
    let want = pending(&c).expect("the last row is the command");
    assert_eq!(want.command, LOGIN);
    assert_eq!(Some(&want.key), c.items.last().map(|i| &i.id));
}

#[test]
fn the_transcripts_output_ends_it() {
    let path = transcript(
        "end",
        &[
            &user("16", &format!("<bash-input>{LOGIN}</bash-input>")),
            &user(
                "45",
                "<bash-stdout>Your browser has been opened</bash-stdout><bash-stderr></bash-stderr>",
            ),
        ],
    );
    assert_eq!(pending(&conversation::read(&path)), None);
}

#[test]
fn what_the_person_typed_is_not_a_command() {
    let path = transcript("typed", &[&user("16", "run the tests")]);
    assert_eq!(pending(&conversation::read(&path)), None);
    assert_eq!(pending(&Default::default()), None);
}

fn want(key: &str) -> Pending {
    Pending {
        key: key.into(),
        command: "ls".into(),
    }
}

fn lines(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|r| r.to_string()).collect()
}

#[test]
fn the_first_read_is_due_at_once_and_one_runs_at_a_time() {
    let mut live = Live::default();
    assert!(!live.due(0), "nothing is pending");
    assert!(!live.sync(Some(&want("a"))), "no text to take away yet");
    assert!(live.due(0));
    live.begin();
    assert!(!live.due(10_000), "a read is in flight");
}

#[test]
fn text_that_arrives_is_shown_and_the_next_read_waits() {
    let mut live = Live::default();
    live.sync(Some(&want("a")));
    live.begin();
    assert!(live.arrived("a", lines(&["link"]), 1_000));
    assert_eq!(
        live.row(Some(&want("a"))),
        Some(Row::ShellLive {
            text: "link".into()
        })
    );
    assert!(!live.due(1_000 + EVERY_MS - 1));
    assert!(live.due(1_000 + EVERY_MS));
}

#[test]
fn unchanged_text_is_not_a_change_and_is_read_less_often() {
    let mut live = Live::default();
    live.sync(Some(&want("a")));
    let mut at = 0;
    live.begin();
    assert!(live.arrived("a", lines(&["x"]), at));
    for _ in 0..2 {
        at += EVERY_MS;
        assert!(live.due(at));
        live.begin();
        assert!(!live.arrived("a", lines(&["x"]), at));
    }
    assert!(!live.due(at + EVERY_MS));
    assert!(live.due(at + STEADY_MS));
    live.begin();
    assert!(live.arrived("a", lines(&["x", "y"]), at + STEADY_MS));
    assert!(
        live.due(at + STEADY_MS + EVERY_MS),
        "new text is read soon again"
    );
}

#[test]
fn an_empty_read_keeps_what_was_shown() {
    let mut live = Live::default();
    live.sync(Some(&want("a")));
    live.begin();
    live.arrived("a", lines(&["x"]), 0);
    live.begin();
    assert!(!live.arrived("a", Vec::new(), 5_000));
    assert!(live.row(Some(&want("a"))).is_some());
}

#[test]
fn the_text_goes_when_the_command_has_its_output_or_the_session_stops_working() {
    let mut live = Live::default();
    live.sync(Some(&want("a")));
    live.begin();
    live.arrived("a", lines(&["x"]), 0);
    assert!(live.sync(None), "the row has to be taken away");
    assert_eq!(live.row(Some(&want("a"))), None);
    assert!(!live.due(0));
    assert!(!live.sync(None));
}

#[test]
fn a_new_command_starts_over_and_a_late_reply_for_the_old_one_is_dropped() {
    let mut live = Live::default();
    live.sync(Some(&want("a")));
    live.begin();
    assert!(!live.sync(Some(&want("b"))));
    assert!(!live.arrived("a", lines(&["old"]), 0));
    assert_eq!(live.row(Some(&want("b"))), None);
    assert!(live.due(0));
}

#[test]
fn a_command_that_prints_nothing_is_given_up_on_after_a_few_reads() {
    let mut live = Live::default();
    live.sync(Some(&want("a")));
    let mut at = 0;
    let mut reads = 0;
    while live.due(at) {
        live.begin();
        live.arrived("a", Vec::new(), at);
        reads += 1;
        at += STEADY_MS;
        assert!(reads <= 10);
    }
    assert_eq!(reads, 6);
    assert!(!live.due(at + 3_600_000));
}

#[test]
fn a_failed_read_waits_longer() {
    let mut live = Live::default();
    live.sync(Some(&want("a")));
    live.begin();
    live.failed("a", 1_000);
    assert!(!live.due(1_000 + FAILED_MS - 1));
    assert!(live.due(1_000 + FAILED_MS));
}

fn agent(target: AgentTarget, command: Option<&str>, id: &str) -> Agent {
    Agent {
        id: id.into(),
        name: "n".into(),
        project: "p".into(),
        cwd: "/w/p".into(),
        group: AgentGroup::Working,
        label: "Working".into(),
        detail: None,
        since_ms: 1,
        target,
        suggested_reply: None,
        transcript_path: None,
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: command.map(String::from),
        resume_session_id: None,
        at_prompt: false,
        node: None,
        worktree_path: None,
    }
}

#[test]
fn only_a_local_claude_job_has_a_screen_to_read() {
    let job = || AgentTarget::Job("abcd1234".into());
    let remote = format!("{}/s1", "0123456789abcdef");
    assert!(readable(&agent(job(), None, "s1"), Tool::Claude));
    assert!(!readable(&agent(job(), None, "s1"), Tool::Codex));
    assert!(!readable(&agent(job(), Some("ls"), "s1"), Tool::Claude));
    assert!(!readable(
        &agent(AgentTarget::Process(7), None, "s1"),
        Tool::Claude
    ));
    assert!(!readable(&agent(job(), None, &remote), Tool::Claude));
}

#[test]
fn the_daemons_lines_are_read_as_strings() {
    assert_eq!(lines_of(&json!({"lines": ["a", "", "b"]})), ["a", "", "b"]);
    assert!(lines_of(&json!({"items": []})).is_empty());
}

#[test]
fn the_link_a_login_prints_is_shown_under_its_row_and_goes_when_the_transcript_has_the_output() {
    let url = "https://accounts.example.invalid/o/oauth2/auth?client_id=1&scope=a%20b";
    let start = transcript(
        "login",
        &[&user("16", &format!("<bash-input>{LOGIN}</bash-input>"))],
    );
    let c = conversation::read(&start);
    let want = pending(&c).unwrap();
    let mut live = Live::default();
    live.sync(Some(&want));
    live.begin();
    live.arrived(
        &want.key,
        lines(&[
            "Your browser has been opened to visit:",
            "",
            &format!("    {url}"),
        ]),
        0,
    );
    let Some(Row::ShellLive { text }) = live.row(pending(&c).as_ref()) else {
        panic!("the live text shows under the command");
    };
    assert!(text.contains(url));

    let done = transcript(
        "login-done",
        &[
            &user("16", &format!("<bash-input>{LOGIN}</bash-input>")),
            &user(
                "45",
                "<bash-stdout>Credentials saved</bash-stdout><bash-stderr></bash-stderr>",
            ),
        ],
    );
    let after = conversation::read(&done);
    assert_eq!(live.row(pending(&after).as_ref()), None);
    assert!(rows(&after)
        .iter()
        .any(|r| matches!(r, Row::CommandOutput { text } if text == "Credentials saved")));
}

#[test]
fn links_in_the_text_are_found_whole_without_the_marks_around_them() {
    use super::markdown::url_links;
    let text = "Visit:\n\n    https://a.example.invalid/o?x=1&y=2_3 (then wait).\nOr http://localhost:8085/.";
    let links = url_links(text);
    let urls: Vec<&str> = links.iter().map(|(_, url)| url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "https://a.example.invalid/o?x=1&y=2_3",
            "http://localhost:8085/"
        ]
    );
    for (range, url) in &links {
        assert_eq!(&text[range.clone()], url);
    }
    assert!(url_links("no links, only xhttps://nope and https:// x").is_empty());
}
