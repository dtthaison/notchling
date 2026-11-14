use super::*;
use crate::transcript::{codex, conversation};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/versions")
        .join(name)
}

const BUDGET: u64 = 4_000_000;

#[test]
fn unknown_block_counts_under_its_version() {
    let skips = strict_claude(&fixture("claude-unknown-block.jsonl"), BUDGET);
    assert_eq!(skips.count("2.1.324", SkipKind::UnknownBlock), 1);
    assert_eq!(skips.unknown_total(), 1);
    assert_eq!(skips.lines_by_version["2.1.324"], 3);
    assert_eq!(skips.turns_in("2.1.324"), 1);
}

#[test]
fn unknown_codex_event_counts() {
    let skips = strict_codex(&fixture("codex-unknown-event.jsonl"), BUDGET);
    assert_eq!(skips.count("0.167.0", SkipKind::UnknownCodexEvent), 1);
    assert_eq!(skips.unknown_total(), 1);
    assert_eq!(skips.turns_in("0.167.0"), 1);
}

#[test]
fn old_lines_count_under_their_own_version() {
    let skips = strict_claude(&fixture("claude-mixed-versions.jsonl"), BUDGET);
    assert_eq!(skips.count("2.1.323", SkipKind::UnknownBlock), 1);
    assert_eq!(skips.unknown_in("2.1.324"), 0);
    assert_eq!(skips.turns_in("2.1.323"), 1);
    assert_eq!(skips.turns_in("2.1.324"), 1);
}

#[test]
fn strict_pass_never_holds_text() {
    let claude = strict_claude(&fixture("claude-unknown-block.jsonl"), BUDGET);
    let codex = strict_codex(&fixture("codex-unknown-event.jsonl"), BUDGET);
    for skips in [claude, codex] {
        let text = serde_json::to_string(&skips).expect("json");
        assert!(!text.contains("PLANTED-MARKER"), "{text}");
        assert!(!text.contains("hologram"), "{text}");
        assert!(!text.contains("teleport"), "{text}");
    }
}

#[test]
fn lenient_reader_output_is_unchanged() {
    let path = fixture("claude-unknown-block.jsonl");
    let first = conversation::read(&path.to_string_lossy());
    strict_claude(&path, BUDGET);
    let second = conversation::read(&path.to_string_lossy());
    assert_eq!(first.items.len(), second.items.len());
    assert_eq!(first.items.len(), 3);
    assert_eq!(first.facts.turns, 1);
    let rollout = fixture("codex-unknown-event.jsonl");
    let first = codex::read(&rollout.to_string_lossy());
    strict_codex(&rollout, BUDGET);
    let second = codex::read(&rollout.to_string_lossy());
    assert_eq!(first.items.len(), second.items.len());
    assert_eq!(first.facts.turns, 1);
}

#[test]
fn unparseable_lines_and_queue_ops_are_kinds() {
    let mut skips = Skips::default();
    let lines: Vec<&[u8]> = vec![
        b"not json at all",
        br#"{"type":"queue-operation","version":"9.9.9","operation":"teleport"}"#,
        br#"{"type":"surprise","version":"9.9.9"}"#,
    ];
    claude_lines(&mut skips, lines.into_iter());
    assert_eq!(skips.count("", SkipKind::ParseFailed), 1);
    assert_eq!(skips.count("9.9.9", SkipKind::UnknownQueueOp), 1);
    assert_eq!(skips.count("9.9.9", SkipKind::UnknownLine), 1);
}

#[test]
fn acp_updates_are_judged() {
    let lines = vec![
        r#"{"sessionUpdate":"agent_message_chunk","content":{"text":"x"}}"#.to_string(),
        r#"{"params":{"update":{"sessionUpdate":"warp_drive"}}}"#.to_string(),
        "{".to_string(),
    ];
    let skips = strict_acp(&lines);
    assert_eq!(skips.count("", SkipKind::UnknownAcpUpdate), 1);
    assert_eq!(skips.count("", SkipKind::ParseFailed), 1);
}

#[test]
fn the_self_test_fixtures_are_clean() {
    let claude = strict_claude(&fixture("self-test-claude.jsonl"), BUDGET);
    let codex = strict_codex(&fixture("self-test-codex.jsonl"), BUDGET);
    assert_eq!(claude.unknown_total(), 0);
    assert_eq!(codex.unknown_total(), 0);
}

#[test]
fn merge_adds_counts() {
    let mut a = Skips::default();
    a.note(Some("1.0.0"), SkipKind::UnknownLine);
    let mut b = Skips::default();
    b.note(Some("1.0.0"), SkipKind::UnknownLine);
    b.turn(Some("1.0.0"));
    a.merge(&b);
    assert_eq!(a.count("1.0.0", SkipKind::UnknownLine), 2);
    assert_eq!(a.turns_in("1.0.0"), 1);
}
