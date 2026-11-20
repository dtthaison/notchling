use super::event::{Event, Kind};
use super::rules::{
    bash_rules, read_tool, spawn_rules, tags, Action, Limits, Rule, Tags, Would, READ_BYTES,
};
use serde_json::{json, Value};
use std::path::Path;

fn call(tool: &str, input: Value) -> Event {
    Event {
        kind: Kind::Pre,
        session: "s1".into(),
        agent: None,
        tool: tool.into(),
        tool_use_id: Some("toolu_1".into()),
        cwd: Some("/work".into()),
        input,
    }
}

fn spawn(description: &str, model: Option<&str>, kind: Option<&str>) -> Event {
    let mut input = json!({"description": description, "prompt": "do it"});
    if let Some(model) = model {
        input["model"] = json!(model);
    }
    if let Some(kind) = kind {
        input["subagent_type"] = json!(kind);
    }
    call("Agent", input)
}

fn bash(command: &str) -> Event {
    call("Bash", json!({"command": command}))
}

fn big(_: &Path) -> Option<u64> {
    Some(READ_BYTES + 1)
}

fn small(_: &Path) -> Option<u64> {
    Some(READ_BYTES)
}

fn missing(_: &Path) -> Option<u64> {
    None
}

fn rules_of(event: &Event) -> Vec<Rule> {
    spawn_rules(event).iter().map(|w| w.rule).collect()
}

fn bash_hits(command: &str) -> Vec<Would> {
    bash_rules(&bash(command), &Limits::default(), &big)
}

fn bash_rule(command: &str) -> Option<Rule> {
    bash_hits(command).first().map(|w| w.rule)
}

#[test]
fn tags_are_read_from_brackets_only() {
    assert_eq!(
        tags("[code] add the thing"),
        Tags {
            role: Some("code"),
            up: false
        }
    );
    assert_eq!(
        tags("[code up: the parser is subtle] add"),
        Tags {
            role: Some("code"),
            up: true
        }
    );
    assert_eq!(
        tags("[up: needs depth] x"),
        Tags {
            role: None,
            up: true
        }
    );
    assert_eq!(tags("review the code").role, None);
    assert_eq!(tags("[review-secrets] scan").role, Some("review-secrets"));
    assert_eq!(tags("[unknown] x").role, None);
    assert_eq!(tags("[broken x").role, None);
}

#[test]
fn an_untagged_premium_spawn_would_be_blocked() {
    for model in ["opus", "fable", "claude-opus-4-1", "Opus"] {
        let hits = spawn_rules(&spawn("add a column", Some(model), None));
        assert_eq!(
            hits,
            vec![Would {
                rule: Rule::Model,
                action: Action::Block,
                reason: "premium-untagged"
            }],
            "{model}"
        );
    }
}

#[test]
fn a_routine_tag_on_a_premium_model_would_be_flagged() {
    for tag in ["code", "read", "fix"] {
        let hits = spawn_rules(&spawn(&format!("[{tag}] add a column"), Some("opus"), None));
        assert_eq!(hits.len(), 1, "{tag}");
        assert_eq!(hits[0].rule, Rule::Model);
        assert_eq!(hits[0].reason, "premium-routine");
    }
}

#[test]
fn judge_hardest_and_up_spawns_are_left_alone() {
    for description in [
        "[judge] pick a design",
        "[hardest] fix the race",
        "[code up: subtle locking] fix it",
        "[up: reason] x",
        "[review-secrets] scan",
    ] {
        assert!(
            spawn_rules(&spawn(description, Some("opus"), None)).is_empty(),
            "{description}"
        );
    }
}

#[test]
fn cheap_or_unnamed_models_are_not_flagged() {
    for model in [Some("sonnet"), Some("haiku"), None] {
        assert!(spawn_rules(&spawn("[code] x", model, None)).is_empty());
        assert!(spawn_rules(&spawn("add a column", model, None)).is_empty());
    }
}

#[test]
fn a_notchling_agent_type_would_be_corrected_not_blocked() {
    let hits = spawn_rules(&spawn("[code] x", Some("opus"), Some("notchling-builder")));
    assert_eq!(hits[0].action, Action::Correct);
}

