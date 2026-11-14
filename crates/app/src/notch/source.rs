use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notchling_engine::sessions::{Agent, Monitor, Probes, Roots};
use notchling_engine::settings::dirs::{data_dir, DirEnv};
use notchling_engine::transcript::codex;
use notchling_engine::transcript::model::Conversation;
use notchling_engine::transcript::snapshot::{self, Snapshot};

use super::requests::{self, HookRequest};
use crate::chat::codex_list;
use crate::tools::foreign::{self, AgyList};
use crate::tools::live::ToolLive;
use crate::tools::requests::RowTool;
use crate::transcripts::Locator;

pub const DATA_ROOT_ENV: &str = "NOTCHLING_DATA_ROOT";

pub const ANTIGRAVITY_DIR: &str = ".gemini/antigravity-cli";

pub const APP_DIR: &str = "app";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataRoot {
    pub claude_dir: PathBuf,
    pub data_dir: PathBuf,
    pub codex_dir: PathBuf,
    pub antigravity_dir: PathBuf,
    pub fixture: Option<PathBuf>,
}

impl DataRoot {
    pub fn real() -> Option<DataRoot> {
        let env = DirEnv::from_process();
        let home = env.home.as_ref()?;
        Some(DataRoot {
            claude_dir: home.join(".claude"),
            data_dir: data_dir(&env)?,
            codex_dir: home.join(".codex"),
            antigravity_dir: home.join(ANTIGRAVITY_DIR),
            fixture: None,
        })
    }

    pub fn fixture(root: &Path) -> DataRoot {
        DataRoot {
            claude_dir: root.join("claude"),
            data_dir: root.join("notchling"),
            codex_dir: root.join("codex"),
            antigravity_dir: root.join("antigravity-cli"),
            fixture: Some(root.to_path_buf()),
        }
    }

    pub fn antigravity_copy_dir(&self) -> PathBuf {
        self.data_dir.join(APP_DIR).join("antigravity")
    }

    pub fn for_render() -> DataRoot {
        let root = std::env::var_os(DATA_ROOT_ENV)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/notch"));
        DataRoot::fixture(&root)
    }

    pub fn roots(&self) -> Roots {
        Roots {
            main_dir: self.claude_dir.clone(),
            lanes_file: self.data_dir.join("lanes.json"),
        }
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.claude_dir.join("projects")
    }

    pub fn requests_dir(&self) -> PathBuf {
        self.data_dir.join("requests")
    }
}

#[derive(Clone, Debug, Default)]
pub struct Loaded {
    pub agents: Vec<Agent>,
    pub snapshots: HashMap<String, Snapshot>,
    pub requests: HashMap<String, HookRequest>,
}

pub type AgyReader = Arc<dyn Fn(&str) -> Option<Conversation> + Send + Sync>;

pub struct Source {
    pub root: DataRoot,
    monitor: Monitor,
    locator: Locator,
    agy_list: AgyList,
    pub agy_reader: Option<AgyReader>,
}

impl Source {
    pub fn new(root: DataRoot) -> Source {
        Source {
            locator: Locator::new(root.fixture.clone()),
            agy_list: AgyList::shared(root.antigravity_dir.clone(), root.antigravity_copy_dir()),
            root,
            monitor: Monitor::default(),
            agy_reader: None,
        }
    }

    #[cfg(test)]
    pub fn agy_list(&self) -> &AgyList {
        &self.agy_list
    }

    pub fn keeping_everything(root: DataRoot) -> Source {
        let mut source = Source::new(root);
        source.monitor.completed_window_ms = i64::MAX / 4;
        source
    }

    pub fn load(&mut self, now_ms: i64) -> Loaded {
        self.load_from(None, None, now_ms)
    }

    pub fn load_from(
        &mut self,
        claude: Option<Vec<Agent>>,
        prompts: Option<Vec<serde_json::Value>>,
        now_ms: i64,
    ) -> Loaded {
        self.load_live(claude, prompts, &ToolLive::default(), now_ms)
    }

