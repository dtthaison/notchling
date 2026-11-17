use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::transcript::conversation;
use notchling_engine::transcript::model::ItemKind;
use serde_json::json;

use super::data::Tool;
use super::rows::{rows, Row};
use super::shell_live::{
    lines_of, pending, readable, skipped, Live, Pending, EVERY_MS, FAILED_MS, GIVE_UP_AFTER,
    STEADY_MS,
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
    let want = pending("s1", &c).expect("the last row is the command");
    assert_eq!(want.command, LOGIN);
    assert_eq!(want.session, "s1");
    assert_eq!(Some(&want.item), c.items.last().map(|i| &i.id));
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
    assert_eq!(pending("s1", &conversation::read(&path)), None);
}

#[test]
fn what_the_person_typed_is_not_a_command() {
    let path = transcript("typed", &[&user("16", "run the tests")]);
    assert_eq!(pending("s1", &conversation::read(&path)), None);
    assert_eq!(pending("s1", &Default::default()), None);
}

fn want(item: &str) -> Pending {
    Pending {
        session: "s1".into(),
        item: item.into(),
        command: "ls".into(),
    }
}

fn elsewhere(session: &str, item: &str) -> Pending {
    Pending {
        session: session.into(),
        ..want(item)
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
    assert!(live.arrived(&want("a"), lines(&["link"]), 1_000));
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
    assert!(live.arrived(&want("a"), lines(&["x"]), at));
    for _ in 0..2 {
        at += EVERY_MS;
        assert!(live.due(at));
        live.begin();
        assert!(!live.arrived(&want("a"), lines(&["x"]), at));
    }
    assert!(!live.due(at + EVERY_MS));
    assert!(live.due(at + STEADY_MS));
    live.begin();
    assert!(live.arrived(&want("a"), lines(&["x", "y"]), at + STEADY_MS));
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
    live.arrived(&want("a"), lines(&["x"]), 0);
    live.begin();
    assert!(!live.arrived(&want("a"), Vec::new(), 5_000));
    assert!(live.row(Some(&want("a"))).is_some());
}

#[test]
fn the_text_goes_when_the_command_has_its_output_or_the_session_stops_working() {
    let mut live = Live::default();
    live.sync(Some(&want("a")));
    live.begin();
    live.arrived(&want("a"), lines(&["x"]), 0);
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
    assert!(!live.arrived(&want("a"), lines(&["old"]), 0));
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
        live.arrived(&want("a"), Vec::new(), at);
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
    live.failed(&want("a"), 1_000);
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
    let want = pending("s1", &c).unwrap();
    let mut live = Live::default();
    live.sync(Some(&want));
    live.begin();
    live.arrived(
        &want,
        lines(&[
            "Your browser has been opened to visit:",
            "",
            &format!("    {url}"),
        ]),
        0,
    );
    let Some(Row::ShellLive { text }) = live.row(pending("s1", &c).as_ref()) else {
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
    assert_eq!(live.row(pending("s1", &after).as_ref()), None);
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

#[test]
fn the_same_row_and_command_in_two_sessions_are_two_different_commands() {
    let path = transcript(
        "twice",
        &[&user("16", &format!("<bash-input>{LOGIN}</bash-input>"))],
    );
    let c = conversation::read(&path);
    let (a, b) = (pending("s1", &c).unwrap(), pending("s2", &c).unwrap());
    assert_eq!(a.item, b.item);
    assert_ne!(a, b);
    let mut live = Live::default();
    live.sync(Some(&a));
    live.begin();
    live.arrived(&a, lines(&["link of s1"]), 0);
    assert!(live.row(Some(&a)).is_some());
    assert_eq!(live.row(Some(&b)), None, "never under another session");
    assert!(
        live.sync(Some(&b)),
        "the first session's text is taken away"
    );
    assert_eq!(live.row(Some(&b)), None);
}

#[test]
fn a_late_reply_for_another_session_is_dropped() {
    let (a, b) = (elsewhere("s1", "i3"), elsewhere("s2", "i3"));
    let mut live = Live::default();
    live.sync(Some(&a));
    live.begin();
    live.sync(Some(&b));
    assert!(!live.arrived(&a, lines(&["link of s1"]), 0));
    assert_eq!(live.row(Some(&b)), None);
    assert!(live.due(0), "the other session starts its own read");
}

#[test]
fn going_away_and_coming_back_never_runs_two_reads_of_one_job() {
    let a = elsewhere("s1", "i3");
    let mut live = Live::default();
    live.sync(Some(&a));
    live.begin();
    live.sync(None);
    live.sync(Some(&a));
    assert!(!live.due(1_000_000), "the first read is still in flight");
    live.reset();
    assert!(!live.due(1_000_000), "a new selection does not forget it");
    live.sync(Some(&a));
    assert!(!live.due(1_000_000), "still in flight after the new sync");
    assert!(live.arrived(&a, lines(&["link"]), 5_000));
    assert!(live.row(Some(&a)).is_some());
    assert!(live.due(5_000 + EVERY_MS));
}

#[test]
fn a_selection_starts_the_give_up_count_over() {
    let a = want("a");
    let mut live = Live::default();
    live.sync(Some(&a));
    for n in 0..GIVE_UP_AFTER {
        assert!(live.due(n as i64 * STEADY_MS), "read {n}");
        live.begin();
        live.arrived(&a, Vec::new(), n as i64 * STEADY_MS);
    }
    assert!(!live.due(1_000_000), "given up");
    live.reset();
    live.sync(Some(&a));
    assert!(live.due(0), "a new selection reads again");
}

#[test]
fn a_read_the_daemon_skipped_is_not_a_miss() {
    assert!(skipped(&json!({"lines": [], "busy": true})));
    assert!(!skipped(&json!({"lines": []})));
    let a = want("a");
    let mut live = Live::default();
    live.sync(Some(&a));
    let mut at = 0;
    for _ in 0..(GIVE_UP_AFTER * 3) {
        assert!(live.due(at));
        live.begin();
        live.skipped(&a, at);
        assert!(!live.due(at + EVERY_MS - 1));
        at += EVERY_MS;
    }
    assert!(live.due(at), "still reading after many skips");
}
