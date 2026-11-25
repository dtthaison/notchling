use super::evidence::{command_hash, evidence_hash, output_hash, Evidence};
use super::{candidates, find_by_evidence, Asker, Candidate, Source};
use crate::sessions::iso::parse_iso_ms;
use crate::sessions::testutil::TempDir;
use std::path::{Path, PathBuf};

const MAIN: &str = include_str!("../../tests/fixtures/ask/main.jsonl");
const A1: &str = include_str!("../../tests/fixtures/ask/subagents/agent-a1.jsonl");
const A1_META: &str = include_str!("../../tests/fixtures/ask/subagents/agent-a1.meta.json");
const A2: &str = include_str!("../../tests/fixtures/ask/subagents/agent-a2.jsonl");
const A2_META: &str = include_str!("../../tests/fixtures/ask/subagents/agent-a2.meta.json");

const SESSION: &str = "11111111-2222-4333-8444-555555555555";
const TASKS: &str =
    "/private/tmp/claude-501/-Users-dev-work/11111111-2222-4333-8444-555555555555/tasks";
const SALT: [u8; 32] = [3; 32];

struct Layout {
    _tmp: TempDir,
    main: PathBuf,
}

fn layout() -> Layout {
    let tmp = TempDir::new("ask-evidence");
    let main = tmp.write(&format!("projects/-Users-dev-work/{SESSION}.jsonl"), MAIN);
    let sub = format!("projects/-Users-dev-work/{SESSION}/subagents");
    tmp.write(&format!("{sub}/agent-a1.jsonl"), A1);
    tmp.write(&format!("{sub}/agent-a1.meta.json"), A1_META);
    tmp.write(&format!("{sub}/agent-a2.jsonl"), A2);
    tmp.write(&format!("{sub}/agent-a2.meta.json"), A2_META);
    Layout { _tmp: tmp, main }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn asking_argv() -> Vec<String> {
    [
        "/Applications/Notchling.app/Contents/MacOS/notchling",
        "ask",
        "--choice",
        "Keep it",
        "--choice",
        "Drop it",
        "--",
        "Keep the old API?",
    ]
    .iter()
    .map(|w| w.to_string())
    .collect()
}

fn subagents_only(all: Vec<Candidate>) -> Vec<Candidate> {
    all.into_iter()
        .filter(|c| matches!(c.source, Source::Subagent { .. }))
        .collect()
}

#[test]
fn the_output_file_names_the_subagent_from_its_hash_alone() {
    let l = layout();
    let all = candidates(&l.main, "Quiet", now_ms());
    let out = output_hash(&SALT, Path::new(&format!("{TASKS}/ba1000001.output")));
    let evidence = Evidence {
        salt: &SALT,
        cmd_hash: None,
        out_hash: Some(&out),
        opened_ms: parse_iso_ms("2026-11-16T10:01:11.500Z").unwrap(),
    };
    let found = find_by_evidence(&evidence, &all, read).expect("found");
    assert_eq!(found.asker, Asker::Subagent);
    assert_eq!(found.agent_id.as_deref(), Some("a1"));
    let main = output_hash(&SALT, Path::new(&format!("{TASKS}/bmain0001.output")));
    let evidence = Evidence {
        out_hash: Some(&main),
        opened_ms: parse_iso_ms("2026-11-16T10:00:06.500Z").unwrap(),
        ..evidence
    };
    let found = find_by_evidence(&evidence, &all, read).expect("found");
    assert_eq!(found.asker, Asker::Main);
}

#[test]
fn the_command_hash_matches_one_and_two_matches_are_unknown() {
    let l = layout();
    let all = subagents_only(candidates(&l.main, "Quiet", now_ms()));
    let cmd = command_hash(&SALT, &asking_argv()).unwrap();
    let evidence = Evidence {
        salt: &SALT,
        cmd_hash: Some(&cmd),
        out_hash: None,
        opened_ms: parse_iso_ms("2026-11-16T10:01:11.500Z").unwrap(),
    };
    let found = find_by_evidence(&evidence, &all, read).expect("found");
    assert_eq!(found.asker, Asker::Unknown);
    let one = &all[..1];
    let found = find_by_evidence(&evidence, one, read).expect("found");
    assert_ne!(found.asker, Asker::Unknown);
}

#[test]
fn a_hash_of_another_command_or_another_salt_finds_nothing() {
    let l = layout();
    let all = candidates(&l.main, "Quiet", now_ms());
    let mut other = asking_argv();
    other.push("more".into());
    let cmd = command_hash(&SALT, &other).unwrap();
    let evidence = Evidence {
        salt: &SALT,
        cmd_hash: Some(&cmd),
        out_hash: None,
        opened_ms: parse_iso_ms("2026-11-16T10:01:11.500Z").unwrap(),
    };
    assert!(find_by_evidence(&evidence, &all, read).is_none());
    let salted = command_hash(&[9; 32], &asking_argv()).unwrap();
    let evidence = Evidence {
        cmd_hash: Some(&salted),
        ..evidence
    };
    assert!(find_by_evidence(&evidence, &all, read).is_none());
}

#[test]
fn hashes_are_salted_hex_and_depend_on_every_part() {
    let a = evidence_hash(&SALT, "cmd", &["a", "b"]);
    assert_eq!(a.len(), 32);
    assert!(a.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_ne!(a, evidence_hash(&SALT, "cmd", &["ab"]));
    assert_ne!(a, evidence_hash(&SALT, "out", &["a", "b"]));
    assert_ne!(a, evidence_hash(&[0; 32], "cmd", &["a", "b"]));
    assert_eq!(command_hash(&SALT, &[]), None);
}
