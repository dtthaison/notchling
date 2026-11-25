use super::*;
use crate::contract::Tool;
use crate::sessions::testutil::TempDir;

fn world(tag: &str) -> (TempDir, String) {
    let dir = TempDir::new(tag);
    dir.write(
        "p/sess-1/subagents/agent-a1.meta.json",
        r#"{"agentType":"Explore","description":"Survey","toolUseId":"toolu_1"}"#,
    );
    dir.write(
        "p/sess-1/subagents/agent-a1.jsonl",
        "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n",
    );
    let transcript = dir.write("p/sess-1.jsonl", "");
    (dir, transcript.to_string_lossy().into_owned())
}

#[test]
fn claude_rows_carry_their_session_and_depth_one() {
    let (_dir, transcript) = world("shape-claude");
    let rows = list(&transcript, &HashMap::new(), 0.0);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.id, "claude:a1");
    assert_eq!(row.own_id(), "a1");
    assert_eq!(row.session, "sess-1");
    assert_eq!(row.parent, "sess-1");
    assert_eq!(row.depth, 1);
    assert_eq!(row.origin, Origin::native(Tool::Claude));
}

#[test]
fn status_comes_from_the_finished_map_by_the_own_id() {
    let (_dir, transcript) = world("shape-status");
    let finished: HashMap<String, String> = [("a1".to_string(), "completed".to_string())].into();
    let rows = list(&transcript, &finished, 0.0);
    assert_eq!(rows[0].status, Status::Completed);
    let by_row_id: HashMap<String, String> =
        [("claude:a1".to_string(), "completed".to_string())].into();
    assert_ne!(
        list(&transcript, &by_row_id, 1.0e12)[0].status,
        Status::Completed
    );
}

#[test]
fn the_default_origin_is_native_claude() {
    assert_eq!(Origin::default(), Origin::native(Tool::Claude));
    assert_eq!(Origin::default().via, Via::Native);
    assert_eq!(Origin::delegated(Tool::Codex).via, Via::Delegated);
}

#[test]
fn an_id_with_no_prefix_is_its_own_id() {
    let mut row = list(&world("shape-bare").1, &HashMap::new(), 0.0).remove(0);
    row.id = "plain".into();
    assert_eq!(row.own_id(), "plain");
    row.id = "job:codex-1-2".into();
    assert_eq!(row.own_id(), "codex-1-2");
}