    pub fn load_live(
        &mut self,
        claude: Option<Vec<Agent>>,
        prompts: Option<Vec<serde_json::Value>>,
        live: &ToolLive,
        now_ms: i64,
    ) -> Loaded {
        let mut agents = match claude {
            Some(agents) => agents,
            None => self
                .monitor
                .poll(&self.root.roots(), &Probes::from_proc(), now_ms),
        };
        let projects = self.root.projects_dir();
        let mut snapshots = HashMap::new();
        for agent in &agents {
            if agent.command.is_some() {
                continue;
            }
            if let Some(path) = self.locator.find(agent, &projects, now_ms) {
                snapshots.insert(agent.id.clone(), snapshot::read(&path.to_string_lossy()));
            }
        }
        self.locator.retain(&agents);
        for agent in self.foreign_rows(live, now_ms) {
            let conversation = match RowTool::of(&agent.id) {
                RowTool::Codex => agent.transcript_path.as_deref().map(codex::read),
                RowTool::Agy if live.is_open(&agent.id) => {
                    self.agy_reader.as_ref().and_then(|read| read(&agent.id))
                }
                _ => None,
            };
            if let Some(conversation) = conversation {
                snapshots.insert(agent.id.clone(), foreign::snapshot(&conversation));
            }
            agents.push(agent);
        }
        let requests = match prompts {
            Some(prompts) => requests::from_daemon(&prompts, &agents),
            None => requests::read_dir(&self.root.requests_dir(), now_ms as f64 / 1000.0),
        };
        Loaded {
            agents,
            snapshots,
            requests,
        }
    }

