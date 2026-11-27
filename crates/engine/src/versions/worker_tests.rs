use super::mark::{mark, Mark};
use super::worker::{parse_probe, ToolState, WorkerTools};
use super::{Method, Tool, Version};

const RECORDED: &str = "claude.path=/home/w/.local/bin/claude\nclaude.link=/home/w/.local/share/claude/versions/2.1.324\nclaude.version=2.1.324 (Claude Code)\ncodex.path=/usr/local/bin/codex\ncodex.link=../lib/node_modules/@openai/codex/bin/codex.js\ncodex.version=codex-cli 0.166.0\nagy.path=\nnotchling.path=\n";

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn installed(tools: &WorkerTools, tool: Tool) -> (Option<Version>, Method, String) {
    match tools.tools.get(&tool).unwrap() {
        ToolState::Installed {
            version,
            method,
            path,
        } => (version.clone(), *method, path.clone()),
        ToolState::NotInstalled => panic!("{tool:?} not installed"),
    }
}

#[test]
fn probe_lines_make_tools() {
    let tools = parse_probe(RECORDED, 42);
    assert_eq!(tools.at_ms, 42);
    assert_eq!(tools.tools.len(), 4);
    let (version, method, path) = installed(&tools, Tool::Claude);
    assert_eq!(version, Some(v("2.1.324")));
    assert_eq!(method, Method::Native);
    assert_eq!(path, "/home/w/.local/bin/claude");
    let (version, method, _) = installed(&tools, Tool::Codex);
    assert_eq!(version, Some(v("0.166.0")));
    assert_eq!(method, Method::Npm);
    assert_eq!(tools.tools[&Tool::Agy], ToolState::NotInstalled);
    assert_eq!(tools.tools[&Tool::Notchling], ToolState::NotInstalled);
}

#[test]
fn empty_path_is_not_installed() {
    let tools = parse_probe("claude.path=\nclaude.version=2.1.324 (Claude Code)\n", 0);
    assert_eq!(tools.tools[&Tool::Claude], ToolState::NotInstalled);
    assert_eq!(
        parse_probe("", 0).tools[&Tool::Codex],
        ToolState::NotInstalled
    );
}

#[test]
fn hostile_probe_output_keeps_nothing() {
    let mut text = String::from("claude.path=/home/w/.local/bin/claude\nclaude.version=2.1.3;id\n");
    text.push_str("codex.path=/tmp/../etc/codex\ncodex.version=codex-cli 0.1.0\n");
    for i in 0..200 {
        text.push_str(&format!("noise{i}=x\n"));
    }
    text.push_str("agy.path=/home/w/.local/bin/agy\n");
    let tools = parse_probe(&text, 0);
    let (version, _, path) = installed(&tools, Tool::Claude);
    assert_eq!(version, None);
    assert_eq!(path, "/home/w/.local/bin/claude");
    let (version, method, path) = installed(&tools, Tool::Codex);
    assert_eq!(path, "");
    assert_eq!(method, Method::Unknown);
    assert_eq!(version, Some(v("0.1.0")));
    assert_eq!(tools.tools[&Tool::Agy], ToolState::NotInstalled);
    let newline_path = parse_probe("agy.path=/a/b\\nc d\n", 0);
    let (_, method, path) = installed(&newline_path, Tool::Agy);
    assert_eq!((method, path.as_str()), (Method::Unknown, ""));
}

#[test]
fn a_key_seen_twice_keeps_the_first() {
    let tools = parse_probe(
        "claude.path=/a/claude\nclaude.version=1.0.0 (Claude Code)\nclaude.version=2.0.0 (Claude Code)\nclaude.path=/b/claude\n",
        0,
    );
    let (version, _, path) = installed(&tools, Tool::Claude);
    assert_eq!(version, Some(v("1.0.0")));
    assert_eq!(path, "/a/claude");
}

#[test]
fn marks_cover_every_case() {
    let mac = v("2.1.324");
    let native = |version: Option<Version>| ToolState::Installed {
        version,
        method: Method::Native,
        path: "/h/.local/bin/claude".into(),
    };
    let same = native(Some(v("2.1.324")));
    assert_eq!(mark(Tool::Claude, Some(&mac), &same, false), Mark::Same);
    let behind = native(Some(v("2.1.323")));
    assert_eq!(mark(Tool::Claude, Some(&mac), &behind, false), Mark::Behind);
    let ahead = native(Some(v("2.1.325")));
    assert_eq!(mark(Tool::Claude, Some(&mac), &ahead, false), Mark::Ahead);
    assert_eq!(mark(Tool::Claude, Some(&mac), &behind, true), Mark::Held);
    assert_eq!(
        mark(Tool::Claude, Some(&mac), &ToolState::NotInstalled, true),
        Mark::NotInstalled
    );
    let brew = ToolState::Installed {
        version: Some(v("2.1.323")),
        method: Method::Brew,
        path: "/opt/homebrew/bin/claude".into(),
    };
    assert_eq!(mark(Tool::Claude, Some(&mac), &brew, false), Mark::CantPin);
    assert_eq!(
        mark(Tool::Claude, None, &native(Some(v("2.1.3"))), false),
        Mark::Unknown
    );
    assert_eq!(
        mark(Tool::Claude, Some(&mac), &native(None), false),
        Mark::Unknown
    );
}

#[test]
fn marks_serialize_in_snake_case() {
    assert_eq!(
        serde_json::to_string(&Mark::CantPin).unwrap(),
        "\"cant_pin\""
    );
    assert_eq!(
        serde_json::to_string(&Mark::NotInstalled).unwrap(),
        "\"not_installed\""
    );
}

#[test]
fn worker_tools_round_trip_with_the_stored_names() {
    let tools = parse_probe(RECORDED, 7);
    let text = serde_json::to_string(&tools).unwrap();
    assert!(text.contains("\"atMs\":7"));
    assert!(text.contains("\"antigravity\":{\"state\":\"notInstalled\"}"));
    let back: WorkerTools = serde_json::from_str(&text).unwrap();
    assert_eq!(back, tools);
    let older: WorkerTools =
        serde_json::from_str("{\"tools\":{\"agy\":{\"state\":\"notInstalled\"}},\"atMs\":1}")
            .unwrap();
    assert_eq!(older.tools[&Tool::Agy], ToolState::NotInstalled);
}
