use super::foreign::*;
use super::model::*;
use crate::transcript::model::{
    ChatItem, Conversation, DiffKind, DiffLine, ItemKind, ToolCall, ToolKind,
};
use std::collections::HashMap;

const SINCE: f64 = 1_800_000_000_000.0;

fn row(id: &str, since_ms: f64) -> ForeignRow {
    ForeignRow {
        id: id.into(),
        name: format!("Title {id}"),
        project: "proj".into(),
        cwd: "/tmp/proj".into(),
        since_ms,
        working: false,
        transcript_path: id
            .starts_with("codex:")
            .then(|| "/tmp/rollout-x.jsonl".into()),
    }
}

fn call(id: &str, name: &str, kind: ToolKind, title: &str, input: &str) -> ToolCall {
    ToolCall::new(id, name, kind, title.into(), input.into())
}

fn item(id: &str, kind: ItemKind, date: Option<f64>) -> ChatItem {
    ChatItem {
        id: id.into(),
        kind,
        date,
    }
}

fn diff(kind: DiffKind, text: &str) -> DiffLine {
    DiffLine {
        kind,
        text: text.into(),
        number: None,
    }
}

#[test]
fn parse_row_ids() {
    assert_eq!(parse("codex:abc"), Some((Tool::Codex, "abc".to_string())));
    assert_eq!(parse("agy:123-4"), Some((Tool::Agy, "123-4".to_string())));
    assert_eq!(parse("agy:"), None);
    assert_eq!(
        parse("3f2a9c1e-1111-2222-3333-444444444444"),
        None,
        "a Claude Code session is not foreign"
    );
}

#[test]
fn session_mapping() {
    let running = session(&row("codex:abc", SINCE), true, None).expect("codex");
    assert_eq!(running.tool, "codex");
    assert_eq!(running.state, SessionState::Running);
    assert_eq!(running.control, Control::Job("codex:abc".into()));
    assert_eq!(
        running.transcript_path.as_deref(),
        Some("/tmp/rollout-x.jsonl")
    );
    assert_eq!(running.project, "proj");
    assert_eq!(running.status_updated_ms, SINCE);
    let idle = session(&row("agy:9", SINCE), false, None).expect("agy");
    assert_eq!(idle.tool, "agy");
    assert_eq!(idle.state, SessionState::Done);
    assert!(session(&row("not-foreign", SINCE), false, None).is_none());
    assert_eq!(running.to_wire().tool, "codex");
    let claude = Session::new("c", "n", "/", Control::ViewOnly, SessionState::Done);
    assert_eq!(claude.to_wire().tool, "claude");
}

fn options(list: &[(&str, &str)]) -> Vec<PermissionOption> {
    list.iter()
        .map(|(id, kind)| PermissionOption {
            id: id.to_string(),
            name: id.to_string(),
            kind: kind.to_string(),
        })
        .collect()
}

#[test]
fn a_permission_makes_it_need_input() {
    let permission = AcpPermission {
        rpc_id: "7".into(),
        call: call(
            "t",
            "run_command",
            ToolKind::Shell,
            "Run command",
            "rm -rf build\nsecond",
        ),
        options: options(&[("a1", "allow_once"), ("r1", "reject_once")]),
    };
    let prompt = prompt(&permission, "agy:S1", SINCE);
    assert_eq!(prompt.id, "agy-perm:agy:S1:7");
    assert_eq!(prompt.session_id, "agy:S1");
    assert_eq!(prompt.kind, PromptKind::Permission);
    assert_eq!(prompt.summary, "Run command: rm -rf build");
    assert_eq!(
        parse_prompt(&prompt.id),
        Some(("agy:S1".to_string(), "7".to_string()))
    );
    let waiting = session(&row("agy:S1", SINCE), true, Some(prompt.clone())).expect("agy");
    assert_eq!(waiting.state, SessionState::NeedsInput);
    assert_eq!(waiting.pending, Some(prompt));
    assert_eq!(parse_prompt("something-else"), None);
    assert_eq!(
        parse_prompt("agy-perm:agy:S1:a:b").map(|p| p.1),
        Some("a:b".to_string())
    );
}

#[test]
fn permission_answers() {
    let base = call("t", "x", ToolKind::Other, "x", "");
    let permission = AcpPermission {
        rpc_id: "1".into(),
        call: base.clone(),
        options: options(&[
            ("always", "allow_always"),
            ("once", "allow_once"),
            ("no", "reject_once"),
        ]),
    };
    assert_eq!(
        option(&PromptResponse::Allow, &permission).as_deref(),
        Some("once"),
        "the one-time allow, never the standing one"
    );
    assert_eq!(
        option(&PromptResponse::Deny, &permission).as_deref(),
        Some("no")
    );
    let only_always = AcpPermission {
        rpc_id: "1".into(),
        call: base,
        options: options(&[("always", "allow_always")]),
    };
    assert_eq!(
        option(&PromptResponse::Allow, &only_always).as_deref(),
        Some("always")
    );
    assert_eq!(
        option(&PromptResponse::Deny, &only_always),
        None,
        "no reject option: the turn is cancelled"
    );
}