    fn foreign_rows(&mut self, live: &ToolLive, now_ms: i64) -> Vec<Agent> {
        let days = self.root.fixture.is_none().then_some(14);
        let sessions = self.root.codex_dir.join("sessions");
        let mut rows: Vec<Agent> = codex_list::rollouts(&sessions, days, now_ms, 30)
            .iter()
            .filter_map(|path| codex_list::session(path))
            .collect();
        rows.extend(self.agy_list.rows(now_ms));
        let delegated =
            crate::chat::delegated::hidden(&crate::chat::delegated::read(&self.root.data_dir));
        rows.retain(|row| !delegated.contains(&row.id));
        live.overlay(&mut rows);
        foreign::visible(rows, now_ms)
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn fixture_clock(agents: &[Agent]) -> i64 {
    agents.iter().map(|a| a.since_ms).max().unwrap_or(0) + 60_000
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::sessions::AgentGroup;

    fn fixture() -> DataRoot {
        DataRoot::fixture(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/notch"))
    }

    fn loaded() -> Loaded {
        let mut source = Source::keeping_everything(fixture());
        let first = source.load(0);
        let now = fixture_clock(&first.agents);
        source.load(now)
    }

    #[test]
    fn the_fixture_has_a_row_in_every_tab() {
        let l = loaded();
        for group in [
            AgentGroup::NeedsInput,
            AgentGroup::Working,
            AgentGroup::Completed,
        ] {
            assert!(l.agents.iter().any(|a| a.group == group), "{group:?}");
        }
    }

    #[test]
    fn snapshots_are_found_by_session_id_without_a_link_path() {
        let l = loaded();
        let working = l
            .agents
            .iter()
            .find(|a| a.group == AgentGroup::Working)
            .unwrap();
        let snap = &l.snapshots[&working.id];
        assert!(!snap.lines.is_empty());
        assert!(snap.last_prompt.is_some());
    }

    #[test]
    fn the_fixture_holds_a_question_and_a_permission() {
        let l = loaded();
        assert!(l.snapshots.values().any(|s| s.question.is_some()));
        assert!(l
            .requests
            .values()
            .any(|r| r.kind == super::super::requests::RequestKind::Permission));
    }

    #[test]
    fn render_roots_never_name_the_real_home() {
        let root = DataRoot::for_render();
        assert!(!root.claude_dir.ends_with(".claude"));
        assert!(!root.codex_dir.ends_with(".codex"));
        assert!(!root.antigravity_dir.ends_with(".gemini/antigravity-cli"));
        assert_eq!(root.roots().main_dir, root.claude_dir);
        assert_eq!(root.requests_dir(), root.data_dir.join("requests"));
        assert_eq!(
            root.antigravity_copy_dir(),
            root.data_dir.join("app").join("antigravity")
        );
    }

    #[test]
    fn the_other_tools_rows_follow_the_notch_fixture_and_the_live_state() {
        use crate::tools::live::ToolLive;
        use notchling_engine::sessions::AgentTarget;
        let mut plain = Source::keeping_everything(fixture());
        let now = fixture_clock(&plain.load(0).agents);
        assert!(plain
            .load(now)
            .agents
            .iter()
            .all(|a| !a.id.starts_with("codex:") && !a.id.starts_with("agy:")));
        let mut live = ToolLive::default();
        let row = Agent {
            id: "agy:live-1".into(),
            name: "Edit hello".into(),
            project: "p".into(),
            cwd: "/w/p".into(),
            group: AgentGroup::Working,
            label: "Working".into(),
            detail: None,
            since_ms: now,
            target: AgentTarget::Resume {
                session_id: "live-1".into(),
                cwd: "/w/p".into(),
            },
            suggested_reply: None,
            transcript_path: None,
            host: "Antigravity".into(),
            headless: false,
            config_dir: None,
            command: None,
            resume_session_id: None,
            at_prompt: false,
        };
        live.apply(&notchling_local_api::Event {
            event: "session.changed".into(),
            data: serde_json::json!({"id": "agy:live-1", "row": row, "change": "turn_started"}),
        });
        plain.agy_reader = Some(Arc::new(|id: &str| {
            use notchling_engine::transcript::model::{ChatItem, ItemKind};
            (id == "agy:live-1").then(|| Conversation {
                items: vec![ChatItem {
                    id: "u".into(),
                    kind: ItemKind::user("Change hello.txt"),
                    date: None,
                }],
                ..Conversation::default()
            })
        }));
        let loaded = plain.load_live(None, None, &live, now);
        let agy = loaded
            .agents
            .iter()
            .find(|a| a.id == "agy:live-1")
            .expect("the daemon's row");
        assert_eq!(agy.group, AgentGroup::Working);
        assert_eq!(
            loaded.snapshots["agy:live-1"].last_prompt.as_deref(),
            Some("Change hello.txt")
        );
    }

    #[test]
    fn a_row_without_a_transcript_is_not_searched_again_at_once() {
        let mut source = Source::new(fixture());
        let mut ghost = notchling_engine::sessions::Agent {
            id: "no-such-session".into(),
            ..loaded().agents[0].clone()
        };
        ghost.transcript_path = None;
        let projects = source.root.projects_dir();
        assert_eq!(source.locator.find(&ghost, &projects, 1_000), None);
        assert!(source.locator.has_missed("no-such-session"));
        assert_eq!(source.locator.find(&ghost, &projects, 2_000), None);
        assert_eq!(fixture_clock(&[]), 60_000);
    }

    #[test]
    fn a_job_another_session_handed_to_codex_is_not_a_notch_row() {
        use notchling_engine::delegate::options::{Options, Tool};
        use notchling_engine::delegate::record::{self, Delegation};
        let s = crate::testutil::Scratch::new("notch-delegated");
        let rollout = |id: &str| {
            s.write(
                &format!("codex/sessions/2026/11/12/rollout-2026-11-12T09-00-00-{id}.jsonl"),
                &format!(
                    "{{\"timestamp\":\"2026-11-12T10:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"cwd\":\"/w\"}}}}\n"
                ),
            )
        };
        rollout("mine");
        rollout("job");
        let options = Options {
            tool: Tool::Codex,
            prompt: "x".into(),
            model: None,
            effort: None,
            folder: "/w".into(),
            out: None,
            write: false,
            session: None,
            timeout: std::time::Duration::from_secs(1),
        };
        let mut job = Delegation::new("parent", "/w", &options, 1.0, 1);
        job.session = Some("job".into());
        record::write(&s.path().join("notchling").join(record::FOLDER), &job).unwrap();
        let mut source = Source::keeping_everything(DataRoot::fixture(s.path()));
        let rows = source.foreign_rows(&ToolLive::default(), 1_794_160_000_000);
        let ids: Vec<&str> = rows.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["codex:mine"]);
    }
}
