use super::*;
use crate::ask::find::canon;
use crate::ask::model::{FIND_MAX_FILES, FIND_WINDOW_MS};
use crate::sessions::iso::parse_iso_ms;
use crate::sessions::testutil::TempDir;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const MAIN: &str = include_str!("../../tests/fixtures/ask/main.jsonl");
const BLOCKING: &str = include_str!("../../tests/fixtures/ask/blocking.jsonl");
const A1: &str = include_str!("../../tests/fixtures/ask/subagents/agent-a1.jsonl");
const A1_META: &str = include_str!("../../tests/fixtures/ask/subagents/agent-a1.meta.json");
const A2: &str = include_str!("../../tests/fixtures/ask/subagents/agent-a2.jsonl");
const A2_META: &str = include_str!("../../tests/fixtures/ask/subagents/agent-a2.meta.json");

const SESSION: &str = "11111111-2222-4333-8444-555555555555";
const TASKS: &str =
    "/private/tmp/claude-501/-Users-dev-work/11111111-2222-4333-8444-555555555555/tasks";

struct Layout {
    _tmp: TempDir,
    main: PathBuf,
}

fn layout(main: &str) -> Layout {
    let tmp = TempDir::new("ask-find");
    let main_path = tmp.write(&format!("projects/-Users-dev-work/{SESSION}.jsonl"), main);
    let sub = format!("projects/-Users-dev-work/{SESSION}/subagents");
    tmp.write(&format!("{sub}/agent-a1.jsonl"), A1);
    tmp.write(&format!("{sub}/agent-a1.meta.json"), A1_META);
    tmp.write(&format!("{sub}/agent-a2.jsonl"), A2);
    tmp.write(&format!("{sub}/agent-a2.meta.json"), A2_META);
    Layout {
        _tmp: tmp,
        main: main_path,
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn at(stamp: &str) -> i64 {
    parse_iso_ms(stamp).unwrap()
}

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

fn asking_argv() -> Vec<String> {
    argv(&[
        "/Applications/Notchling.app/Contents/MacOS/notchling",
        "ask",
        "--choice",
        "Keep it",
        "--choice",
        "Drop it",
        "--",
        "Keep the old API?",
    ])
}

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn subagents_only(all: Vec<Candidate>) -> Vec<Candidate> {
    all.into_iter()
        .filter(|c| matches!(c.source, Source::Subagent { .. }))
        .collect()
}

fn output(name: &str) -> PathBuf {
    PathBuf::from(format!("{TASKS}/{name}.output"))
}

fn found_agent(tool_use: &str, id: &str, description: &str) -> Found {
    Found {
        asker: Asker::Subagent,
        agent_id: Some(id.to_string()),
        label: Some(description.to_string()),
        agent_type: Some("general-purpose".to_string()),
        model: Some("sonnet".to_string()),
        tool_use_id: Some(tool_use.to_string()),
    }
}

fn unknown() -> Found {
    Found {
        asker: Asker::Unknown,
        agent_id: None,
        label: None,
        agent_type: None,
        model: None,
        tool_use_id: None,
    }
}

#[test]
fn background_ask_in_a_subagent_is_that_subagent() {
    let l = layout(MAIN);
    let all = candidates(&l.main, "Quiet", now_ms());
    let argv = asking_argv();
    let stdout = output("ba1000001");
    let claimed = HashSet::new();
    let asking = Asking {
        argv: &argv,
        stdout: Some(&stdout),
        opened_ms: at("2026-11-16T10:01:11.500Z"),
        claimed: &claimed,
    };
    assert_eq!(
        find(&asking, &all, read),
        Some(found_agent("toolu_a1_bg", "a1", "Port the parser"))
    );
}

#[test]
fn background_ask_in_main_is_main() {
    let l = layout(MAIN);
    let all = candidates(&l.main, "Quiet", now_ms());
    let argv = asking_argv();
    let stdout = output("bmain0001");
    let claimed = HashSet::new();
    let asking = Asking {
        argv: &argv,
        stdout: Some(&stdout),
        opened_ms: at("2026-11-16T10:00:06.500Z"),
        claimed: &claimed,
    };
    assert_eq!(
        find(&asking, &all, read),
        Some(Found {
            asker: Asker::Main,
            agent_id: None,
            label: Some("Quiet".to_string()),
            agent_type: None,
            model: None,
            tool_use_id: Some("toolu_main_bg".to_string()),
        })
    );
}

#[test]
fn output_file_tells_identical_commands_apart() {
    let l = layout(MAIN);
    let all = candidates(&l.main, "Quiet", now_ms());
    let argv = asking_argv();
    let stdout = output("ba2000002");
    let claimed = HashSet::new();
    let asking = Asking {
        argv: &argv,
        stdout: Some(&stdout),
        opened_ms: at("2026-11-16T10:01:11.500Z"),
        claimed: &claimed,
    };
    assert_eq!(
        find(&asking, &all, read),
        Some(found_agent("toolu_a2_bg", "a2", "Write the parser tests"))
    );
}

#[test]
fn foreground_output_file_falls_back_to_the_command() {
    let l = layout(MAIN);
    let all = subagents_only(candidates(&l.main, "Quiet", now_ms()));
    let argv = asking_argv();
    let stdout = output("bforeground9");
    let claimed: HashSet<String> = ["toolu_a1_bg".to_string()].into();
    let asking = Asking {
        argv: &argv,
        stdout: Some(&stdout),
        opened_ms: at("2026-11-16T10:01:11.500Z"),
        claimed: &claimed,
    };
    assert_eq!(
        find(&asking, &all, read),
        Some(found_agent("toolu_a2_bg", "a2", "Write the parser tests"))
    );
}

#[test]
fn same_command_two_subagents_is_unknown() {
    let l = layout(MAIN);
    let all = subagents_only(candidates(&l.main, "Quiet", now_ms()));
    assert_eq!(all.len(), 2);
    let argv = asking_argv();
    let none = HashSet::new();
    let asking = Asking {
        argv: &argv,
        stdout: None,
        opened_ms: at("2026-11-16T10:01:11.500Z"),
        claimed: &none,
    };
    assert_eq!(find(&asking, &all, read), Some(unknown()));
    let claimed: HashSet<String> = ["toolu_a1_bg".to_string()].into();
    let asking = Asking {
        claimed: &claimed,
        ..asking
    };
    assert_eq!(
        find(&asking, &all, read),
        Some(found_agent("toolu_a2_bg", "a2", "Write the parser tests"))
    );
}

#[test]
fn command_after_cd_matches() {
    let l = layout(BLOCKING);
    let all = candidates(&l.main, "Quiet", now_ms());
    let argv = argv(&[
        "/Applications/Notchling.app/Contents/MacOS/notchling",
        "ask",
        "--",
        "Which port should the test server use?",
    ]);
    let claimed = HashSet::new();
    let asking = Asking {
        argv: &argv,
        stdout: None,
        opened_ms: at("2026-11-16T10:02:46.000Z"),
        claimed: &claimed,
    };
    let got = find(&asking, &all, read).unwrap();
    assert_eq!(got.asker, Asker::Main);
    assert_eq!(got.tool_use_id.as_deref(), Some("toolu_cd"));
}

#[test]
fn blocking_wait_command_matches() {
    let l = layout(BLOCKING);
    let all = candidates(&l.main, "Quiet", now_ms());
    let argv = argv(&[
        "notchling",
        "ask",
        "--wait",
        "--choice",
        "Ship it",
        "--choice",
        "Hold it",
        "--",
        "Ship 2.0 today?",
    ]);
    let claimed = HashSet::new();
    let asking = Asking {
        argv: &argv,
        stdout: None,
        opened_ms: at("2026-11-16T10:02:06.000Z"),
        claimed: &claimed,
    };
    let got = find(&asking, &all, read).unwrap();
    assert_eq!(got.tool_use_id.as_deref(), Some("toolu_wait"));
}

#[test]
fn older_than_ten_minutes_never_matches() {
    let l = layout(BLOCKING);
    let all = candidates(&l.main, "Quiet", now_ms());
    let argv = argv(&[
        "notchling",
        "ask",
        "--",
        "Which port should the test server use?",
    ]);
    let claimed = HashSet::new();
    let opened = at("2026-11-16T10:02:45.000Z");
    let late = Asking {
        argv: &argv,
        stdout: None,
        opened_ms: opened + FIND_WINDOW_MS + 1_000,
        claimed: &claimed,
    };
    assert_eq!(find(&late, &all, read), None);
    let in_time = Asking {
        opened_ms: opened + FIND_WINDOW_MS - 1_000,
        ..late
    };
    assert!(find(&in_time, &all, read).is_some());
}

#[test]
fn no_line_yet_is_none() {
    let l = layout(MAIN);
    let all = subagents_only(candidates(&l.main, "Quiet", now_ms()));
    let argv = asking_argv();
    let claimed = HashSet::new();
    let stdout = output("ba1000001");
    let asking = Asking {
        argv: &argv,
        stdout: Some(&stdout),
        opened_ms: at("2026-11-16T10:01:11.500Z"),
        claimed: &claimed,
    };
    let first_line_only =
        |path: &Path| read(path).map(|text| text.lines().next().unwrap_or("").to_string());
    assert_eq!(find(&asking, &all, first_line_only), None);
}

#[test]
fn partial_first_line_is_skipped() {
    let l = layout(MAIN);
    let all = candidates(&l.main, "Quiet", now_ms());
    let argv = asking_argv();
    let claimed = HashSet::new();
    let stdout = output("ba1000001");
    let asking = Asking {
        argv: &argv,
        stdout: Some(&stdout),
        opened_ms: at("2026-11-16T10:01:11.500Z"),
        claimed: &claimed,
    };
    let cut = |path: &Path| read(path).map(|text| format!("tool_use\", \"x\": 1}}\n{text}"));
    let got = find(&asking, &all, cut).unwrap();
    assert_eq!(got.agent_id.as_deref(), Some("a1"));
}

#[test]
fn tool_result_parts_list_form_matches() {
    let l = layout(MAIN);
    let listed = A1
        .replace(
            "\"content\": \"Command running in background",
            "\"content\": [{\"type\": \"text\", \"text\": \"Command running in background",
        )
        .replace(
            "do not poll that file or sleep while waiting.\"",
            "do not poll that file or sleep while waiting.\"}]",
        );
    assert_ne!(listed, A1);
    let all = candidates(&l.main, "Quiet", now_ms());
    let argv = asking_argv();
    let claimed = HashSet::new();
    let stdout = output("ba1000001");
    let asking = Asking {
        argv: &argv,
        stdout: Some(&stdout),
        opened_ms: at("2026-11-16T10:01:11.500Z"),
        claimed: &claimed,
    };
    let tail = |path: &Path| {
        if path.ends_with("agent-a1.jsonl") {
            Some(listed.clone())
        } else {
            read(path)
        }
    };
    let got = find(&asking, &all, tail).unwrap();
    assert_eq!(got.agent_id.as_deref(), Some("a1"));
}

#[cfg(unix)]
#[test]
fn canonical_paths_compare() {
    let l = layout(MAIN);
    let all = candidates(&l.main, "Quiet", now_ms());
    let argv = asking_argv();
    let claimed = HashSet::new();
    let link_root = TempDir::new("ask-find-link");
    std::fs::create_dir_all(link_root.path("real")).unwrap();
    std::os::unix::fs::symlink(link_root.path("real"), link_root.path("link")).unwrap();
    let stdout = link_root.path("link/ba2000002.output");
    let launched = link_root
        .path("real/ba2000002.output")
        .to_string_lossy()
        .into_owned();
    let relaunched = A2.replace(&format!("{TASKS}/ba2000002.output"), &launched);
    assert_ne!(relaunched, A2);
    let asking = Asking {
        argv: &argv,
        stdout: Some(&stdout),
        opened_ms: at("2026-11-16T10:01:11.500Z"),
        claimed: &claimed,
    };
    let tail = |path: &Path| {
        if path.ends_with("agent-a2.jsonl") {
            Some(relaunched.clone())
        } else {
            read(path)
        }
    };
    let got = find(&asking, &all, tail).unwrap();
    assert_eq!(got.agent_id.as_deref(), Some("a2"));
    assert_eq!(
        canon(Path::new("/tmp/claude-501/x.output")),
        canon(Path::new("/tmp")).join("claude-501/x.output")
    );
}

#[test]
fn candidates_cap_and_window() {
    let l = layout(MAIN);
    let dir = l.main.with_extension("").join("subagents");
    let old_file = dir.join("agent-a1.jsonl");
    let eleven_minutes = SystemTime::now() - Duration::from_secs(11 * 60);
    std::fs::File::options()
        .write(true)
        .open(&old_file)
        .unwrap()
        .set_modified(eleven_minutes)
        .unwrap();
    for n in 0..70 {
        let id = format!("z{n:02}");
        std::fs::write(dir.join(format!("agent-{id}.jsonl")), A2).unwrap();
        std::fs::write(dir.join(format!("agent-{id}.meta.json")), A2_META).unwrap();
    }
    let all = candidates(&l.main, "Quiet", now_ms());
    assert_eq!(all.len(), FIND_MAX_FILES);
    assert!(matches!(all[0].source, Source::Main { .. }));
    assert_eq!(all[0].path, l.main);
    assert!(all.iter().all(|c| c.path != old_file));
    let ids: Vec<&str> = all
        .iter()
        .filter_map(|c| match &c.source {
            Source::Subagent { id, .. } => Some(id.as_str()),
            Source::Main { .. } => None,
        })
        .collect();
    assert_eq!(ids.len(), FIND_MAX_FILES - 1);
    let few = layout(MAIN);
    let fewer = candidates(&few.main, "", now_ms());
    assert_eq!(fewer.len(), 3);
    assert_eq!(fewer[0].source, Source::Main { name: None });
}

#[test]
fn delegate_tool_reads_argv() {
    let full = "/Applications/Notchling.app/Contents/MacOS/notchling";
    assert_eq!(
        delegate_tool(&argv(&[full, "delegate", "codex", "--write", "--", "x"])),
        Some(Asker::Codex)
    );
    assert_eq!(
        delegate_tool(&argv(&["notchling", "delegate", "agy", "--", "x"])),
        Some(Asker::Antigravity)
    );
    assert_eq!(
        delegate_tool(&argv(&["notchling", "delegate", "other"])),
        None
    );
    assert_eq!(delegate_tool(&argv(&["notchling", "ask", "codex"])), None);
    assert_eq!(delegate_tool(&argv(&["other", "delegate", "codex"])), None);
    assert_eq!(delegate_tool(&[]), None);
}
