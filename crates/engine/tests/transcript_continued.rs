mod common;

use common::{s, Scratch};
use notchling_engine::transcript::continued::earlier;
use notchling_engine::transcript::conversation;
use notchling_engine::transcript::locate::{predecessor, successor};
use notchling_engine::transcript::model::{Conversation, ItemKind};
use notchling_engine::transcript::snapshot;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const OLD: &str = "5a3c9e10-1111-4a2b-9c3d-0123456789ab";
const NEW: &str = "7b4d0f21-2222-4b3c-8d4e-123456789abc";

fn project() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../app/fixtures/chat-continued/claude/projects/-Users-me-work-notes")
}

fn tick() {
    std::thread::sleep(Duration::from_millis(30));
}

fn user_texts(read: &Conversation) -> Vec<String> {
    read.items
        .iter()
        .filter_map(|item| match &item.kind {
            ItemKind::User { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn texts_of(path: &Path) -> Vec<String> {
    user_texts(&conversation::read_shared(&s(path)))
}

fn said(id: &str, text: &str) -> String {
    format!(
        r#"{{"type":"user","timestamp":"2026-11-13T07:20:00.000Z","sessionId":"{id}","message":{{"role":"user","content":"{text}"}}}}"#
    ) + "\n"
}

fn replied(id: &str, text: &str, tool: Option<&str>) -> String {
    let call = tool.map_or_else(String::new, |t| {
        format!(r#",{{"type":"tool_use","id":"{t}","name":"Bash","input":{{"command":"ls"}}}}"#)
    });
    format!(
        r#"{{"type":"assistant","timestamp":"2026-11-13T07:20:04.000Z","sessionId":"{id}","message":{{"id":"m-{id}","model":"claude-opus-5-5","content":[{{"type":"text","text":"{text}"}}{call}],"usage":{{"input_tokens":5,"output_tokens":7}}}}}}"#
    ) + "\n"
}

fn continued(id: &str, to: &str) -> String {
    format!(
        r#"{{"type":"continued-in","timestamp":"2026-11-13T07:32:53.533Z","sessionId":"{id}","continuedInSessionId":"{to}"}}"#
    ) + "\n"
}

fn titled(id: &str) -> String {
    format!(r#"{{"type":"ai-title","aiTitle":"Title","sessionId":"{id}"}}"#) + "\n"
}

fn cleared(agent: &str) -> String {
    format!(r#"{{"type":"system","agentName":"{agent}","content":"SessionStart:clear"}}"#) + "\n"
}

fn named(agent: &str) -> String {
    format!(r#"{{"type":"agent-name","agentName":"{agent}"}}"#) + "\n"
}

#[test]
fn a_session_continued_in_another_shows_the_earlier_messages() {
    let new = project().join(format!("{NEW}.jsonl"));
    assert_eq!(
        texts_of(&new),
        [
            "Tidy the notes folder and tell me what changed.",
            "Now do the archive folder too."
        ],
        "the new transcript holds only a title and a name; the old one holds the conversation"
    );
    let old = project().join(format!("{OLD}.jsonl"));
    assert_eq!(
        texts_of(&old).len(),
        2,
        "the old transcript still reads on its own"
    );
    assert_eq!(earlier(&s(&old)), Vec::<String>::new());
}

#[test]
fn the_earlier_messages_come_first_with_their_own_ids_and_the_model_they_ran_with() {
    let new = conversation::read_shared(&s(&project().join(format!("{NEW}.jsonl"))));
    let old = conversation::read_shared(&s(&project().join(format!("{OLD}.jsonl"))));
    assert_eq!(new.items.len(), old.items.len());
    let ids: std::collections::HashSet<_> = new.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids.len(), new.items.len(), "every row id is its own");
    assert!(new.items[0].id.starts_with(OLD), "{}", new.items[0].id);
    assert_eq!(new.items[0].date, old.items[0].date);
    assert_eq!(new.facts.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(new.facts.branch.as_deref(), Some("notes"));
    assert_eq!(new.facts.turns, old.facts.turns);
    assert_eq!(new.facts.output_tokens, old.facts.output_tokens);
    assert!(Arc::ptr_eq(
        &conversation::read_shared(&s(&project().join(format!("{NEW}.jsonl")))),
        &new
    ));
}

#[test]
fn the_notch_snapshot_of_the_new_session_has_the_earlier_lines_and_last_prompt() {
    let new = snapshot::read(&s(&project().join(format!("{NEW}.jsonl"))));
    assert_eq!(
        new.last_prompt.as_deref(),
        Some("Now do the archive folder too.")
    );
    assert_eq!(
        new.lines.last().map(|l| l.text.as_str()),
        Some("Starting on the archive folder.")
    );
    assert_eq!(
        new.lines.iter().map(|l| l.id).collect::<Vec<_>>(),
        (0..new.lines.len()).collect::<Vec<_>>()
    );
    let old = snapshot::read(&s(&project().join(format!("{OLD}.jsonl"))));
    assert_eq!(new.lines, old.lines);
}

#[test]
fn a_new_session_that_says_something_keeps_its_own_last_prompt_after_the_old_lines() {
    let scratch = Scratch::new("own-prompt");
    scratch.write(
        "p/a.jsonl",
        &(said("a", "from a") + &replied("a", "a answers", None) + &continued("a", "b")),
    );
    let b = scratch.write(
        "p/b.jsonl",
        &(said("b", "from b") + &replied("b", "b answers", None)),
    );
    let read = snapshot::read(&s(&b));
    assert_eq!(read.last_prompt.as_deref(), Some("from b"));
    assert_eq!(
        read.lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>(),
        ["from a", "a answers", "from b", "b answers"]
    );
}

#[test]
fn a_chain_is_followed_back_to_its_first_session() {
    let scratch = Scratch::new("chain");
    scratch.write(
        "p/a.jsonl",
        &(said("a", "first") + &replied("a", "one", None) + &continued("a", "b")),
    );
    scratch.write(
        "p/b.jsonl",
        &(titled("b") + &said("b", "second") + &replied("b", "two", None) + &continued("b", "c")),
    );
    let c = scratch.write("p/c.jsonl", &(titled("c") + &said("c", "third")));
    assert_eq!(
        earlier(&s(&c)),
        [s(&scratch.path("p/a.jsonl")), s(&scratch.path("p/b.jsonl"))]
    );
    assert_eq!(texts_of(&c), ["first", "second", "third"]);
    let read = conversation::read_shared(&s(&c));
    assert_eq!(read.facts.output_tokens, 14, "two earlier replies of 7");
    assert_eq!(texts_of(&scratch.path("p/b.jsonl")), ["first", "second"]);
    assert_eq!(texts_of(&scratch.path("p/a.jsonl")), ["first"]);
}

#[test]
fn a_loop_or_a_link_to_itself_ends_the_chain() {
    let scratch = Scratch::new("loop");
    scratch.write(
        "p/a.jsonl",
        &(said("a", "from a") + &continued("a", "b") + &continued("a", "a")),
    );
    let b = scratch.write("p/b.jsonl", &(said("b", "from b") + &continued("b", "a")));
    assert_eq!(texts_of(&b), ["from a", "from b"]);
    assert_eq!(texts_of(&scratch.path("p/a.jsonl")), ["from b", "from a"]);
}

#[test]
fn a_continued_in_copied_from_another_session_is_not_a_link() {
    let scratch = Scratch::new("copied");
    scratch.write(
        "p/copy.jsonl",
        &(said("copy", "copied history") + &continued("someone-else", "b")),
    );
    let b = scratch.write("p/b.jsonl", &said("b", "mine"));
    assert_eq!(texts_of(&b), ["mine"]);
}

#[test]
fn a_clear_successor_and_a_continued_in_do_not_disturb_each_other() {
    let scratch = Scratch::new("clear-mixed");
    scratch.write(
        "p/w.jsonl",
        &(named("worker")
            + &said("w", "before the move")
            + &replied("w", "w answers", None)
            + &continued("w", "x")),
    );
    tick();
    let x = scratch.write(
        "p/x.jsonl",
        &(named("worker") + &said("x", "after the move")),
    );
    tick();
    let y = scratch.write(
        "p/y.jsonl",
        &(cleared("worker") + &said("y", "after the clear")),
    );
    let (x, y) = (s(&x), s(&y));
    assert_eq!(
        texts_of(Path::new(&x)),
        ["before the move", "after the move"]
    );
    assert_eq!(successor(&x, "worker"), Some(y.clone()));
    assert_eq!(predecessor(&y, "worker"), Some(x.clone()));
    assert_eq!(
        earlier(&y),
        Vec::<String>::new(),
        "a clear starts the conversation over"
    );
    assert_eq!(texts_of(Path::new(&y)), ["after the clear"]);
    assert_eq!(earlier(&x), [s(&scratch.path("p/w.jsonl"))]);
}

#[test]
fn a_continued_in_naming_a_missing_file_leaves_everything_readable() {
    let scratch = Scratch::new("missing");
    let a = scratch.write("p/a.jsonl", &(said("a", "alone") + &continued("a", "gone")));
    assert_eq!(texts_of(&a), ["alone"]);
    let gone = scratch.path("p/gone.jsonl");
    assert_eq!(earlier(&s(&gone)), [s(&a)]);
    assert_eq!(
        texts_of(&gone),
        ["alone"],
        "the earlier messages show until the new file exists"
    );
    scratch.write("p/gone.jsonl", &titled("gone"));
    assert_eq!(texts_of(&gone), ["alone"]);
    let nowhere = scratch.path("nowhere/x.jsonl");
    assert_eq!(earlier(&s(&nowhere)), Vec::<String>::new());
    assert_eq!(texts_of(&nowhere), Vec::<String>::new());
}

#[test]
fn a_predecessor_that_was_deleted_ends_the_chain_at_what_is_left() {
    let scratch = Scratch::new("deleted");
    scratch.write("p/a.jsonl", &(said("a", "first") + &continued("a", "b")));
    scratch.write("p/b.jsonl", &(said("b", "second") + &continued("b", "c")));
    let c = scratch.write("p/c.jsonl", &said("c", "third"));
    assert_eq!(texts_of(&c), ["first", "second", "third"]);
    tick();
    std::fs::remove_file(scratch.path("p/a.jsonl")).expect("remove");
    assert_eq!(texts_of(&c), ["second", "third"]);
}

#[test]
fn tool_calls_of_an_earlier_session_are_never_shown_as_still_running() {
    let scratch = Scratch::new("tools");
    scratch.write(
        "p/a.jsonl",
        &(said("a", "go") + &replied("a", "running", Some("t1")) + &continued("a", "b")),
    );
    let b = scratch.write("p/b.jsonl", &said("b", "next"));
    let read = conversation::read_shared(&s(&b));
    let calls: Vec<_> = read
        .items
        .iter()
        .filter_map(|i| match &i.kind {
            ItemKind::Tools(calls) => Some(calls.clone()),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].finished);
    let own = conversation::read_shared(&s(&scratch.path("p/a.jsonl")));
    assert!(own
        .items
        .iter()
        .any(|i| matches!(&i.kind, ItemKind::Tools(c) if c.iter().any(|c| !c.finished))));
}

#[test]
fn a_continued_in_written_after_the_new_file_exists_is_found() {
    let scratch = Scratch::new("late");
    let a = scratch.write("p/a.jsonl", &said("a", "history"));
    let b = scratch.write("p/b.jsonl", &said("b", "now"));
    assert_eq!(texts_of(&b), ["now"]);
    scratch.append("p/a.jsonl", &continued("a", "b"));
    assert_eq!(texts_of(&b), ["history", "now"]);
    assert_eq!(earlier(&s(&b)), [s(&a)]);
}
