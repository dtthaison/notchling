//! Where Chat reads from: the real folders, or a fixture root laid out like the notch's
//! (`<root>/claude`, `<root>/notchling`, `<root>/codex`; never a `.claude` path), named by
//! `--data-root`, by `NOTCHLING_DATA_ROOT`, or for a render the checked-in `crates/app/fixtures/chat`.
//! A render never reads the real `~/.claude`. Everything here only reads files.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notchling_engine::sessions::{Agent, Monitor, Probes, Roots};
use notchling_engine::transcript::model::Conversation;
use notchling_engine::transcript::subagents::{self, Subagent};
use notchling_engine::transcript::{codex, conversation};
use notchling_platform::paths;

use super::codex_list;
use crate::notch::source::{
    DataRoot as NotchRoot, Source, ANTIGRAVITY_DIR, APP_DIR, DATA_ROOT_ENV,
};
use crate::tools::foreign::AgyList;
use crate::tools::requests::RowTool;
use crate::transcripts::Locator;

pub use super::loader::{Loaded, Loader};

/// Which tool a session belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Claude,
    Codex,
    /// Antigravity CLI: its rows come from a copy of its summaries database, and its conversation
    /// from the daemon (`agy.conversation`), since the files are protobuf.
    Antigravity,
}

impl Tool {
    pub fn of(row_id: &str) -> Tool {
        match RowTool::of(row_id) {
            RowTool::Claude => Tool::Claude,
            RowTool::Codex => Tool::Codex,
            RowTool::Agy => Tool::Antigravity,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Tool::Claude => "Claude Code",
            Tool::Codex => "Codex",
            Tool::Antigravity => "Antigravity",
        }
    }
}

