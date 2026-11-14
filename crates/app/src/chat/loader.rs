use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use notchling_engine::contract::{Registry, Tool as SubTool};
use notchling_engine::sessions::{Agent, Monitor};
use notchling_engine::tools::agy::elsewhere;
use notchling_engine::tools::codex::children::{ChildSource, DayFolders};
use notchling_engine::transcript::codex;
use notchling_engine::transcript::model::Conversation;
use notchling_engine::transcript::snapshot::PendingQuestion;
use notchling_engine::transcript::subagents::{self, Origin, Subagent};
use notchling_local_api::SOCKET_NAME;

use super::codex_children::{DaemonChildren, Kept};
use super::data::{DataRoot, Session, Tool};
use super::inspector::SubagentRow;
use super::log::{self, Change};
use super::paging::{Counts, Tree};
use super::subagent_rows::{decorate, Disk, SubagentRows};
use crate::notch::requests::{self, HookRequest};
use crate::tools::foreign::AgyList;
use crate::tools::live::ToolLive;
use crate::transcripts::Locator;

#[derive(Clone, Debug)]
pub struct Loaded {
    pub sessions: Vec<Session>,
    pub selected: Option<String>,
    pub conversation: Arc<Conversation>,
    pub change: Option<(Arc<Conversation>, Change)>,
    pub subagents: Vec<Subagent>,
    pub subagent_rows: Vec<SubagentRow>,
    pub subagent_counts: Counts,
    pub delegated: HashSet<String>,
    pub subagent: Option<(String, Arc<Conversation>)>,
    pub question: Option<PendingQuestion>,
    pub requests: HashMap<String, HookRequest>,
    pub now_ms: i64,
    pub partial: bool,
}

pub struct Loader {
    root: DataRoot,
    monitor: Monitor,
    locator: Locator,
    agy: AgyList,
    pub files_prompts: bool,
    quick_switch: bool,
    pub subagent_path: Option<String>,
    pub subagent_origin: Origin,
    pub more_subagents: usize,
    pub open_groups: HashSet<String>,
    pub more_under: HashMap<String, usize>,
    tree: Tree,
    registry: Arc<Registry>,
    children: Arc<dyn ChildSource>,
    peeked: SubagentRows,
    agy_log: Option<AgyLog>,
    last: Option<(String, Arc<Conversation>)>,
}

impl Loader {
    pub fn new(root: DataRoot) -> Loader {
        let source: Arc<dyn ChildSource> = if root.fixture.is_some() {
            Arc::new(DayFolders)
        } else {
            Arc::new(DaemonChildren::new(Box::new(Kept::socket(
                root.data_dir.join(SOCKET_NAME),
                env!("CARGO_PKG_VERSION").to_string(),
            ))))
        };
        Loader::with_children(root, source)
    }

    pub fn with_children(root: DataRoot, source: Arc<dyn ChildSource>) -> Loader {
        let registry = Arc::new(Registry::with_sources(source.clone()));
        Loader {
            registry: registry.clone(),
            children: source,
            quick_switch: false,
            monitor: root.new_monitor(),
            locator: root.new_locator(),
            agy: root.new_agy_list(),
            files_prompts: false,
            subagent_path: None,
            subagent_origin: Origin::default(),
            more_subagents: 0,
            open_groups: HashSet::new(),
            more_under: HashMap::new(),
            tree: Tree::default(),
            peeked: SubagentRows::new(Disk(registry.clone())),
            agy_log: None,
            last: None,
            root,
        }
    }

    pub fn quick_on_switch(mut self) -> Loader {
        self.quick_switch = true;
        self
    }

    #[cfg(test)]
    pub fn agy_list(&self) -> &AgyList {
        &self.agy
    }

    pub fn load(&mut self, selected: Option<&str>, now_ms: i64) -> Loaded {
        self.load_from(selected, None, now_ms)
    }

