//! Chat and the notch find a session's transcript with the same function (`notchling_app::transcripts`):
//! a terminal session has no `linkScanPath`, so its file is found by session id in the project
//! folders, and a job that ran `/clear` is followed to the file `/clear` started. The data is built
//! in a scratch folder laid out as a fixture root (`claude`, `notchling`, `codex`); nothing reads the
//! real `~/.claude`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use notchling_app::chat::data::DataRoot;
use notchling_app::chat::rows::{rows, Row};
use notchling_app::notch::source::{DataRoot as NotchRoot, Source};

/// The fixture's day; its rows are dated a few hours earlier.
const NOW_MS: i64 = 1_794_182_400_000;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fixture root in the temp folder, one per test (they run at the same time).
struct World(PathBuf);

impl World {
    fn new() -> World {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("notchling-chat-locate-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("make the scratch root");
        World(dir)
    }

    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("make the folder");
        std::fs::write(&path, text).expect("write the file");
        path
    }

    /// A terminal session's file, `sessions/<pid>.json`: the pid is this process's, so it is alive
    /// wherever the tests run.
    fn terminal_session(&self, id: &str, cwd: &str) {
        let pid = std::process::id();
        self.write(
            &format!("claude/sessions/{pid}.json"),
            &format!(
                r#"{{"sessionId":"{id}","pid":{pid},"cwd":"{cwd}","status":"idle","name":"terminal one","startedAt":1794160000000}}"#
            ),
        );
    }

    fn job(&self, short: &str, session: &str, name: &str, link: &Path) {
        self.write(
            &format!("claude/jobs/{short}/state.json"),
            &format!(
                r#"{{"state":"working","sessionId":"{session}","name":"{name}","cwd":"/work/app","updatedAt":"2026-11-08T08:00:00.000Z","linkScanPath":{link:?}}}"#
            ),
        );
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn user(text: &str) -> String {
    format!(
        r#"{{"type":"user","timestamp":"2026-11-08T08:00:00.000Z","message":{{"role":"user","content":"{text}"}}}}"#
    ) + "\n"
}

fn cleared(name: &str) -> String {
    format!(r#"{{"type":"system","agentName":"{name}","content":"SessionStart:clear"}}"#) + "\n"
}

fn has_user_text(c: &notchling_engine::transcript::model::Conversation, wanted: &str) -> bool {
    rows(c)
        .iter()
        .any(|r| matches!(r, Row::User { text, .. } if text == wanted))
}

#[test]
fn a_terminal_session_without_a_link_path_finds_its_transcript_by_id() {
    let w = World::new();
    w.terminal_session("term-1", "/work/app");
    let transcript = w.write(
        "claude/projects/-work-app/term-1.jsonl",
        &user("typed in the terminal"),
    );

    let root = DataRoot::fixture(&w.0);
    let sessions = root.sessions(NOW_MS);
    let session = sessions
        .iter()
        .find(|s| s.agent.id == "term-1")
        .expect("the terminal session is listed");
    assert_eq!(session.agent.transcript_path, None, "no link path to go by");
    assert_eq!(session.transcript.as_deref(), Some(transcript.as_path()));
    assert!(has_user_text(
        &root.conversation(session),
        "typed in the terminal"
    ));

    // The notch finds the same file.
    let loaded = Source::keeping_everything(NotchRoot::fixture(&w.0)).load(NOW_MS);
    let snapshot = &loaded.snapshots["term-1"];
    assert_eq!(
        snapshot.last_prompt.as_deref(),
        Some("typed in the terminal")
    );
}

#[test]
fn a_job_after_clear_shows_the_file_clear_started() {
    let w = World::new();
    let old = w.write(
        "claude/projects/-work-app/job-old.jsonl",
        &user("before clear"),
    );
    let new = w.write(
        "claude/projects/-work-app/job-new.jsonl",
        &(cleared("worker") + &user("after clear")),
    );
    w.job("ab12cd34", "job-old", "worker", &old);

    let root = DataRoot::fixture(&w.0);
    let sessions = root.sessions(NOW_MS);
    let session = sessions
        .iter()
        .find(|s| s.agent.id == "job-old")
        .expect("the job is listed");
    assert_eq!(session.transcript.as_deref(), Some(new.as_path()));
    let conversation = root.conversation(session);
    assert!(has_user_text(&conversation, "after clear"));
    assert!(!has_user_text(&conversation, "before clear"));

    let loaded = Source::keeping_everything(NotchRoot::fixture(&w.0)).load(NOW_MS);
    assert_eq!(
        loaded.snapshots["job-old"].last_prompt.as_deref(),
        Some("after clear")
    );
}
