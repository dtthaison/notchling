//! Where Chat reads from: the real folders, or a fixture root laid out like the notch's
//! (`<root>/claude`, `<root>/notchling`, `<root>/codex`; never a `.claude` path), named by
//! `--data-root`, by `NOTCHLING_DATA_ROOT`, or for a render the checked-in `crates/app/fixtures/chat`.
//! A render never reads the real `~/.claude`. Everything here only reads files.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use notchling_engine::sessions::{Agent, Monitor, Probes, Roots};
use notchling_engine::transcript::model::Conversation;
use notchling_engine::transcript::subagents::{self, Subagent};
use notchling_engine::transcript::{codex, conversation};
use notchling_platform::paths;

use super::codex_list;
use crate::notch::source::{DataRoot as NotchRoot, Source, DATA_ROOT_ENV};
use crate::transcripts::Locator;

/// Which tool a session belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Claude,
    Codex,
}

/// A sidebar session: the engine's row, its tool and where its transcript is.
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub agent: Agent,
    pub tool: Tool,
    pub transcript: Option<PathBuf>,
}

/// The folders Chat reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataRoot {
    /// Read as `~/.claude`: `jobs/`, `sessions/`, `projects/`.
    pub claude_dir: PathBuf,
    /// Notchling's data folder (`lanes.json`).
    pub data_dir: PathBuf,
    /// Read as `~/.codex`: Codex's `sessions/`.
    pub codex_dir: PathBuf,
    /// The user's home, for `~` in paths.
    pub home: PathBuf,
    /// A fixture root: transcript paths in it may be relative to this folder, its rows never age
    /// out of the Done section and its Codex files aren't filtered by date.
    pub fixture: Option<PathBuf>,
}

impl DataRoot {
    /// The user's real folders. `None` when the environment doesn't say where home is.
    pub fn real() -> Option<DataRoot> {
        let home = paths::home()?;
        Some(DataRoot {
            claude_dir: home.join(".claude"),
            data_dir: paths::data_dir()?,
            codex_dir: home.join(".codex"),
            home,
            fixture: None,
        })
    }

    /// A fixture root: `<root>/claude`, `<root>/notchling`, `<root>/codex`.
    pub fn fixture(root: &Path) -> DataRoot {
        DataRoot {
            claude_dir: root.join("claude"),
            data_dir: root.join("notchling"),
            codex_dir: root.join("codex"),
            home: root.to_path_buf(),
            fixture: Some(root.to_path_buf()),
        }
    }

    /// The fixture root the environment names (`NOTCHLING_DATA_ROOT`), if any.
    pub fn from_env() -> Option<DataRoot> {
        DataRoot::named(None, std::env::var_os(DATA_ROOT_ENV).as_deref())
    }

    /// The fixture root a `--data-root` argument names, else the environment variable's value
    /// (empty names nothing).
    fn named(data_root: Option<&Path>, env: Option<&OsStr>) -> Option<DataRoot> {
        let dir = match data_root {
            Some(dir) => dir.to_path_buf(),
            None => PathBuf::from(env.filter(|v| !v.is_empty())?),
        };
        Some(DataRoot::fixture(&dir))
    }