    pub fn load_from(
        &mut self,
        selected: Option<&str>,
        claude: Option<Vec<Agent>>,
        now_ms: i64,
    ) -> Loaded {
        self.load_live(selected, claude, &ToolLive::default(), now_ms)
    }

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
        let switched = self.last.as_ref().map(|(id, _)| id) != selected.as_ref();
        let partial = self.quick_switch && switched && session.is_some();
        let conversation = session.map_or_else(Arc::default, |s| self.root.conversation_shared(s));
        let (subagents, subagent_rows, subagent_counts) = if partial {
            (Vec::new(), Vec::new(), Counts::default())
        } else {
            self.subagents_of(session, &selected, &conversation, now_ms)
        };
        let question = session
            .filter(|s| s.tool == Tool::Claude && s.agent.command.is_none())
            .and_then(|_| conversation.pending_question.clone());
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
            subagent_rows,
            subagent_counts,
            delegated,
            subagent,
            question,
            requests,
            now_ms,
            partial,
        }
    }

    fn subagents_of(
        &mut self,
        session: Option<&Session>,
        selected: &Option<String>,
        conversation: &Conversation,
        now_ms: i64,
    ) -> (Vec<Subagent>, Vec<SubagentRow>, Counts) {
        let subagents = session.map_or_else(Vec::new, |session| {
            let listed = self.root.subagents_in(
                &self.registry,
                session,
                conversation,
                now_ms as f64 / 1000.0,
            );
            self.children.done();
            listed
        });
        let mut more = std::mem::take(&mut self.more_under);
        more.insert(String::new(), std::mem::take(&mut self.more_subagents));
        let shown = self
            .tree
            .show(selected.as_deref(), &subagents, &self.open_groups, &more);
        let indexes: Vec<usize> = shown.entries.iter().map(|e| e.index).collect();
        let mut rows = self.peeked.rows(&subagents, &indexes);
        decorate(&mut rows, &shown);
        (subagents, rows, shown.counts)
    }
}

struct AgyLog {
    path: String,
    stamp: Option<(u64, std::time::SystemTime)>,
    conversation: Arc<Conversation>,
}

impl Loader {
    fn read_subagent(&mut self, path: &str) -> Arc<Conversation> {
        match self.subagent_origin.tool {
            SubTool::Claude => subagents::read_shared(path),
            SubTool::Codex => codex::read_shared(path),
            SubTool::Antigravity => {
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

        std::fs::remove_file(&file).unwrap();
        let second = loader.load(first.selected.as_deref(), 2_000);
        let row = second.sessions.iter().find(|r| r.agent.id == "term-1");
        assert_eq!(row.map(|r| r.agent.label.as_str()), Some("Exited"));
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
        assert_eq!(loaded.subagents.len(), 1);
    }

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
            .filter(|a| a.origin == Origin::delegated(SubTool::Codex))
            .collect();
        assert_eq!(jobs.len(), 1, "{:?}", loaded.subagents);
        assert_eq!(jobs[0].description, "Review the diff");
        assert_eq!(jobs[0].path, rollout.to_string_lossy());
        assert!(loaded.delegated.contains("codex:c1"));
        assert!(loaded.sessions.iter().any(|x| x.agent.id == "codex:c1"));

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
        loader.subagent_origin = Origin::delegated(SubTool::Codex);
        let (_, conversation) = loader
            .load(Some("term-1"), 1_794_160_200_000)
            .subagent
            .expect("the open job is read");
        assert_eq!(conversation.items.len(), 1, "{:?}", conversation.items);

        let log = s.write(
            "agy/log.jsonl",
            "{\"type\":\"USER_INPUT\",\"step_index\":0,\"content\":{\"user_input\":\"<USER_REQUEST>hi</USER_REQUEST>\"}}\n",
        );
        loader.subagent_path = Some(log.to_string_lossy().into_owned());
        loader.subagent_origin = Origin::delegated(SubTool::Antigravity);
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
