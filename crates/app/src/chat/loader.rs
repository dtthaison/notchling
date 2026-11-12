//! Reading a data root again and again for Chat ([`Loader`]): the sessions, with the daemon's
//! live state over the other tools' rows, and the selected session's conversation. Keeps what
//! polls need between reads; owns no view state, so a view sends it to the background executor
//! and takes it back.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use notchling_engine::sessions::{Agent, Monitor};
use notchling_engine::tools::agy::elsewhere;
use notchling_engine::transcript::codex;
use notchling_engine::transcript::model::Conversation;
use notchling_engine::transcript::snapshot::PendingQuestion;
use notchling_engine::transcript::subagents::{self, Origin, Subagent};

use super::data::{DataRoot, Session, Tool};
use super::log::{self, Change};
use crate::notch::requests::{self, HookRequest};
use crate::tools::foreign::AgyList;
use crate::tools::live::ToolLive;
use crate::transcripts::Locator;

/// One read of everything Chat shows.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub sessions: Vec<Session>,
    /// The session the conversation is of: the one asked for if it is listed, else the first.
    pub selected: Option<String>,
    pub conversation: Arc<Conversation>,
    /// What changed from the conversation this loader handed out last, when that was the same
    /// session's: the conversation the change starts from, and the change. `None` for the first
    /// load, another session or a draft: the view then compares the conversations itself.
    pub change: Option<(Arc<Conversation>, Change)>,
    pub subagents: Vec<Subagent>,
    /// The Codex and Antigravity rows that are jobs another session handed over: the Sidebar leaves
    /// them out (Search still finds them, and opening one from there brings it back).
    pub delegated: HashSet<String>,
    /// The transcript of the subagent Chat has open ([`Loader::subagent_path`]), by that path, read
    /// again with every load so a running one follows.
    pub subagent: Option<(String, Arc<Conversation>)>,
    /// The selected Claude Code session's unanswered AskUserQuestion, from its transcript tail.
    pub question: Option<PendingQuestion>,
    /// The Swift hook's held prompts by session id (`requests/`), read only while the daemon's
    /// aren't followed ([`Loader::files_prompts`]).
    pub requests: HashMap<String, HookRequest>,
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
    /// Read the Swift hook's `requests/` folder too: set while the daemon's held prompts aren't
    /// followed (no daemon, or this isn't the app tier), as the notch does.
    pub files_prompts: bool,
    /// The file of the subagent Chat shows in place of the conversation, read with each load, and
    /// whose it is (a job handed to Codex or Antigravity is read by that tool's reader).
    pub subagent_path: Option<String>,
    pub subagent_origin: Origin,
    /// Antigravity's log as last read, by its path and the file's size and date, so a log that
    /// hasn't changed gives the same conversation (and no redraw).
    agy_log: Option<AgyLog>,
    /// The session and conversation of the last load, what the next one's change starts from.
    last: Option<(String, Arc<Conversation>)>,
}

impl Loader {
    pub fn new(root: DataRoot) -> Loader {
        Loader {
            monitor: root.new_monitor(),
            locator: root.new_locator(),
            agy: root.new_agy_list(),
            files_prompts: false,
            subagent_path: None,
            subagent_origin: Origin::Claude,
            agy_log: None,
            last: None,
            root,
        }
    }

    /// The Antigravity list this loader reads (a test checks it is the notch's too).
    #[cfg(test)]
    pub fn agy_list(&self) -> &AgyList {
        &self.agy
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
        let requests = if self.files_prompts {
            requests::read_dir(&self.root.data_dir.join("requests"), now_ms as f64 / 1000.0)
        } else {
            HashMap::new()
        };
        let delegations = self.root.delegations();
        let delegated = super::delegated::hidden(&delegations);
        let (conversation, subagents) = match session {
            Some(session) => {
                let conversation = self.root.conversation_shared(session);
                let subagents = self.root.subagents(
                    session,
                    &conversation,
                    &delegations,
                    now_ms as f64 / 1000.0,
                );
                (conversation, subagents)
            }
            None => (Arc::default(), Vec::new()),
        };
        // The open question comes from the same read as the conversation, not a second one.
        let question = session
            .filter(|s| s.tool == Tool::Claude && s.agent.command.is_none())
            .and_then(|_| conversation.pending_question.clone());
        // Compared here, off the main thread: the view only checks the pointer it starts from.
        let change = match (&selected, self.last.take()) {
            (Some(id), Some((last_id, last))) if *id == last_id => {
                let change = log::diff(&last, &conversation);
                Some((last, change))
            }
            _ => None,
        };
        self.last = selected
            .as_ref()
            .map(|id| (id.clone(), Arc::clone(&conversation)));
        let subagent = self
            .subagent_path
            .clone()
            .filter(|path| !path.is_empty())
            .map(|path| {
                let read = self.read_subagent(&path);
                (path, read)
            });
        Loaded {
            sessions,
            selected,
            conversation,
            change,
            subagents,
            delegated,
            subagent,
            question,
            requests,
            now_ms,
        }
    }
}

/// A log's last read.
struct AgyLog {
    path: String,
    stamp: Option<(u64, std::time::SystemTime)>,
    conversation: Arc<Conversation>,
}

