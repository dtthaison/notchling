use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::outward::{cut_steps, is_outward, CutStep};
use crate::transcript::conversation;
use crate::transcript::model::{
    BackgroundShell, ChatItem, Conversation, ItemKind, ToolCall, ToolKind,
};

fn bash(cmd: &str) -> bool {
    is_outward("Bash", &json!({ "command": cmd }))
}

fn tools(names: &[&str], input: &Value, outward: bool) {
    for name in names {
        assert_eq!(
            is_outward(name, input),
            outward,
            "{name} with {input} should be outward: {outward}"
        );
    }
}

fn read_fixture(name: &str) -> Conversation {
    conversation::read(&format!(
        "{}/tests/fixtures/cluster/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
}

fn item(kind: ItemKind) -> ChatItem {
    ChatItem {
        id: "i".into(),
        kind,
        date: None,
    }
}

fn call(name: &str, kind: ToolKind, input: &str, finished: bool) -> ToolCall {
    let mut call = ToolCall::new("t", name, kind, format!("Used {name}"), input.into());
    call.finished = finished;
    call
}

fn conv(items: Vec<ItemKind>) -> Conversation {
    Conversation {
        items: items.into_iter().map(item).collect(),
        ..Conversation::default()
    }
}

fn shell(id: &str, command: &str) -> BackgroundShell {
    BackgroundShell {
        id: id.into(),
        command: command.into(),
        title: format!("Ran {command}"),
        output_path: format!("/tmp/{id}.output"),
        started: None,
    }
}

fn step(tool: &str, summary: &str, outward: bool) -> CutStep {
    CutStep {
        tool: tool.into(),
        summary: summary.into(),
        outward,
    }
}

#[test]
fn cut_git_push_is_outward() {
    let steps = cut_steps(&read_fixture("cut-push.jsonl"));
    assert_eq!(steps, vec![step("Bash", "Push the notes branch", true)]);
}

#[test]
fn git_status_is_not() {
    for cmd in [
        "git status",
        "git log --oneline",
        "git diff HEAD",
        "git fetch origin",
        "git remote -v",
        "ls -la",
        "cargo test --workspace",
    ] {
        assert!(!bash(cmd), "{cmd}");
    }
}

#[test]
fn chained_command_with_push_is_outward() {
    for cmd in [
        "git add -A && git commit -m x && git push origin main",
        "cargo test || git push",
        "echo done; git push",
        "# don't push yet\ngit push",
        "git add -A # it's staged\ngit push",
    ] {
        assert!(bash(cmd), "{cmd}");
    }
}

#[test]
fn a_read_that_names_a_listed_word_is_accepted_noise() {
    for cmd in [
        "echo \"remember to push\"",
        "git commit -m 'git push later'",
        "grep -r publish src",
        "make deploy",
        "echo send-email",
        "echo 'Ship It: DELETE nothing'",
        "man ssh",
        "rsync --list-only src/",
    ] {
        assert!(bash(cmd), "{cmd}");
    }
    for cmd in [
        "echo pushing",
        "echo ssh-keygen",
        "echo repost",
        "echo prepublish-docs",
    ] {
        assert!(!bash(cmd), "{cmd}");
    }
}

#[test]
fn the_parser_and_the_backstop_are_ored() {
    assert!(bash("git -C /w/repo push"));
    assert!(bash("echo 'a'; echo \"unterminated"));
    assert!(bash("npx vercel --prod"));
    assert!(!bash("echo hello # later"));
    assert!(!bash("echo hello"));
}

#[test]
fn long_flag_clusters_are_linear() {
    let cluster = format!("curl -{} https://example.test", "s".repeat(60_000));
    let started = Instant::now();
    assert!(!bash(&cluster));
    assert!(started.elapsed() < Duration::from_secs(1));
    let started = Instant::now();
    assert!(bash(&format!("curl -{} -d x", "s".repeat(60_000))));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn a_command_over_64_kb_is_outward() {
    let started = Instant::now();
    assert!(bash(&format!("curl -{}", "s".repeat(200_000))));
    assert!(bash(&format!("echo {}", "a".repeat(70_000))));
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(!bash(&format!("echo {}", "a".repeat(60_000))));
}

#[test]
fn mcp_send_is_outward() {
    let input = json!({});
    tools(
        &[
            "mcp__slack__send_message",
            "mcp__github__create_issue",
            "mcp__github__update_issue",
            "mcp__jira__delete_ticket",
            "mcp__blog__publish",
            "mcp__vercel__deploy",
            "mcp__github__merge_pull_request",
            "mcp__github__push_files",
            "mcp__notes__write_note",
            "mcp__mail__reply",
            "mcp__chat__post_message",
            "mcp__Slack__SEND_MESSAGE",
            "mcp__postgres__query",
            "mcp__github__searchfoo",
            "mcp__lonely",
            "mcp__github__getaway",
            "mcp__srv__read__write",
            "mcp__srv__GETTER",
        ],
        &input,
        true,
    );
    tools(
        &[
            "mcp__github__list_issues",
            "mcp__github__get_file_contents",
            "mcp__github__get_issue",
            "mcp__github__getIssue",
            "mcp__context7__get-library-docs",
            "mcp__context7__context7_get_docs",
            "mcp__github__github_list_issues",
            "mcp__slack__search",
            "mcp__docs__read",
            "mcp__docs__fetch_page",
            "mcp__x__find_things",
            "mcp__x__VIEW_Thing",
            "mcp__x__describe",
            "mcp__postgres__read_rows",
            "mcp__push__list_things",
        ],
        &input,
        false,
    );
}

#[test]
fn artifact_publish_is_outward() {
    assert!(is_outward(
        "Artifact",
        &json!({"action": "publish", "file_path": "/tmp/a.html"})
    ));
    assert!(is_outward("Artifact", &json!({"file_path": "/tmp/a.html"})));
    assert!(is_outward("Artifact", &json!({})));
    assert!(is_outward("Artifact", &json!({"mode": "delete"})));
    assert!(is_outward("Artifact", &Value::Null));
    for action in ["read", "get", "list", "view", "READ"] {
        assert!(!is_outward("Artifact", &json!({ "action": action })));
        assert!(!is_outward("Artifact", &json!({ "mode": action })));
    }
}

#[test]
fn artifact_data_and_comments_follow_the_artifact_rule() {
    for tool in ["ArtifactData", "ArtifactComments"] {
        assert!(is_outward(tool, &json!({"action": "set"})));
        assert!(is_outward(tool, &json!({"action": "reply"})));
        assert!(is_outward(tool, &json!({})));
        assert!(is_outward(tool, &Value::Null));
        assert!(!is_outward(tool, &json!({"action": "list"})));
        assert!(!is_outward(tool, &json!({"action": "get"})));
    }
    assert!(!is_outward("ArtifactData", &json!({"action": "query"})));
}

#[test]
fn monitor_follows_the_bash_check() {
    assert!(is_outward("Monitor", &json!({"command": "git push"})));
    assert!(is_outward(
        "Monitor",
        &json!({"command": "tail -f x && git push"})
    ));
    assert!(!is_outward("Monitor", &json!({"command": "tail -f log"})));
    assert!(is_outward("Monitor", &json!({})));
}

#[test]
fn agents_are_outward_in_is_outward_itself() {
    tools(&["Agent", "Task"], &json!({"prompt": "review"}), true);
    tools(&["Agent", "Task"], &json!({}), true);
}

#[test]
fn read_and_edit_are_not() {
    tools(
        &[
            "Read",
            "Edit",
            "Write",
            "MultiEdit",
            "Grep",
            "Glob",
            "WebFetch",
            "WebSearch",
            "TodoWrite",
            "NotebookEdit",
        ],
        &json!({"file_path": "/w/a.rs"}),
        false,
    );
    tools(
        &["Read", "Edit", "Write", "Grep"],
        &json!({"command": "git push"}),
        false,
    );
    assert!(!is_outward("Bash", &json!({})));
    assert!(!is_outward("Bash", &json!({"command": ""})));
    assert!(!is_outward("Bash", &json!({"command": 3})));
}

#[test]
fn cut_artifact_with_unparseable_input_is_outward() {
    let cut = "{\n  \"action\" : \"read\",\n  \"content\" : \"aaaa\n…";
    let c = conv(vec![
        ItemKind::user("go"),
        ItemKind::Tools(vec![call("Artifact", ToolKind::Other, cut, false)]),
    ]);
    let steps = cut_steps(&c);
    assert_eq!(steps.len(), 1);
    assert!(steps[0].outward);
    let read = "{\n  \"action\" : \"read\",\n  \"url\" : \"https://example.test/a\"\n}";
    let c = conv(vec![
        ItemKind::user("go"),
        ItemKind::Tools(vec![call("Artifact", ToolKind::Other, read, false)]),
    ]);
    assert!(!cut_steps(&c)[0].outward);
}

#[test]
fn cut_steps_reads_unanswered_calls() {
    let c = read_fixture("cut-push.jsonl");
    let last_tools = c
        .items
        .iter()
        .rev()
        .find_map(|i| match &i.kind {
            ItemKind::Tools(calls) => Some(calls),
            _ => None,
        })
        .expect("a tools item");
    let cut = last_tools.last().expect("a call");
    assert!(!cut.finished);
    assert_eq!(cut.input, "git push origin notes");
    assert_eq!(last_tools.iter().filter(|c| !c.finished).count(), 1);
}

#[test]
fn cut_steps_only_looks_at_the_last_turn() {
    let c = conv(vec![
        ItemKind::user("one"),
        ItemKind::Tools(vec![call("Bash", ToolKind::Shell, "git push", false)]),
        ItemKind::user("two"),
        ItemKind::Tools(vec![
            call("Bash", ToolKind::Shell, "git push", true),
            call("Read", ToolKind::Read, "/w/a.rs", false),
            call("Bash", ToolKind::Shell, "ls", false),
        ]),
    ]);
    let steps = cut_steps(&c);
    let tools: Vec<(&str, bool)> = steps.iter().map(|s| (s.tool.as_str(), s.outward)).collect();
    assert_eq!(tools, vec![("Read", false), ("Bash", false)]);
}

#[test]
fn no_cut_steps_for_a_finished_or_empty_turn() {
    assert!(cut_steps(&Conversation::default()).is_empty());
    let c = conv(vec![
        ItemKind::user("go"),
        ItemKind::Tools(vec![call("Bash", ToolKind::Shell, "git push", true)]),
    ]);
    assert!(cut_steps(&c).is_empty());
    let c = conv(vec![
        ItemKind::Tools(vec![call("Bash", ToolKind::Shell, "git push", false)]),
        ItemKind::user("go"),
    ]);
    assert!(cut_steps(&c).is_empty());
}

#[test]
fn cut_steps_without_a_user_item_reads_every_item() {
    let c = conv(vec![ItemKind::Tools(vec![call(
        "Bash",
        ToolKind::Shell,
        "git push",
        false,
    )])]);
    assert_eq!(cut_steps(&c).len(), 1);
}

#[test]
fn a_shell_call_is_checked_as_a_shell_whatever_its_name() {
    let c = conv(vec![
        ItemKind::user("go"),
        ItemKind::Tools(vec![call("PowerShell", ToolKind::Shell, "git push", false)]),
    ]);
    assert!(cut_steps(&c)[0].outward);
}

#[test]
fn cut_mcp_call_takes_its_name_from_the_call() {
    let c = conv(vec![
        ItemKind::user("go"),
        ItemKind::Tools(vec![call(
            "mcp__slack__send_message",
            ToolKind::Other,
            "{\n  \"text\" : \"hi\"\n}",
            false,
        )]),
    ]);
    assert_eq!(
        cut_steps(&c),
        vec![step(
            "mcp__slack__send_message",
            "Used mcp__slack__send_message",
            true
        )]
    );
}

#[test]
fn cut_foreground_agent_is_outward() {
    for (name, input) in [("Agent", "do the push"), ("Task", "review")] {
        let c = conv(vec![
            ItemKind::user("go"),
            ItemKind::Tools(vec![call(name, ToolKind::Agent, input, false)]),
        ]);
        let steps = cut_steps(&c);
        assert_eq!(steps.len(), 1);
        assert!(steps[0].outward);
        assert_eq!(steps[0].tool, name);
    }
}

#[test]
fn running_background_shells_are_cut_steps() {
    let mut c = conv(vec![ItemKind::user("go")]);
    c.shells = vec![
        shell("b1", "git push origin main"),
        shell("b2", "sleep 600"),
        shell("b3", "git push origin done"),
    ];
    c.finished_agents = HashMap::from([("b3".to_string(), "completed".to_string())]);
    assert_eq!(
        cut_steps(&c),
        vec![
            step("Bash", "Ran git push origin main", true),
            step("Bash", "Ran sleep 600", false),
        ]
    );
}

#[test]
fn a_shell_stopped_by_taskstop_or_killshell_is_not_running() {
    for (name, input) in [
        ("TaskStop", "{\n  \"task_id\" : \"b1\"\n}"),
        ("KillShell", "{\n  \"shell_id\" : \"b1\"\n}"),
    ] {
        let mut c = conv(vec![
            ItemKind::user("go"),
            ItemKind::Tools(vec![call(name, ToolKind::Other, input, true)]),
        ]);
        c.shells = vec![shell("b1", "git push"), shell("b2", "git push")];
        assert_eq!(
            cut_steps(&c),
            vec![step("Bash", "Ran git push", true)],
            "{name}"
        );
        let mut failed = call(name, ToolKind::Other, input, true);
        failed.is_error = true;
        let mut c = conv(vec![ItemKind::user("go"), ItemKind::Tools(vec![failed])]);
        c.shells = vec![shell("b1", "git push")];
        assert_eq!(cut_steps(&c).len(), 1, "{name} failed");
    }
}

#[test]
fn background_agents_without_a_notification_are_outward_cut_steps() {
    let mut launched = call("Agent", ToolKind::Agent, "push it", true);
    launched.id = "t-bg".into();
    launched.output = Some("Async agent launched successfully.\nagentId: abcdef123456".into());
    let mut foreground = call("Agent", ToolKind::Agent, "review", true);
    foreground.id = "t-fg".into();
    foreground.output = Some("done".into());
    let mut c = conv(vec![
        ItemKind::user("go"),
        ItemKind::Tools(vec![launched.clone(), foreground]),
    ]);
    c.finished_agents = HashMap::from([("t-fg".to_string(), "completed".to_string())]);
    let steps = cut_steps(&c);
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].tool, "Agent");
    assert!(steps[0].outward);
    c.finished_agents
        .insert("abcdef123456".to_string(), "completed".to_string());
    assert!(cut_steps(&c).is_empty());
}

#[test]
fn a_failed_agent_call_is_not_running_work() {
    let mut failed = call("Agent", ToolKind::Agent, "push it", true);
    failed.is_error = true;
    let c = conv(vec![ItemKind::user("go"), ItemKind::Tools(vec![failed])]);
    assert!(cut_steps(&c).is_empty());
}

#[test]
fn a_background_agent_from_an_earlier_turn_still_counts() {
    let mut launched = call("Agent", ToolKind::Agent, "deploy", true);
    launched.output = Some("Async agent launched.\nagentId: abcdef123456".into());
    let c = conv(vec![
        ItemKind::user("one"),
        ItemKind::Tools(vec![launched]),
        ItemKind::user("two"),
        ItemKind::Tools(vec![call("Read", ToolKind::Read, "/w/a", true)]),
    ]);
    assert_eq!(cut_steps(&c), vec![step("Agent", "Used Agent", true)]);
}

#[test]
fn background_work_in_a_real_transcript() {
    assert_eq!(
        cut_steps(&read_fixture("cut-background.jsonl")),
        vec![
            step("Bash", "Push main in the background", true),
            step("Agent", "Agent: Deploy the site", true),
        ]
    );
}
