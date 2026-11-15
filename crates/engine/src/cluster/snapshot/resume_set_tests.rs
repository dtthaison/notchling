use super::resume_set::{resume_set, shaped};
use crate::cluster::id::Tool;
use crate::cluster::main_sessions::MainSession;
use crate::sessions::testutil::TempDir;
use std::path::PathBuf;

fn main_session(id: &str, tool: Tool, transcript: Option<PathBuf>) -> MainSession {
    MainSession {
        id: id.to_string(),
        tool,
        cwd: "/work/repo".to_string(),
        config_dir: None,
        transcript: transcript.map(|p| p.to_string_lossy().into_owned()),
        respawn: Vec::new(),
        since_ms: 1,
    }
}

fn said(id: &str) -> String {
    format!(r#"{{"type":"user","sessionId":"{id}","message":{{"role":"user","content":"hi"}}}}"#)
        + "\n"
}

fn continued(id: &str, to: &str) -> String {
    format!(
        r#"{{"type":"continued-in","timestamp":"2026-11-13T07:32:53.533Z","sessionId":"{id}","continuedInSessionId":"{to}"}}"#
    ) + "\n"
}

#[test]
fn resume_set_for_claude_lists_its_files() {
    let home = TempDir::new("resume-claude");
    let transcript = home.write(".claude/projects/-work-repo/abc.jsonl", &said("abc"));
    home.write(".claude/todos/abc-agent-abc.json", "[]");
    home.write(".claude/todos/abc-other.json", "[]");
    home.write(".claude/todos/abd-agent-abd.json", "[]");
    home.write(".claude/todos/abcd-agent.json", "[]");
    home.write(".claude/todos/abc-notes.txt", "x");
    let set = resume_set(
        &main_session("abc", Tool::Claude, Some(transcript.clone())),
        home.root(),
    );
    assert_eq!(set.session, "abc");
    assert_eq!(set.tool, Tool::Claude);
    assert_eq!(set.transcripts, vec![transcript]);
    assert_eq!(
        set.dirs,
        vec![
            home.path(".claude/projects/-work-repo/abc"),
            home.path(".claude/file-history/abc"),
        ]
    );
    assert_eq!(
        set.files,
        vec![
            home.path(".claude/todos/abc-agent-abc.json"),
            home.path(".claude/todos/abc-other.json"),
        ]
    );
}

#[test]
fn every_path_the_table_lists_has_the_shape_a_restore_accepts() {
    let home = TempDir::new("resume-shaped");
    let transcript = home.write(".claude/projects/-work-repo/abc.jsonl", &said("abc"));
    home.write(".claude/todos/abc-agent-abc.json", "[]");
    let set = resume_set(
        &main_session("abc", Tool::Claude, Some(transcript)),
        home.root(),
    );
    for path in &set.transcripts {
        assert!(
            shaped(Tool::Claude, "abc", home.root(), path, true),
            "{path:?}"
        );
    }
    for path in set.files.iter().chain(&set.dirs) {
        assert!(
            shaped(Tool::Claude, "abc", home.root(), path, false),
            "{path:?}"
        );
    }
    let rollout = home.write(".codex/sessions/2026/11/15/rollout-t-abc.jsonl", "{}\n");
    let set = resume_set(
        &main_session("codex:abc", Tool::Codex, Some(rollout)),
        home.root(),
    );
    for path in &set.transcripts {
        assert!(
            shaped(Tool::Codex, "codex:abc", home.root(), path, true),
            "{path:?}"
        );
    }
    for path in &set.files {
        assert!(
            shaped(Tool::Codex, "codex:abc", home.root(), path, false),
            "{path:?}"
        );
    }
    assert!(!shaped(
        Tool::Claude,
        "abc",
        home.root(),
        &home.path(".zshrc"),
        false
    ));
}

#[test]
fn resume_set_uses_the_sessions_own_config_folder() {
    let home = TempDir::new("resume-lane");
    let lane = home.path("lanes/work");
    let transcript = home.write("lanes/work/projects/-p/abc.jsonl", &said("abc"));
    home.write("lanes/work/todos/abc-agent.json", "[]");
    let mut m = main_session("abc", Tool::Claude, Some(transcript));
    m.config_dir = Some(lane.to_string_lossy().into_owned());
    let set = resume_set(&m, home.root());
    assert_eq!(set.dirs[1], lane.join("file-history/abc"));
    assert_eq!(set.files, vec![lane.join("todos/abc-agent.json")]);
}

#[test]
fn resume_set_without_a_transcript_still_lists_what_it_can() {
    let home = TempDir::new("resume-terminal");
    home.write(".claude/todos/t1-agent.json", "[]");
    let set = resume_set(&main_session("t1", Tool::Claude, None), home.root());
    assert!(set.transcripts.is_empty());
    assert_eq!(set.dirs, vec![home.path(".claude/file-history/t1")]);
    assert_eq!(set.files, vec![home.path(".claude/todos/t1-agent.json")]);
}

#[test]
fn resume_set_follows_continued_in() {
    let home = TempDir::new("resume-continued");
    let a = home.write(
        ".claude/projects/-p/a.jsonl",
        &(said("a") + &continued("a", "b")),
    );
    let b = home.write(
        ".claude/projects/-p/b.jsonl",
        &(said("b") + &continued("b", "c")),
    );
    let c = home.write(".claude/projects/-p/c.jsonl", &said("c"));
    let set = resume_set(
        &main_session("c", Tool::Claude, Some(c.clone())),
        home.root(),
    );
    assert_eq!(set.transcripts, vec![a, b, c]);
}

#[test]
fn resume_set_for_codex() {
    let home = TempDir::new("resume-codex");
    let rollout = home.write(
        ".codex/sessions/2026/11/14/rollout-2026-11-14T10-00-00-5f0c.jsonl",
        "{}\n",
    );
    let set = resume_set(
        &main_session("codex:5f0c", Tool::Codex, Some(rollout.clone())),
        home.root(),
    );
    assert_eq!(set.tool, Tool::Codex);
    assert_eq!(set.transcripts, vec![rollout]);
    assert_eq!(set.files, vec![home.path(".codex/session_index.jsonl")]);
    assert!(set.dirs.is_empty());
    let none = resume_set(&main_session("codex:x", Tool::Codex, None), home.root());
    assert!(none.transcripts.is_empty());
    assert!(none.files.is_empty());
}

#[test]
fn other_tools_have_no_resume_set() {
    let home = TempDir::new("resume-agy");
    let set = resume_set(&main_session("agy:1", Tool::Agy, None), home.root());
    assert!(set.transcripts.is_empty() && set.files.is_empty() && set.dirs.is_empty());
}