/// The folders with none scanned twice. A lane's `projects` is a symlink to the main one, so
/// several config folders reach the same files by different paths; and a folder inside another
/// watched one is scanned with it. The first spelling of each is kept, in order.
fn distinct_folders(folders: Vec<PathBuf>) -> Vec<PathBuf> {
    let keyed: Vec<(PathBuf, PathBuf)> = folders
        .into_iter()
        .map(|f| (std::fs::canonicalize(&f).unwrap_or_else(|_| f.clone()), f))
        .collect();
    keyed
        .iter()
        .enumerate()
        .filter(|(i, (path, _))| {
            !keyed.iter().enumerate().any(|(j, (other, _))| {
                j != *i && path.starts_with(other) && (path != other || j < *i)
            })
        })
        .map(|(_, (_, folder))| folder.clone())
        .collect()
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
    /// Read as `~/.gemini/antigravity-cli`, through a copy of its summaries database.
    pub antigravity_dir: PathBuf,
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
            antigravity_dir: home.join(ANTIGRAVITY_DIR),
            home,
            fixture: None,
        })
    }

    /// A fixture root: `<root>/claude`, `<root>/notchling`, `<root>/codex`,
    /// `<root>/antigravity-cli`.
    pub fn fixture(root: &Path) -> DataRoot {
        DataRoot {
            claude_dir: root.join("claude"),
            data_dir: root.join("notchling"),
            codex_dir: root.join("codex"),
            antigravity_dir: root.join("antigravity-cli"),
            home: root.to_path_buf(),
            fixture: Some(root.to_path_buf()),
        }
    }

    /// The app's own folder under the data folder, for what only the app keeps: the second
    /// opinions and its copy of Antigravity's database. Beside `daemon/` (the daemon's) and apart
    /// from the Swift app's files in the data folder itself, which a Swift app running next to
    /// this one keeps writing.
    pub fn app_dir(&self) -> PathBuf {
        self.data_dir.join(APP_DIR)
    }

    /// The Antigravity list, read through a copy in our own folder: the one the notch reads too,
    /// so the two make one read between them.
    pub fn new_agy_list(&self) -> AgyList {
        AgyList::shared(
            self.antigravity_dir.clone(),
            self.app_dir().join("antigravity"),
        )
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
            codex_dir: self.codex_dir.clone(),
            antigravity_dir: self.antigravity_dir.clone(),
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
    /// folder, and Codex's sessions. Each real folder once: the watcher scans all of it every poll.
    pub fn watch_folders(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for config in notchling_engine::sessions::config_dirs(&self.roots()) {
            out.push(config.dir.join("jobs"));
            out.push(config.dir.join("sessions"));
            out.push(config.dir.join("projects"));
        }
        out.push(self.codex_dir.join("sessions"));
        distinct_folders(out)
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
    /// Code's across the main folder and the lanes, then Codex's rollouts (30 newest, 14 days)
    /// and Antigravity's summaries (14 days). Each Claude row's transcript is found the way the
    /// notch finds it (`Locator`). The monitor and the locator keep what polls need between them:
    /// ended terminal sessions, and rows with no transcript yet.
    pub fn sessions_with(
        &self,
        monitor: &mut Monitor,
        locator: &mut Locator,
        now_ms: i64,
    ) -> Vec<Session> {
        self.sessions_from(monitor, locator, &mut self.new_agy_list(), None, now_ms)
    }

    /// [`DataRoot::sessions_with`] with Claude Code's rows given: the daemon's list
    /// (`session_source`) in place of reading the jobs and sessions folders here. `None` reads them.
    pub fn sessions_from(
        &self,
        monitor: &mut Monitor,
        locator: &mut Locator,
        agy: &mut AgyList,
        claude: Option<Vec<Agent>>,
        now_ms: i64,
    ) -> Vec<Session> {
        let agents = match claude {
            Some(agents) => agents,
            None => monitor.poll(&self.roots(), &Probes::from_proc(), now_ms),
        };
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
        out.extend(agy.rows(now_ms).into_iter().map(|agent| Session {
            agent,
            tool: Tool::Antigravity,
            transcript: None,
        }));
        out.sort_by(|a, b| {
            a.agent
                .group
                .cmp(&b.agent.group)
                .then(b.agent.since_ms.cmp(&a.agent.since_ms))
                .then(a.agent.id.cmp(&b.agent.id))
        });
        out
    }

    /// The session's conversation, read incrementally by the engine; empty without a transcript. An
    /// unchanged file gives the same `Arc` as the last read.
    pub fn conversation_shared(&self, session: &Session) -> Arc<Conversation> {
        let Some(path) = &session.transcript else {
            return Arc::default();
        };
        let path = path.to_string_lossy();
        match session.tool {
            Tool::Claude => super::cleared::with_model(
                &path,
                &session.agent.name,
                conversation::read_shared(&path),
            ),
            Tool::Codex => codex::read_shared(&path),
            // The daemon's, fetched by the view; never read from Antigravity's files.
            Tool::Antigravity => Arc::default(),
        }
    }

    /// [`DataRoot::conversation_shared`] as an owned copy.
    pub fn conversation(&self, session: &Session) -> Conversation {
        (*self.conversation_shared(session)).clone()
    }

    /// The subagents a Claude Code session started, with how each ended.
    pub fn subagents(
        &self,
        session: &Session,
        conversation: &Conversation,
        now_secs: f64,
    ) -> Vec<Subagent> {
        match (&session.transcript, session.tool) {
            (Some(path), Tool::Claude) => {
                let path = path.to_string_lossy();
                let mut list = subagents::list(&path, &conversation.finished_agents, now_secs);
                // A /clear leaves the old conversation's running subagents beside the old file.
                for agent in super::cleared::running_before(&path, &session.agent.name, now_secs) {
                    if !list.iter().any(|a| a.id == agent.id) {
                        list.push(agent);
                    }
                }
                list.sort_by(|a, b| a.started.total_cmp(&b.started));
                list
            }
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
    fn antigravity_is_copied_into_the_apps_own_folder_and_read_once_for_chat_and_the_notch() {
        let root = DataRoot::fixture(Path::new("/f"));
        let list = root.new_agy_list();
        // Not data/antigravity, which the Swift app copies over, and not daemon/, the daemon's.
        assert_eq!(list.copy_dir, PathBuf::from("/f/notchling/app/antigravity"));
        assert_eq!(
            list.copy_dir,
            NotchRoot::fixture(Path::new("/f")).antigravity_copy_dir()
        );
        // The notch's source and Chat's loader hold the one list.
        let notch = root.notch_source();
        assert!(notch.agy_list().is_same_list(&list));
        assert!(Loader::new(root).agy_list().is_same_list(&list));
    }

    #[test]
    fn real_root_leaves_paths_alone() {
        let root = DataRoot {
            claude_dir: "/Users/me/.claude".into(),
            data_dir: "/Users/me/Library/Application Support/Notchling".into(),
            codex_dir: "/Users/me/.codex".into(),
            antigravity_dir: "/Users/me/.gemini/antigravity-cli".into(),
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

    #[cfg(unix)]
    #[test]
    fn lanes_share_one_projects_folder_so_it_is_watched_once() {
        use crate::testutil::Scratch;
        let s = Scratch::new("watch-lanes");
        let main = s.dir("claude");
        for sub in ["jobs", "sessions", "projects"] {
            s.dir(&format!("claude/{sub}"));
        }
        let lane = s.dir("lanes/work");
        s.dir("lanes/work/jobs");
        s.dir("lanes/work/sessions");
        // The lane's projects is the main one, as `LaneStore.shared` links it.
        std::os::unix::fs::symlink(main.join("projects"), lane.join("projects")).unwrap();
        s.write(
            "notchling/lanes.json",
            &format!(
                r#"[{{"name":"work","path":{:?},"accountID":"a1","email":"w@example.com"}}]"#,
                lane.to_string_lossy()
            ),
        );

        let folders = DataRoot::fixture(s.path()).watch_folders();
        let projects = std::fs::canonicalize(main.join("projects")).unwrap();
        let hits = folders
            .iter()
            .filter(|f| std::fs::canonicalize(f).ok().as_ref() == Some(&projects))
            .count();
        assert_eq!(hits, 1, "{folders:?}");
        // The lane's own jobs and sessions are still watched.
        assert!(folders.contains(&lane.join("jobs")), "{folders:?}");
        assert!(folders.contains(&lane.join("sessions")), "{folders:?}");
        assert!(folders.contains(&main.join("jobs")), "{folders:?}");
    }

    #[test]
    fn a_folder_inside_another_watched_folder_is_dropped() {
        let folders = vec![
            PathBuf::from("/nowhere/a"),
            PathBuf::from("/nowhere/a/b"),
            PathBuf::from("/nowhere/c"),
            PathBuf::from("/nowhere/c"),
        ];
        assert_eq!(
            distinct_folders(folders),
            [PathBuf::from("/nowhere/a"), PathBuf::from("/nowhere/c")]
        );
    }

    fn real_stand_in() -> Option<DataRoot> {
        Some(DataRoot {
            claude_dir: "/Users/me/.claude".into(),
            data_dir: "/Users/me/Library/Application Support/Notchling".into(),
            codex_dir: "/Users/me/.codex".into(),
            antigravity_dir: "/Users/me/.gemini/antigravity-cli".into(),
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
