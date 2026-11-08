//! Ports of TranscriptTests, ShellCommandTests, QueuedMessageTests, ModelConfirmationTests and the
//! incremental conversation tests. They read the Swift app's own fixtures.

mod common;

use common::*;
use notchling_engine::transcript::conversation::{self, model_id_in_confirmation};
use notchling_engine::transcript::model::*;
use notchling_engine::transcript::text::collapse_pastes;

fn user(text: &str) -> ItemKind {
    ItemKind::User {
        text: text.into(),
        images: 0,
    }
}

fn read_lines_as_file(lines: &[&str]) -> Conversation {
    let scratch = Scratch::new("queued");
    let path = scratch.write("q.jsonl", &(lines.join("\n") + "\n"));
    conversation::read(&s(&path))
}

#[test]
fn conversation_items() {
    let c = conversation::read(&s(&notchling_fixture("conversation.jsonl")));
    let kinds: Vec<&ItemKind> = c.items.iter().map(|i| &i.kind).collect();
    assert_eq!(kinds.len(), 8, "items: {kinds:?}");

    assert_eq!(*kinds[0], user("Fix the bug"));
    assert_eq!(
        *kinds[1],
        ItemKind::Thinking {
            seconds: 3.0,
            text: "Check the folder first.".into()
        }
    );
    assert_eq!(
        *kinds[2],
        ItemKind::Assistant {
            text: "Looking.".into()
        }
    );

    let ItemKind::Tools(calls) = kinds[3] else {
        panic!("expected tools, got {:?}", kinds[3])
    };
    assert_eq!(
        calls.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["Bash", "Edit"]
    );
    assert_eq!(calls[0].title, "List files");
    assert_eq!(calls[0].kind, ToolKind::Shell);
    assert!(calls[0].finished);
    assert_eq!(calls[0].output.as_deref(), Some("a.txt\nb.txt"));
    assert_eq!(calls[1].title, "Edited a.txt");
    assert_eq!(
        calls[1].output, None,
        "edits show their diff, not the tool's text"
    );
    assert_eq!(
        calls[1].diff.iter().map(|d| d.kind).collect::<Vec<_>>(),
        [
            DiffKind::Hunk,
            DiffKind::Context,
            DiffKind::Remove,
            DiffKind::Add
        ]
    );
    assert_eq!(calls[1].added(), 1);
    assert_eq!(calls[1].removed(), 1);

    assert_eq!(
        *kinds[4],
        ItemKind::Assistant {
            text: "Done.".into()
        }
    );
    assert_eq!(
        *kinds[5],
        ItemKind::TurnEnd {
            seconds: 12.0,
            model: Some("claude-opus-5-5".into())
        }
    );
    assert_eq!(
        *kinds[6],
        user("/cost"),
        "a local_command line shows as typed"
    );
    assert_eq!(
        *kinds[7],
        ItemKind::Notice {
            text: "Interrupted".into(),
            symbol: "stop.circle".into()
        }
    );
}

