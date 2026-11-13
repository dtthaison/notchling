use super::*;
use crate::chat::data::DataRoot;
use crate::notch::source::fixture_clock;
use crate::testutil::Scratch;
use notchling_engine::sessions::{AgentGroup, AgentTarget};
use notchling_engine::transcript::model::ItemKind;

fn job(id: &str, cwd: &str) -> Agent {
    Agent {
        id: id.into(),
        name: "notes helper".into(),
        project: "notes".into(),
        cwd: cwd.into(),
        group: AgentGroup::Completed,
        label: "Idle".into(),
        detail: None,
        since_ms: 0,
        target: AgentTarget::Job("cn01".into()),
        suggested_reply: None,
        transcript_path: None,
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
    }
}

fn said(id: &str, text: &str) -> String {
    format!(
        r#"{{"type":"user","sessionId":"{id}","message":{{"role":"user","content":"{text}"}}}}"#
    ) + "\n"
}

fn continued(id: &str, to: &str) -> String {
    format!(r#"{{"type":"continued-in","sessionId":"{id}","continuedInSessionId":"{to}"}}"#) + "\n"
}

fn fixture() -> DataRoot {
    DataRoot::fixture(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/chat-continued"))
}

#[test]
fn a_job_with_no_transcript_yet_shows_the_session_it_continues() {
    let dir = Scratch::new("locator-continued");
    let projects = dir.path().join("projects");
    let old = dir.write(
        "projects/-work-app/old.jsonl",
        &(said("old", "hi") + &continued("old", "new")),
    );
    let mut locator = Locator::new(None);
    let row = job("new", "/work/app");
    assert_eq!(locator.find(&row, &projects, 1_000), Some(old.clone()));
    assert!(
        locator.has_missed("new"),
        "the new file is still looked for"
    );
    assert_eq!(locator.find(&row, &projects, 2_000), Some(old));

    let file = dir.write("projects/-work-app/new.jsonl", "{}\n");
    assert_eq!(
        locator.find(&row, &projects, 1_000 + MISS_RETRY),
        Some(file),
        "the file itself wins once it exists"
    );
}

#[test]
fn a_job_with_no_continued_in_behind_it_still_finds_nothing() {
    let dir = Scratch::new("locator-nothing");
    let projects = dir.path().join("projects");
    dir.write("projects/-work-app/old.jsonl", &said("old", "hi"));
    let mut locator = Locator::new(None);
    assert_eq!(locator.find(&job("new", "/work/app"), &projects, 0), None);
}

#[test]
fn chat_shows_the_earlier_messages_of_a_session_that_was_continued_in() {
    let root = fixture();
    let sessions = root.sessions(0);
    assert_eq!(sessions.len(), 1);
    let session = &sessions[0];
    assert!(session
        .transcript
        .as_deref()
        .is_some_and(|p| p.ends_with("7b4d0f21-2222-4b3c-8d4e-123456789abc.jsonl")));
    let shown = root.conversation_shared(session);
    let users: Vec<&str> = shown
        .items
        .iter()
        .filter_map(|i| match &i.kind {
            ItemKind::User { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        users,
        [
            "Tidy the notes folder and tell me what changed.",
            "Now do the archive folder too."
        ]
    );
    assert_eq!(shown.facts.model.as_deref(), Some("claude-opus-5-5"));
}

#[test]
fn the_notch_row_of_a_session_that_was_continued_in_has_the_earlier_history() {
    let mut source = fixture().notch_source();
    let first = source.load(0);
    let loaded = source.load(fixture_clock(&first.agents));
    assert_eq!(loaded.agents.len(), 1);
    let shot = &loaded.snapshots[&loaded.agents[0].id];
    assert_eq!(
        shot.last_prompt.as_deref(),
        Some("Now do the archive folder too.")
    );
    assert!(shot.lines.len() >= 4, "{:?}", shot.lines);
}
