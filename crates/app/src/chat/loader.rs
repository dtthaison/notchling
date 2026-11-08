//! Reading a data root again and again for Chat ([`Loader`]): the sessions, with the daemon's
//! live state over the other tools' rows, and the selected session's conversation. Keeps what
//! polls need between reads; owns no view state, so a view sends it to the background executor
//! and takes it back.

use notchling_engine::sessions::{Agent, Monitor};
use notchling_engine::transcript::model::Conversation;
use notchling_engine::transcript::subagents::Subagent;

use super::data::{DataRoot, Session, Tool};
use crate::tools::foreign::AgyList;
use crate::tools::live::ToolLive;
use crate::transcripts::Locator;

/// One read of everything Chat shows.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub sessions: Vec<Session>,
    /// The session the conversation is of: the one asked for if it is listed, else the first.
    pub selected: Option<String>,
    pub conversation: Conversation,
    pub subagents: Vec<Subagent>,
    pub now_ms: i64,
}

/// Reads a data root again and again, keeping what polls need between them: the monitor's memory of
/// ended terminal sessions (they show as Exited), which rows had no transcript, and the Antigravity
/// list's last read.
pub struct Loader {
    root: DataRoot,
    monitor: Monitor,
    locator: Locator,
    agy: AgyList,
}

impl Loader {
    pub fn new(root: DataRoot) -> Loader {
        Loader {
            monitor: root.new_monitor(),
            locator: root.new_locator(),
            agy: root.new_agy_list(),
            root,
        }
    }

    /// The sessions, and the conversation and subagents of `selected` (else the first session).
    /// `now_ms` is the clock.
    pub fn load(&mut self, selected: Option<&str>, now_ms: i64) -> Loaded {
        self.load_from(selected, None, now_ms)
    }

    /// [`Loader::load`] with Claude Code's rows from the daemon when it gave any.
    pub fn load_from(
        &mut self,
        selected: Option<&str>,
        claude: Option<Vec<Agent>>,
        now_ms: i64,
    ) -> Loaded {
        self.load_live(selected, claude, &ToolLive::default(), now_ms)
    }