#[test]
fn conversation_facts() {
    let c = conversation::read(&s(&notchling_fixture("conversation.jsonl")));
    assert_eq!(c.facts.branch.as_deref(), Some("main"));
    assert_eq!(c.facts.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(c.facts.turns, 1);
    assert_eq!(c.facts.commands, 1);
    assert_eq!(c.facts.files, ["/repo/a.txt"]);
    assert_eq!(
        c.facts.input_tokens,
        100 + 5 + 1,
        "a message's usage counts once though its lines repeat it"
    );
    assert_eq!(c.activity, None, "nothing is still running");
}

#[test]
fn missing_file_is_empty() {
    assert_eq!(
        conversation::read("/nonexistent/x.jsonl"),
        Conversation::default()
    );
}

#[test]
fn one_compaction_shows_once() {
    // Claude Code writes a compact_boundary line and then the summary; older builds only the summary.
    let compacted = ItemKind::Notice {
        text: "Conversation compacted".into(),
        symbol: "rectangle.compress.vertical".into(),
    };
    let c = conversation::read(&s(&notchling_fixture("compact.jsonl")));
    let kinds: Vec<ItemKind> = c.items.into_iter().map(|i| i.kind).collect();
    assert_eq!(
        kinds,
        [user("Before"), compacted.clone(), user("After"), compacted]
    );
}

#[test]
fn context_is_the_latest_request() {
    // The last message's input (with cache) and output, not the session's total.
    let c = conversation::read(&s(&notchling_fixture("conversation.jsonl")));
    assert_eq!(c.facts.context_tokens, 1 + 3);
    // Right after a compaction, what it left.
    let c = conversation::read(&s(&notchling_fixture("compact.jsonl")));
    assert_eq!(c.facts.context_tokens, 28610);
}

#[test]
fn collapse_pastes_shortens_pasted_text() {
    assert_eq!(
        collapse_pastes("see <pasted_content id=\"1\">\na\nb\nc\n</pasted_content> ok"),
        "see [Pasted text +3 lines] ok"
    );
    assert_eq!(
        collapse_pastes("<pasted_content>one</pasted_content>"),
        "[Pasted text]"
    );
    assert_eq!(collapse_pastes("plain"), "plain");
}

#[test]
fn shell_commands_and_output() {
    let c = conversation::read(&s(&notchling_fixture("bash-mode.jsonl")));
    let kinds: Vec<ItemKind> = c.items.into_iter().map(|i| i.kind).collect();
    let out = |t: &str| ItemKind::CommandOutput { text: t.into() };
    assert_eq!(
        kinds,
        [
            user("! git branch"),
            out("  feat/x\n* main"), // indentation kept
            user("! ped"),
            out("(eval):1: command not found: ped"),
            user("! true"),
            // Printed nothing: no output row.
            out("green"),
        ]
    );
}

const USER: &str = r#"{"type":"user","message":{"role":"user","content":"Write a story."},"timestamp":"2026-11-04T04:26:51.000Z"}"#;

#[test]
fn effort_from_the_latest_reply() {
    let c = read_lines_as_file(&[
        USER,
        r#"{"type":"assistant","effort":"medium","message":{"id":"m1","model":"claude-opus-5-5","role":"assistant","content":[{"type":"text","text":"Once"}]},"timestamp":"2026-11-04T04:26:52.000Z"}"#,
        r#"{"type":"assistant","effort":"xhigh","message":{"id":"m2","model":"claude-opus-5-5","role":"assistant","content":[{"type":"text","text":"upon"}]},"timestamp":"2026-11-04T04:26:53.000Z"}"#,
    ]);
    assert_eq!(c.facts.effort.as_deref(), Some("xhigh"));
}

#[test]
fn pop_all_takes_the_queue_back() {
    let c = read_lines_as_file(&[
        USER,
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-05T09:58:08.490Z","content":"Then say ONE"}"#,
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-05T09:58:11.666Z","content":"And TWO"}"#,
        r#"{"type":"queue-operation","operation":"popAll","timestamp":"2026-11-05T09:58:13.059Z","content":"Then say ONE"}"#,
        r#"{"type":"queue-operation","operation":"popAll","timestamp":"2026-11-05T09:58:13.060Z","content":"And TWO"}"#,
    ]);
    assert!(c.queued.is_empty());
}

#[test]
fn enqueued_is_queued() {
    let c = read_lines_as_file(&[
        USER,
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-04T04:27:12.000Z","content":"After the story, say fifth."}"#,
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-04T04:27:13.000Z","content":"<task-notification> <task-id>b1</task-id></task-notification>"}"#,
    ]);
    assert_eq!(c.queued, ["After the story, say fifth."]);
    assert_eq!(c.items.len(), 1);
}

#[test]
fn absorbed_mid_turn_shows_as_your_message() {
    let c = read_lines_as_file(&[
        USER,
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-04T04:14:02.751Z","content":"support /model"}"#,
        r#"{"type":"queue-operation","operation":"remove","timestamp":"2026-11-04T04:15:26.108Z","content":"support /model","reason":"absorbed_mid_turn"}"#,
        r#"{"type":"attachment","attachment":{"type":"queued_command","prompt":"support /model","commandMode":"prompt","origin":{"kind":"human"},"humanTurn":true},"timestamp":"2026-11-04T04:14:02.641Z"}"#,
        r#"{"type":"attachment","attachment":{"type":"queued_command","prompt":"<task-notification>done</task-notification>","commandMode":"task-notification"},"timestamp":"2026-11-04T04:14:03.000Z"}"#,
    ]);
    assert!(c.queued.is_empty());
    assert_eq!(
        c.items.last().map(|i| i.kind.clone()),
        Some(user("support /model"))
    );
    assert_eq!(c.items.len(), 2);
}

#[test]
fn dequeued_leaves_the_queue() {
    let c = read_lines_as_file(&[
        USER,
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-11-04T04:27:12.000Z","content":"fifth"}"#,
        r#"{"type":"queue-operation","operation":"dequeue","timestamp":"2026-11-04T04:27:32.000Z"}"#,
        r#"{"type":"user","message":{"role":"user","content":"fifth"},"timestamp":"2026-11-04T04:27:32.100Z"}"#,
    ]);
    assert!(c.queued.is_empty());
    assert_eq!(c.items.last().map(|i| i.kind.clone()), Some(user("fifth")));
}

#[test]
fn message_starting_with_a_paste_shows() {
    let c = read_lines_as_file(&[
        r#"{"type":"user","message":{"role":"user","content":"<pasted_content id=\"a1\">x\ny</pasted_content> fix this"},"timestamp":"2026-11-04T04:00:00.000Z"}"#,
    ]);
    let kinds: Vec<ItemKind> = c.items.into_iter().map(|i| i.kind).collect();
    assert_eq!(kinds, [user("[Pasted text +2 lines] fix this")]);
}

#[test]
fn model_id_from_confirmation() {
    let id = model_id_in_confirmation;
    assert_eq!(
        id("Set model to `Sonnet 5.5` and saved as your default for new sessions").as_deref(),
        Some("claude-sonnet-5-5")
    );
    assert_eq!(
        id("<local-command-stdout>Kept model as `Opus 5.5`</local-command-stdout>").as_deref(),
        Some("claude-opus-5-5")
    );
    assert_eq!(id("Set model to `Default (recommended)`"), None);
    assert_eq!(id("Something else"), None);
}

#[test]
fn model_confirmation_sets_the_header_model_at_once() {
    let c = read_lines_as_file(&[
        r#"{"type":"system","subtype":"local_command","timestamp":"2026-11-04T04:00:00.000Z","content":"<local-command-stdout>Set model to `Sonnet 5.5`</local-command-stdout>"}"#,
    ]);
    assert_eq!(c.facts.model.as_deref(), Some("claude-sonnet-5-5"));
}

/// The cursor must always agree with reading the whole file fresh: a file appended to line by line
/// (half a line first, as when Claude Code is mid-write) reads the same as a fresh copy.
#[test]
fn conversation_append_matches_a_fresh_read() {
    let scratch = Scratch::new("incr");
    let path = scratch.write("chat.jsonl", "");
    for line in lines_of(&notchling_fixture("conversation.jsonl")) {
        let mid = line
            .char_indices()
            .nth(line.chars().count() / 2)
            .map(|(i, _)| i)
            .unwrap_or(0);
        scratch.append("chat.jsonl", &line[..mid]);
        let _ = conversation::read(&s(&path));
        scratch.append("chat.jsonl", &format!("{}\n", &line[mid..]));
        let fresh = scratch.path(&format!("fresh-{}.jsonl", line.len()));
        std::fs::copy(&path, &fresh).expect("copy");
        assert_eq!(
            conversation::read(&s(&path)),
            conversation::read(&s(&fresh)),
            "after: {}",
            &line[..line.len().min(60)]
        );
    }
    assert_eq!(conversation::read(&s(&path)).items.len(), 8);
}

#[test]
fn long_sessions_keep_the_last_500_items() {
    let scratch = Scratch::new("long");
    let path = scratch.write("long.jsonl", "");
    let mut text = String::new();
    for i in 0..1300 {
        text += &format!(r#"{{"type":"user","message":{{"content":"prompt {i}"}}}}"#);
        text += "\n";
        text += &format!(
            r#"{{"type":"assistant","message":{{"id":"m{i}","content":[{{"type":"tool_use","id":"t{i}","name":"Bash","input":{{"command":"ls"}}}}]}}}}"#
        );
        text += "\n";
        text += &format!(
            r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"t{i}","content":"ok {i}"}}]}}}}"#
        );
        text += "\n";
        if i % 100 == 99 {
            scratch.append("long.jsonl", &text);
            text.clear();
            let _ = conversation::read(&s(&path));
        }
    }
    let c = conversation::read(&s(&path));
    assert_eq!(c.items.len(), 500);
    let ItemKind::Tools(calls) = &c.items.last().expect("last").kind else {
        panic!("last item is not tools")
    };
    assert_eq!(
        calls[0].output.as_deref(),
        Some("ok 1299"),
        "results still land on their calls after older items were dropped"
    );
    assert_eq!(c.facts.commands, 1300);
}