    /// What a render reads: the `--data-root` folder, else the environment's, else the fixture this
    /// crate carries. Never the real home.
    pub fn for_render(data_root: Option<&Path>) -> DataRoot {
        DataRoot::named(data_root, std::env::var_os(DATA_ROOT_ENV).as_deref()).unwrap_or_else(
            || DataRoot::fixture(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/chat")),
        )
    }

    /// What the running app reads, for Chat and the notch alike: the `--data-root` folder, else the
    /// environment's, else the user's real folders (`None` without a home).
    pub fn for_run(data_root: Option<&Path>) -> Option<DataRoot> {
        DataRoot::resolve_run(
            data_root,
            std::env::var_os(DATA_ROOT_ENV).as_deref(),
            DataRoot::real,
        )
    }

    /// [`DataRoot::for_run`] with the environment and the real folders passed in.
    fn resolve_run(
        data_root: Option<&Path>,
        env: Option<&OsStr>,
        real: impl FnOnce() -> Option<DataRoot>,
    ) -> Option<DataRoot> {
        DataRoot::named(data_root, env).or_else(real)
    }

    /// The notch's source over this root: the same folders, and a fixture's finished rows stay.
    pub fn notch_source(&self) -> Source {
        let root = NotchRoot {
            claude_dir: self.claude_dir.clone(),
            data_dir: self.data_dir.clone(),
            fixture: self.fixture.clone(),
        };
        match self.fixture {
            Some(_) => Source::keeping_everything(root),
            None => Source::new(root),
        }
    }

    pub fn roots(&self) -> Roots {
        Roots {
            main_dir: self.claude_dir.clone(),
            lanes_file: self.data_dir.join("lanes.json"),
        }
    }

    /// The folders whose changes mean a reload: jobs, sessions and transcripts of every config
    /// folder, and Codex's sessions.
    pub fn watch_folders(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for config in notchling_engine::sessions::config_dirs(&self.roots()) {
            out.push(config.dir.join("jobs"));
            out.push(config.dir.join("sessions"));
            out.push(config.dir.join("projects"));
        }
        out.push(self.codex_dir.join("sessions"));
        out
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.claude_dir.join("projects")
    }

    /// A monitor for this root: a fixture root never ages its Done rows out.
    pub fn new_monitor(&self) -> Monitor {
        let mut monitor = Monitor::default();
        if self.fixture.is_some() {
            monitor.completed_window_ms = i64::MAX / 4;
        }
        monitor
    }

    /// Finds transcripts for this root (a fixture's job state may name them relative to it).
    pub fn new_locator(&self) -> Locator {
        Locator::new(self.fixture.clone())
    }

    /// Every session with a throwaway monitor and locator: nothing is remembered between calls.
    pub fn sessions(&self, now_ms: i64) -> Vec<Session> {
        self.sessions_with(&mut self.new_monitor(), &mut self.new_locator(), now_ms)
    }

    /// Every session, needs input first, then working, then done, newest first in each: Claude
    /// Code's across the main folder and the lanes, then Codex's rollouts (30 newest, 14 days).
    /// Each Claude row's transcript is found the way the notch finds it (`Locator`). The monitor
    /// and the locator keep what polls need between them: ended terminal sessions, and rows with no
    /// transcript yet.
    pub fn sessions_with(
        &self,
        monitor: &mut Monitor,
        locator: &mut Locator,
        now_ms: i64,
    ) -> Vec<Session> {
        let agents = monitor.poll(&self.roots(), &Probes::from_proc(), now_ms);
        let projects = self.projects_dir();
        let mut out: Vec<Session> = agents
            .iter()
            .map(|agent| Session {
                transcript: locator.find(agent, &projects, now_ms),
                agent: agent.clone(),
                tool: Tool::Claude,
            })
            .collect();
        locator.retain(&agents);
        let days = self.fixture.is_none().then_some(14);
        for path in codex_list::rollouts(&self.codex_dir.join("sessions"), days, now_ms, 30) {
            if let Some(agent) = codex_list::session(&path) {
                out.push(Session {
                    agent,
                    tool: Tool::Codex,
                    transcript: Some(path),
                });
            }
        }
        out.sort_by(|a, b| {
            a.agent
                .group
                .cmp(&b.agent.group)
                .then(b.agent.since_ms.cmp(&a.agent.since_ms))
                .then(a.agent.id.cmp(&b.agent.id))
        });
        out
    }

    /// The session's conversation, read incrementally by the engine; empty without a transcript.
    pub fn conversation(&self, session: &Session) -> Conversation {
        let Some(path) = &session.transcript else {
            return Conversation::default();
        };
        let path = path.to_string_lossy();
        match session.tool {
            Tool::Claude => conversation::read(&path),
            Tool::Codex => codex::read(&path),
        }
    }

    /// The subagents a Claude Code session started, with how each ended.
    pub fn subagents(
        &self,
        session: &Session,
        conversation: &Conversation,
        now_secs: f64,
    ) -> Vec<Subagent> {
        match (&session.transcript, session.tool) {
            (Some(path), Tool::Claude) => subagents::list(
                &path.to_string_lossy(),
                &conversation.finished_agents,
                now_secs,
            ),
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_root_layout_is_the_notch_convention() {
        let root = DataRoot::fixture(Path::new("/f"));
        assert_eq!(root.claude_dir, PathBuf::from("/f/claude"));
        assert_eq!(root.roots().main_dir, PathBuf::from("/f/claude"));
        assert_eq!(
            root.roots().lanes_file,
            PathBuf::from("/f/notchling/lanes.json")
        );
        assert_eq!(root.projects_dir(), PathBuf::from("/f/claude/projects"));
        let folders = root.watch_folders();
        assert!(folders.contains(&PathBuf::from("/f/claude/projects")));
        assert!(folders.contains(&PathBuf::from("/f/codex/sessions")));
    }

    #[test]
    fn real_root_leaves_paths_alone() {
        let root = DataRoot {
            claude_dir: "/Users/me/.claude".into(),
            data_dir: "/Users/me/Library/Application Support/Notchling".into(),
            codex_dir: "/Users/me/.codex".into(),
            home: "/Users/me".into(),
            fixture: None,
        };
        assert_eq!(
            root.projects_dir(),
            PathBuf::from("/Users/me/.claude/projects")
        );
        // Real rows age out of Done after a day; a fixture's never do.
        assert_eq!(root.new_monitor().completed_window_ms, 24 * 3600 * 1000);
        assert!(
            DataRoot::fixture(Path::new("/f"))
                .new_monitor()
                .completed_window_ms
                > 1 << 50
        );
    }

    fn real_stand_in() -> Option<DataRoot> {
        Some(DataRoot {
            claude_dir: "/Users/me/.claude".into(),
            data_dir: "/Users/me/Library/Application Support/Notchling".into(),
            codex_dir: "/Users/me/.codex".into(),
            home: "/Users/me".into(),
            fixture: None,
        })
    }

    #[test]
    fn the_running_app_reads_the_argument_then_the_environment_then_the_real_folders() {
        let env = std::ffi::OsString::from("/from-env");
        let arg = Path::new("/from-arg");
        let picked = DataRoot::resolve_run(Some(arg), Some(&env), real_stand_in);
        assert_eq!(
            picked.unwrap().claude_dir,
            PathBuf::from("/from-arg/claude")
        );
        let picked = DataRoot::resolve_run(None, Some(&env), real_stand_in);
        assert_eq!(
            picked.unwrap().claude_dir,
            PathBuf::from("/from-env/claude")
        );
        let empty = std::ffi::OsString::new();
        let picked = DataRoot::resolve_run(None, Some(&empty), real_stand_in).unwrap();
        assert_eq!(picked.fixture, None, "an empty variable names nothing");
        let picked = DataRoot::resolve_run(None, None, real_stand_in).unwrap();
        assert_eq!(picked.claude_dir, PathBuf::from("/Users/me/.claude"));
        assert_eq!(DataRoot::resolve_run(None, None, || None), None);
    }

    #[test]
    fn the_notch_reads_the_same_root_chat_does() {
        let fixture = DataRoot::fixture(Path::new("/f"));
        let source = fixture.notch_source();
        assert_eq!(source.root.claude_dir, PathBuf::from("/f/claude"));
        assert_eq!(source.root.data_dir, PathBuf::from("/f/notchling"));
        assert_eq!(source.root.fixture, Some(PathBuf::from("/f")));

        let real = real_stand_in().unwrap().notch_source();
        assert_eq!(real.root.claude_dir, PathBuf::from("/Users/me/.claude"));
        assert_eq!(
            real.root.data_dir,
            PathBuf::from("/Users/me/Library/Application Support/Notchling")
        );
        assert_eq!(real.root.fixture, None);
    }

    #[test]
    fn a_fixture_notch_keeps_finished_rows_however_old() {
        let root = DataRoot::fixture(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/chat"));
        // Years after the fixture's dates: a real source would have aged every Done row out.
        let far = 1_900_000_000_000;
        let loaded = root.notch_source().load(far);
        assert!(
            loaded.agents.iter().any(|a| a.name == "shell mode"),
            "{:?}",
            loaded.agents.iter().map(|a| &a.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_render_never_names_the_real_home() {
        let explicit = DataRoot::for_render(Some(Path::new("/f")));
        assert_eq!(explicit.claude_dir, PathBuf::from("/f/claude"));
        let default = DataRoot::for_render(None);
        assert!(default.fixture.is_some());
        assert!(!default.claude_dir.ends_with(".claude"));
        // The environment may name another fixture; either way it's a fixture, never ~/.claude.
        assert!(default.claude_dir.ends_with("claude"));
    }
}