#[test]
fn a_fork_that_builds_counts_even_without_a_model() {
    let hits = spawn_rules(&spawn("[code] add it", None, Some("fork")));
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].rule, Rule::Model);
    assert!(spawn_rules(&spawn("talk it over", None, Some("fork"))).is_empty());
    assert!(spawn_rules(&spawn("[judge] decide", None, Some("fork"))).is_empty());
    assert_eq!(
        rules_of(&spawn("add it", Some("fable"), Some("fork"))),
        vec![Rule::Model]
    );
}

#[test]
fn only_a_review_tag_or_agent_type_is_a_review() {
    assert_eq!(
        rules_of(&spawn("[review] the diff", Some("sonnet"), None)),
        vec![Rule::Review]
    );
    assert_eq!(
        rules_of(&spawn("check it", None, Some("notchling-reviewer"))),
        vec![Rule::Review]
    );
    assert_eq!(
        rules_of(&spawn(
            "check it",
            None,
            Some("pr-review-toolkit:code-reviewer")
        )),
        vec![Rule::Review]
    );
    for description in [
        "Fix the review comments from yesterday",
        "review the diff",
        "Review: look at the parser",
        "[code] address review feedback",
    ] {
        assert!(
            spawn_rules(&spawn(description, Some("sonnet"), None)).is_empty(),
            "{description}"
        );
    }
}

#[test]
fn a_review_tag_on_a_premium_model_is_one_review_finding_and_no_model_finding() {
    assert_eq!(
        rules_of(&spawn("[review] the diff", Some("opus"), None)),
        vec![Rule::Review]
    );
}

#[test]
fn other_tools_do_not_spawn() {
    let mut event = spawn("add it", Some("opus"), None);
    event.tool = "Bash".into();
    assert!(spawn_rules(&event).is_empty());
}

#[test]
fn cat_of_a_large_file_would_be_noted() {
    for command in ["cat src/main.rs", "cat -n /abs/file.log", "  cat 'a b.txt'"] {
        let hits = bash_hits(command);
        assert_eq!(hits.len(), 1, "{command}");
        assert_eq!(hits[0].rule, Rule::Read);
        assert_eq!(hits[0].action, Action::Note);
    }
}

#[test]
fn cat_of_a_file_at_the_limit_or_missing_is_left_alone() {
    let event = bash("cat src/main.rs");
    assert!(bash_rules(&event, &Limits::default(), &small).is_empty());
    assert!(bash_rules(&event, &Limits::default(), &missing).is_empty());
}

#[test]
fn asked_for_ranges_pipes_heredocs_and_compounds_are_never_flagged() {
    for command in [
        "sed -n '10,40p' big.log",
        "sed -n '/start/,/end/p' big.log",
        "head -n 50 big.log",
        "head big.log",
        "tail -n 100 big.log",
        "tail -f big.log",
        "cat big.log | head -50",
        "cat big.log | grep error",
        "cat big.log > copy.log",
        "cat < big.log",
        "cat <<EOF\nhello\nEOF",
        "cat big.log && echo done",
        "cat big.log; echo done",
        "cat a.log b.log",
        "cat *.log",
        "cat $FILE",
        "cat ~/big.log",
        "cat \"$(pwd)/big.log\"",
        "cat",
        "cat -",
        "bat big.log",
        "echo cat big.log",
        "timeout 5 cat big.log",
    ] {
        assert_eq!(bash_rule(command), None, "{command}");
    }
}

#[test]
fn a_relative_cat_needs_a_working_folder() {
    let mut event = bash("cat big.log");
    event.cwd = None;
    assert!(bash_rules(&event, &Limits::default(), &big).is_empty());
}

#[test]
fn a_read_with_no_limit_on_a_large_file_would_be_noted() {
    let event = call("Read", json!({"file_path": "/work/big.log"}));
    let hit = read_tool(&event, &Limits::default(), &big).expect("flagged");
    assert_eq!((hit.rule, hit.action), (Rule::Read, Action::Note));
    let relative = call("Read", json!({"file_path": "big.log"}));
    assert!(read_tool(&relative, &Limits::default(), &big).is_some());
}