#[test]
fn background_shell_launch_lands_in_the_conversation() {
    let c = read_lines_as_file(&[
        r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"sleep 9","description":"Wait"}}]},"timestamp":"2026-11-04T04:00:00.000Z"}"#,
        r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"Command running in background with ID: bj5rd6y90. Output is being written to: /private/tmp/x/bj5rd6y90.output. You will be notified."}]},"timestamp":"2026-11-04T04:00:01.000Z"}"#,
    ]);
    assert_eq!(c.shells.len(), 1);
    assert_eq!(c.shells[0].id, "bj5rd6y90");
    assert_eq!(c.shells[0].command, "sleep 9");
    assert_eq!(c.shells[0].title, "Wait");
    assert_eq!(c.shells[0].output_path, "/private/tmp/x/bj5rd6y90.output");
    assert!(c.shells[0].started.is_some());
}

#[test]
fn tasks_follow_create_and_update_results() {
    let c = read_lines_as_file(&[
        r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"TaskCreate","input":{"subject":"Port it"}}]}}"#,
        r#"{"type":"user","toolUseResult":{"task":{"id":"1","subject":"Port it"}},"message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
        r#"{"type":"assistant","message":{"id":"m2","content":[{"type":"tool_use","id":"t2","name":"TaskUpdate","input":{"taskId":"1","status":"in_progress"}}]}}"#,
        r#"{"type":"user","toolUseResult":{"taskId":"1","statusChange":{"from":"pending","to":"in_progress"}},"message":{"content":[{"type":"tool_result","tool_use_id":"t2","content":"ok"}]}}"#,
    ]);
    assert_eq!(
        c.tasks,
        [TaskEntry {
            id: "1".into(),
            subject: "Port it".into(),
            status: TaskStatus::InProgress
        }]
    );
}

