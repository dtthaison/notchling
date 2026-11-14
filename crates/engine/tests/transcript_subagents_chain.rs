mod common;

use common::*;
use notchling_engine::transcript::subagents::{self, Status};
use notchling_engine::transcript::util::now_secs;
use std::collections::HashMap;

fn two() -> (Scratch, String, String) {
    let scratch = Scratch::new("subagents-chain");
    let old = s(&scratch.write("old.jsonl", ""));
    let new = s(&scratch.write("new.jsonl", ""));
    (scratch, new, old)
}

fn finished(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

#[test]
fn an_id_in_two_folders_is_one_row_from_the_first_with_the_meta_it_lacks() {
    let (scratch, new, old) = two();
    scratch.write(
        "old/subagents/agent-ax.meta.json",
        r#"{"description":"Port it","agentType":"Explore","model":"sonnet","effort":"high","toolUseId":"toolu_x"}"#,
    );
    scratch.write("old/subagents/agent-ax.jsonl", "{}\n");
    let continued = scratch.write("new/subagents/agent-ax.jsonl", "{}\n");

    let list = subagents::list_chain(
        &[new.as_str(), old.as_str()],
        &finished(&[("toolu_x", "completed")]),
        now_secs(),
    );
    assert_eq!(list.len(), 1);
    let agent = &list[0];
    assert_eq!(agent.path, s(&continued), "the file it goes on in");
    assert_eq!(agent.description, "Port it");
    assert_eq!(agent.agent_type, "Explore");
    assert_eq!(agent.model.as_deref(), Some("sonnet"));
    assert_eq!(agent.effort.as_deref(), Some("high"));
    assert_eq!(agent.tool_use_id.as_deref(), Some("toolu_x"));
    assert_eq!(
        agent.status,
        Status::Completed,
        "the status reads the meta it took"
    );
}

#[test]
fn the_first_meta_found_wins() {
    let (scratch, new, old) = two();
    scratch.write(
        "old/subagents/agent-ax.meta.json",
        r#"{"description":"Old words"}"#,
    );
    scratch.write("old/subagents/agent-ax.jsonl", "{}\n");
    scratch.write(
        "new/subagents/agent-ax.meta.json",
        r#"{"description":"New words"}"#,
    );
    scratch.write("new/subagents/agent-ax.jsonl", "{}\n");
    let list = subagents::list_chain(&[new, old], &HashMap::new(), now_secs());
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].description, "New words");
}

#[test]
fn a_chain_of_one_is_the_plain_list_and_a_missing_folder_is_skipped() {
    let (scratch, new, old) = two();
    scratch.write(
        "old/subagents/agent-ay.meta.json",
        r#"{"description":"Only here"}"#,
    );
    scratch.write("old/subagents/agent-ay.jsonl", "{}\n");
    let now = now_secs();
    assert_eq!(
        subagents::list_chain(&[old.as_str()], &HashMap::new(), now),
        subagents::list(&old, &HashMap::new(), now)
    );
    let both = subagents::list_chain(&[new, old], &HashMap::new(), now);
    assert_eq!(both.len(), 1);
    assert_eq!(both[0].description, "Only here");
}
