//! Where the notch's data comes from, read-only: the rows (`engine::sessions`), each row's
//! transcript snapshot (`engine::transcript::snapshot`) and the prompts the hook holds.
//!
//! A [`DataRoot`] names the two folders read: the Claude Code config folder (`~/.claude`) and
//! Notchling's data folder (`lanes.json`, `requests/`). The app reads the real ones; a render reads
//! a fixture root laid out as `<root>/claude` and `<root>/notchling` (never `.claude`, which the
//! repository's zones protect wherever it sits), from `NOTCHLING_DATA_ROOT` or the checked-in
//! `crates/app/fixtures/notch`, and never the real home.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use notchling_engine::sessions::{Agent, Monitor, Probes, Roots};
use notchling_engine::settings::dirs::{data_dir, DirEnv};
use notchling_engine::transcript::snapshot::{self, Snapshot};

use super::requests::{self, HookRequest};
use crate::transcripts::Locator;

/// The fixture root a render reads, when set.
pub const DATA_ROOT_ENV: &str = "NOTCHLING_DATA_ROOT";

/// The folders read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataRoot {
    /// Read as `~/.claude`: `jobs/`, `sessions/`, `projects/`.
    pub claude_dir: PathBuf,
    /// Notchling's data folder: `lanes.json`, `requests/`.
    pub data_dir: PathBuf,
    /// A fixture root: a relative transcript path in a job's state belongs to it.
    pub fixture: Option<PathBuf>,
}

impl DataRoot {
    /// This user's real folders; `None` when the environment has no home.
    pub fn real() -> Option<DataRoot> {
        let env = DirEnv::from_process();
        Some(DataRoot {
            claude_dir: env.home.as_ref()?.join(".claude"),
            data_dir: data_dir(&env)?,
            fixture: None,
        })
    }

    /// A fixture root: `<root>/claude` and `<root>/notchling`.
    pub fn fixture(root: &Path) -> DataRoot {
        DataRoot {
            claude_dir: root.join("claude"),
            data_dir: root.join("notchling"),
            fixture: Some(root.to_path_buf()),
        }
    }

    /// What a render reads: [`DATA_ROOT_ENV`]'s folder, else the fixture this crate carries.
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

/// One read of everything the notch shows.
#[derive(Clone, Debug, Default)]
pub struct Loaded {
    pub agents: Vec<Agent>,
    pub snapshots: HashMap<String, Snapshot>,
    pub requests: HashMap<String, HookRequest>,
}

/// Reads a data root, keeping what polls need between them: the monitor's memory of ended terminal
/// sessions and which rows had no transcript.
pub struct Source {
    pub root: DataRoot,
    monitor: Monitor,
    locator: Locator,
}

impl Source {
    pub fn new(root: DataRoot) -> Source {
        Source {
            locator: Locator::new(root.fixture.clone()),
            root,
            monitor: Monitor::default(),
        }
    }

    /// A source that shows every finished row however old: a fixture's dates are fixed.
    pub fn keeping_everything(root: DataRoot) -> Source {
        let mut source = Source::new(root);
        source.monitor.completed_window_ms = i64::MAX / 4;
        source
    }

    /// Polls the rows, reads each row's snapshot and the held prompts. `now_ms` is the clock.
    pub fn load(&mut self, now_ms: i64) -> Loaded {
        let agents = self
            .monitor
            .poll(&self.root.roots(), &Probes::from_proc(), now_ms);
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
        let requests = requests::read_dir(&self.root.requests_dir(), now_ms as f64 / 1000.0);
        Loaded {
            agents,
            snapshots,
            requests,
        }
    }
}

/// Now, in milliseconds since the epoch.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A fixed clock for a fixture: a minute after its newest row, so ages read the same every render.
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
        assert_eq!(root.roots().main_dir, root.claude_dir);
        assert_eq!(root.requests_dir(), root.data_dir.join("requests"));
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
}