    /// [`Loader::load_from`] with what the daemon said about the other tools laid over their rows
    /// (a running turn, a session it started that the files don't list yet, one deleted).
    pub fn load_live(
        &mut self,
        selected: Option<&str>,
        claude: Option<Vec<Agent>>,
        live: &ToolLive,
        now_ms: i64,
    ) -> Loaded {
        let listed = self.root.sessions_from(
            &mut self.monitor,
            &mut self.locator,
            &mut self.agy,
            claude,
            now_ms,
        );
        let mut agents: Vec<Agent> = listed.iter().map(|s| s.agent.clone()).collect();
        live.overlay(&mut agents);
        let sessions: Vec<Session> = agents
            .into_iter()
            .map(|agent| Session {
                transcript: listed
                    .iter()
                    .find(|s| s.agent.id == agent.id)
                    .and_then(|s| s.transcript.clone()),
                tool: Tool::of(&agent.id),
                agent,
            })
            .collect();
        let selected = selected
            .filter(|id| sessions.iter().any(|s| s.agent.id == *id))
            .map(str::to_owned)
            .or_else(|| sessions.first().map(|s| s.agent.id.clone()));
        let session = selected
            .as_deref()
            .and_then(|id| sessions.iter().find(|s| s.agent.id == id));
        let (conversation, subagents) = match session {
            Some(session) => {
                let conversation = self.root.conversation(session);
                let subagents = self
                    .root
                    .subagents(session, &conversation, now_ms as f64 / 1000.0);
                (conversation, subagents)
            }
            None => (Conversation::default(), Vec::new()),
        };
        Loaded {
            sessions,
            selected,
            conversation,
            subagents,
            now_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A fixture root with one live terminal session (this process's pid, so it is alive wherever
    /// the tests run) and its transcript; the returned path is the session's file.
    fn terminal_world(s: &crate::testutil::Scratch) -> PathBuf {
        s.write(
            "claude/projects/-work-app/term-1.jsonl",
            "{\"type\":\"user\",\"timestamp\":\"2026-11-08T08:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"hello\"}}\n",
        );
        let pid = std::process::id();
        s.write(
            &format!("claude/sessions/{pid}.json"),
            &format!(
                r#"{{"sessionId":"term-1","pid":{pid},"cwd":"/work/app","status":"idle","name":"terminal one","startedAt":1794160000000}}"#
            ),
        )
    }

    #[test]
    fn a_loader_keeps_its_monitor_so_an_ended_terminal_session_shows_as_exited() {
        let s = crate::testutil::Scratch::new("loader-exited");
        let file = terminal_world(&s);
        let mut loader = Loader::new(DataRoot::fixture(s.path()));

        let first = loader.load(None, 1_000);
        let row = first.sessions.iter().find(|r| r.agent.id == "term-1");
        assert_eq!(row.map(|r| r.agent.label.as_str()), Some("Idle"));

        // The terminal closes: its session file goes away.
        std::fs::remove_file(&file).unwrap();
        let second = loader.load(first.selected.as_deref(), 2_000);
        let row = second.sessions.iter().find(|r| r.agent.id == "term-1");
        assert_eq!(row.map(|r| r.agent.label.as_str()), Some("Exited"));
        // Its conversation is still readable by id.
        assert!(!second.conversation.items.is_empty());
    }

    #[test]
    fn a_loader_selects_the_asked_session_else_the_first_listed() {
        let s = crate::testutil::Scratch::new("loader-select");
        terminal_world(&s);
        let mut loader = Loader::new(DataRoot::fixture(s.path()));
        assert_eq!(loader.load(None, 1_000).selected.as_deref(), Some("term-1"));
        assert_eq!(
            loader.load(Some("gone"), 1_000).selected.as_deref(),
            Some("term-1")
        );
        assert_eq!(
            loader.load(Some("term-1"), 1_000).selected.as_deref(),
            Some("term-1")
        );
        let empty = crate::testutil::Scratch::new("loader-empty");
        let none = Loader::new(DataRoot::fixture(empty.path())).load(Some("x"), 1_000);
        assert_eq!(none.selected, None);
        assert!(none.sessions.is_empty() && none.conversation.items.is_empty());
    }

    #[test]
    fn a_live_row_the_files_do_not_list_is_selectable_with_its_tool() {
        use notchling_engine::sessions::{AgentGroup, AgentTarget};
        let s = crate::testutil::Scratch::new("loader-live");
        let mut loader = Loader::new(DataRoot::fixture(s.path()));
        let row = Agent {
            id: "agy:new-1".into(),
            name: "Edit hello".into(),
            project: "p".into(),
            cwd: "/w/p".into(),
            group: AgentGroup::Working,
            label: "Working".into(),
            detail: None,
            since_ms: 1_000,
            target: AgentTarget::Resume {
                session_id: "new-1".into(),
                cwd: "/w/p".into(),
            },
            suggested_reply: None,
            transcript_path: None,
            host: "Antigravity".into(),
            headless: false,
            config_dir: None,
            command: None,
            resume_session_id: None,
        };
        let mut live = ToolLive::default();
        live.apply(&notchling_local_api::Event {
            event: "session.changed".into(),
            data: serde_json::json!({"id": "agy:new-1", "row": row, "change": "turn_started"}),
        });
        let loaded = loader.load_live(Some("agy:new-1"), None, &live, 1_000);
        assert_eq!(loaded.selected.as_deref(), Some("agy:new-1"));
        let session = &loaded.sessions[0];
        assert_eq!(
            (session.tool, session.agent.group),
            (Tool::Antigravity, AgentGroup::Working)
        );
        assert!(
            loaded.conversation.items.is_empty(),
            "the daemon's, not the files'"
        );
    }
}