#[test]
fn todo_write_replaces_the_task_list() {
    let c = read_lines_as_file(&[
        r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"TodoWrite","input":{"todos":[{"content":"a","status":"completed"},{"content":"b","status":"in_progress"}]}}]}}"#,
    ]);
    assert_eq!(c.tasks.len(), 2);
    assert_eq!(c.tasks[0].status, TaskStatus::Completed);
    assert_eq!(c.tasks[1].subject, "b");
    assert_eq!(c.activity.as_deref(), Some("Updated the task list"));
}

#[test]
fn write_results_show_created_files_as_added_lines() {
    let c = read_lines_as_file(&[
        r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/p/new.md","content":"one\ntwo"}}]}}"#,
        r#"{"type":"user","toolUseResult":{"type":"create","content":"one\ntwo"},"message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"created"}]}}"#,
    ]);
    let ItemKind::Tools(calls) = &c.items[0].kind else {
        panic!("tools")
    };
    assert_eq!(calls[0].title, "Created new.md");
    assert_eq!(calls[0].output, None);
    assert_eq!(
        calls[0].diff,
        [
            DiffLine {
                kind: DiffKind::Add,
                text: "one".into(),
                number: Some(1)
            },
            DiffLine {
                kind: DiffKind::Add,
                text: "two".into(),
                number: Some(2)
            },
        ]
    );
}