#[test]
fn a_bounded_or_small_or_media_read_is_left_alone() {
    let limits = Limits::default();
    for input in [
        json!({"file_path": "/work/big.log", "limit": 200}),
        json!({"file_path": "/work/big.log", "offset": 10, "limit": 50}),
        json!({"file_path": "/work/doc.pdf", "pages": "1-3"}),
        json!({"file_path": "/work/shot.png"}),
        json!({"file_path": "/work/doc.PDF"}),
    ] {
        assert!(read_tool(&call("Read", input.clone()), &limits, &big).is_none());
    }
    let plain = call("Read", json!({"file_path": "/work/big.log"}));
    assert!(read_tool(&plain, &limits, &small).is_none());
    assert!(read_tool(&plain, &limits, &missing).is_none());
    let null_limit = call("Read", json!({"file_path": "/work/big.log", "limit": null}));
    assert!(read_tool(&null_limit, &limits, &big).is_some());
    assert!(read_tool(&bash("cat x"), &limits, &big).is_none());
}

#[test]
fn tree_dumps_would_be_noted() {
    for command in [
        "ls -R",
        "ls -laR src",
        "ls --recursive",
        "find .",
        "find / -name '*.rs'",
        "tree",
        "tree src",
        "git ls-files",
        "git ls-files -z",
    ] {
        let hits = bash_hits(command);
        assert_eq!(hits.len(), 1, "{command}");
        assert_eq!((hits[0].rule, hits[0].action), (Rule::Tree, Action::Note));
    }
}

#[test]
fn bounded_tree_commands_and_side_effecting_finds_are_left_alone() {
    for command in [
        "ls",
        "ls -la",
        "ls src",
        "find . -maxdepth 2",
        "find . -maxdepth 1 -name x",
        "find . -name '*.tmp' -delete",
        "find . -type f -exec rm {} +",
        "tree -L 2",
        "git ls-files src",
        "git ls-files -- src/main.rs",
        "git status",
        "git ls-files | wc -l",
        "ls -R | head",
        "find . | head",
    ] {
        assert_eq!(bash_rule(command), None, "{command}");
    }
}

#[test]
fn long_sleeps_and_sleep_loops_would_be_blocked() {
    for command in [
        "sleep 25",
        "sleep 30",
        "sleep 600",
        "sleep 1m",
        "sleep 0.5h",
        "while true; do sleep 5; done",
        "until curl -s localhost:3000; do sleep 2; done",
        "while ! gh pr checks 41; do\n  sleep 10\ndone",
        "  while [ -f lock ]; do sleep 1; done",
    ] {
        let hits = bash_hits(command);
        assert_eq!(hits.len(), 1, "{command}");
        assert_eq!((hits[0].rule, hits[0].action), (Rule::Poll, Action::Block));
    }
}

#[test]
fn short_sleeps_and_wrapped_commands_are_never_polls() {
    for command in [
        "sleep 24",
        "sleep 5",
        "sleep 1",
        "sleep",
        "sleep 30 40",
        "timeout 600 cargo test",
        "timeout 600 cargo test --workspace",
        "cargo test",
        "sleep 30 && echo done",
        "echo sleep 60",
        "while read line; do echo $line; done < f",
        "for i in 1 2 3; do sleep 30; done",
        "notchling wait ci 41",
    ] {
        assert_eq!(bash_rule(command), None, "{command}");
    }
}

#[test]
fn a_command_that_is_not_text_is_left_alone() {
    assert!(bash_rules(
        &call("Bash", json!({"command": 5})),
        &Limits::default(),
        &big
    )
    .is_empty());
    assert!(bash_rules(&call("Bash", json!({})), &Limits::default(), &big).is_empty());
    assert!(bash_rules(
        &call("Bash", json!({"command": ""})),
        &Limits::default(),
        &big
    )
    .is_empty());
    assert!(bash_rules(
        &call("Bash", json!({"command": "cat 'unclosed"})),
        &Limits::default(),
        &big
    )
    .is_empty());
}