impl Loader {
    /// The open subagent's transcript, by whose it is.
    fn read_subagent(&mut self, path: &str) -> Arc<Conversation> {
        match self.subagent_origin {
            Origin::Claude => subagents::read_shared(path),
            Origin::Codex => codex::read_shared(path),
            Origin::Antigravity => {
                let stamp = std::fs::metadata(path)
                    .ok()
                    .and_then(|m| Some((m.len(), m.modified().ok()?)));
                if let Some(last) = &self.agy_log {
                    if last.path == path && last.stamp == stamp {
                        return last.conversation.clone();
                    }
                }
                let (conversation, _) = elsewhere::from_log_file(Path::new(path), None);
                let conversation = Arc::new(conversation);
                self.agy_log = Some(AgyLog {
                    path: path.to_string(),
                    stamp,
                    conversation: conversation.clone(),
                });
                conversation
            }
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

    #[test]
    fn an_open_subagent_is_read_with_each_load_and_none_when_nothing_is_open() {
        let s = crate::testutil::Scratch::new("loader-subagent");
        let session = terminal_world(&s);
        let line = "{\"type\":\"user\",\"isSidechain\":true,\"timestamp\":\"2026-11-08T08:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"look around\"}}\n";
        let file = s.write(
            "claude/projects/-work-app/term-1/subagents/agent-a1.jsonl",
            line,
        );
        assert!(session.exists());
        let mut loader = Loader::new(DataRoot::fixture(s.path()));

        assert!(loader.load(None, 1_000).subagent.is_none());

        let path = file.to_string_lossy().into_owned();
        loader.subagent_path = Some(path.clone());
        let loaded = loader.load(None, 2_000);
        let (read_path, conversation) = loaded.subagent.expect("the open subagent is read");
        assert_eq!(read_path, path);
        assert_eq!(conversation.items.len(), 1);
        // And it is listed, so the header has its row.
        assert_eq!(loaded.subagents.len(), 1);
    }

    /// A job `term-1` handed to Codex, with its rollout in the fixture's Codex folder.
    fn handed_to_codex(s: &crate::testutil::Scratch) -> std::path::PathBuf {
        use notchling_engine::delegate::options::{Options, Tool as DelegateTool};
        use notchling_engine::delegate::record::{self, Delegation};
        let options = Options {
            tool: DelegateTool::Codex,
            prompt: "Review the diff".into(),
            model: Some("gpt-5.5".into()),
            effort: Some("high".into()),
            folder: "/work/app".into(),
            out: None,
            write: false,
            session: None,
            timeout: std::time::Duration::from_secs(30),
        };
        let mut job = Delegation::new(
            "term-1",
            "/work/app",
            &options,
            1_794_160_100.0,
            std::process::id(),
        );
        job.session = Some("c1".into());
        record::write(&s.path().join("notchling").join(record::FOLDER), &job)
            .expect("write record");
        s.write(
            "codex/sessions/2026/11/12/rollout-2026-11-12T09-00-00-c1.jsonl",
            concat!(
                r#"{"timestamp":"2026-11-12T10:00:00.000Z","type":"session_meta","payload":{"id":"c1","cwd":"/work/app"}}"#,
                "\n",
                r#"{"timestamp":"2026-11-12T10:00:01.000Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Review the diff"}]}}"#,
                "\n",
            ),
        )
    }

    #[test]
    fn a_job_handed_to_codex_is_a_subagent_of_its_session_and_not_a_sidebar_row() {
        let s = crate::testutil::Scratch::new("loader-delegated");
        terminal_world(&s);
        let rollout = handed_to_codex(&s);
        let mut loader = Loader::new(DataRoot::fixture(s.path()));
        let loaded = loader.load(Some("term-1"), 1_794_160_200_000);

        let jobs: Vec<_> = loaded
            .subagents
            .iter()
            .filter(|a| a.origin == Origin::Codex)
            .collect();
        assert_eq!(jobs.len(), 1, "{:?}", loaded.subagents);
        assert_eq!(jobs[0].description, "Review the diff");
        assert_eq!(jobs[0].path, rollout.to_string_lossy());
        assert!(loaded.delegated.contains("codex:c1"));
        // The row is still in the loader's list (Search opens it from there); the view leaves it out.
        assert!(loaded.sessions.iter().any(|x| x.agent.id == "codex:c1"));

        // Another session shows no jobs of this one.
        let other = loaded
            .sessions
            .iter()
            .find(|x| x.agent.id == "codex:c1")
            .map(|x| x.agent.id.clone());
        let on_codex = loader.load(other.as_deref(), 1_794_160_200_000);
        assert!(on_codex.subagents.is_empty());
    }

    #[test]
    fn an_open_job_is_read_by_its_tools_reader() {
        let s = crate::testutil::Scratch::new("loader-delegated-open");
        terminal_world(&s);
        let rollout = handed_to_codex(&s);
        let mut loader = Loader::new(DataRoot::fixture(s.path()));
        loader.subagent_path = Some(rollout.to_string_lossy().into_owned());
        loader.subagent_origin = Origin::Codex;
        let (_, conversation) = loader
            .load(Some("term-1"), 1_794_160_200_000)
            .subagent
            .expect("the open job is read");
        assert_eq!(conversation.items.len(), 1, "{:?}", conversation.items);

        // Antigravity's log is read through its own reader, and again only when it changes.
        let log = s.write(
            "agy/log.jsonl",
            "{\"type\":\"USER_INPUT\",\"step_index\":0,\"content\":{\"user_input\":\"<USER_REQUEST>hi</USER_REQUEST>\"}}\n",
        );
        loader.subagent_path = Some(log.to_string_lossy().into_owned());
        loader.subagent_origin = Origin::Antigravity;
        let first = loader
            .load(Some("term-1"), 1_794_160_201_000)
            .subagent
            .expect("read");
        let second = loader
            .load(Some("term-1"), 1_794_160_202_000)
            .subagent
            .expect("read");
        assert!(
            Arc::ptr_eq(&first.1, &second.1),
            "an unchanged log is the same conversation"
        );
    }
}