#[test]
fn visible_hides_closed_old_and_excess() {
    let now = SINCE;
    let fresh = row("codex:a", now - 60_000.0);
    let closed = row("codex:b", now - 120_000.0);
    let reopened = row("codex:c", now - 30_000.0);
    let old = row("agy:old", now - 9.0 * 86_400_000.0);
    let shut = HashMap::from([
        ("codex:b".to_string(), closed.since_ms / 1000.0 + 1.0),
        ("codex:c".to_string(), reopened.since_ms / 1000.0 - 100.0),
    ]);
    let list = visible(
        &[closed, old, fresh, reopened],
        &shut,
        now,
        MAX_AGE_MS,
        PER_TOOL,
    );
    let ids: Vec<_> = list.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["codex:c", "codex:a"], "newest first");
    let many: Vec<_> = (0..5)
        .map(|n| row(&format!("agy:{n}"), now - n as f64 * 1000.0))
        .collect();
    let ids: Vec<_> = visible(&many, &HashMap::new(), now, MAX_AGE_MS, 2)
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(ids, ["agy:0", "agy:1"]);
}

#[test]
fn messages_from_a_conversation() {
    let date = SINCE / 1000.0;
    let mut edit = call(
        "c2",
        "Edit",
        ToolKind::Edit,
        "Edit parser.swift",
        "/tmp/proj/parser.swift",
    );
    edit.diff = vec![
        diff(DiffKind::Hunk, "@@"),
        diff(DiffKind::Remove, "a"),
        diff(DiffKind::Add, "b"),
    ];
    let mut shell = call("c1", "Bash", ToolKind::Shell, "swift test", "swift test");
    shell.output = Some("1 failure".into());
    shell.finished = true;
    let conversation = Conversation {
        items: vec![
            item("u1", ItemKind::user("Fix it"), Some(date)),
            item(
                "th",
                ItemKind::Thinking {
                    seconds: 0.0,
                    text: "hmm".into(),
                },
                None,
            ),
            item("t1", ItemKind::Tools(vec![shell, edit]), None),
            item(
                "a1",
                ItemKind::Assistant {
                    text: "Done".into(),
                },
                None,
            ),
            item(
                "end",
                ItemKind::TurnEnd {
                    seconds: 3.0,
                    model: None,
                },
                None,
            ),
        ],
        ..Conversation::default()
    };
    let list = messages(&conversation, "codex:s", SINCE, false);
    let roles: Vec<_> = list.iter().map(|m| m.role).collect();
    use notchling_protocol::generated::ChatRole;
    assert_eq!(
        roles,
        [ChatRole::User, ChatRole::Tools, ChatRole::Assistant],
        "thinking and turn ends are not sent"
    );
    assert_eq!(
        list[1].tool_summary.as_deref(),
        Some("Ran 1 shell command, edited 1 file")
    );
    let steps = list[1].steps.clone().expect("steps");
    assert_eq!(
        steps.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["Bash", "Edit"]
    );
    assert_eq!(steps[0].output.as_deref(), Some("1 failure"));
    assert_eq!(
        steps[1].diff,
        Some(vec!["-a".to_string(), "+b".to_string()]),
        "hunk headers are dropped"
    );
    assert_eq!(steps[1].path.as_deref(), Some("/tmp/proj/parser.swift"));
    assert_eq!(
        list[2].time, SINCE,
        "an item with no date takes the one before it"
    );
    let again = messages(&conversation, "codex:s", SINCE, false);
    assert_eq!(
        list.iter().map(|m| &m.id).collect::<Vec<_>>(),
        again.iter().map(|m| &m.id).collect::<Vec<_>>()
    );
    let other = messages(&conversation, "codex:t", SINCE, false);
    assert_ne!(list[0].id, other[0].id);
    assert_eq!(list[0].id, stable_uuid("codex:s|u1"));
}

#[test]
fn hold_tail_keeps_a_growing_reply_back() {
    let mut conversation = Conversation {
        items: vec![
            item("u", ItemKind::user("q"), None),
            item(
                "a",
                ItemKind::Assistant {
                    text: "partial".into(),
                },
                None,
            ),
        ],
        ..Conversation::default()
    };
    assert_eq!(messages(&conversation, "agy:s", SINCE, true).len(), 1);
    assert_eq!(messages(&conversation, "agy:s", SINCE, false).len(), 2);
    conversation.items.push(item(
        "e",
        ItemKind::TurnEnd {
            seconds: 1.0,
            model: None,
        },
        None,
    ));
    assert_eq!(
        messages(&conversation, "agy:s", SINCE, true).len(),
        2,
        "once the turn has ended the reply is whole"
    );
}

#[test]
fn enrich_takes_the_last_reply_and_context() {
    let mut conversation = Conversation {
        items: vec![
            item(
                "a1",
                ItemKind::Assistant {
                    text: "  first ".into(),
                },
                None,
            ),
            item(
                "a2",
                ItemKind::Assistant {
                    text: " latest\n".into(),
                },
                None,
            ),
        ],
        ..Conversation::default()
    };
    conversation.facts.context_tokens = 1234;
    conversation.facts.model = Some("gpt-6".into());
    let mut s = session(&row("codex:s", SINCE), false, None).expect("codex");
    enrich(&mut s, &conversation);
    let last = s.last_message.clone().expect("last message");
    assert_eq!(last.text, "latest");
    assert_eq!(last.id, stable_uuid("codex:s|a2"));
    let facts = s.facts.expect("facts");
    assert_eq!(facts.context_tokens, Some(1234));
    assert_eq!(facts.model.as_deref(), Some("gpt-6"));
}

#[test]
fn stable_uuid_matches_the_swift_app() {
    let id = stable_uuid("codex:s|u1");
    assert_eq!(id.len(), 36);
    assert_eq!(id, id.to_uppercase());
    assert_eq!(id.matches('-').count(), 4);
    assert_eq!(stable_uuid(""), "E3B0C442-98FC-1C14-9AFB-F4C8996FB924");
}
