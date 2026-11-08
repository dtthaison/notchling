//! Where Chat reads from: the real folders, or a fixture root for `--render chat` and the tests,
//! which never touch `~/.claude`. Everything here only reads files.

use std::path::{Path, PathBuf};

use notchling_engine::sessions::{Agent, Monitor, Probes, Roots};
use notchling_engine::transcript::model::Conversation;
use notchling_engine::transcript::subagents::{self, Subagent};
use notchling_engine::transcript::{codex, conversation};
use notchling_platform::paths;

use super::codex_list;

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
    /// Holds `.claude` (jobs, sessions, projects).
    pub home: PathBuf,
    /// Notchling's data folder (`lanes.json`).
    pub data_dir: PathBuf,
    /// `~/.codex`: Codex's sessions.
    pub codex_home: PathBuf,
    /// A fixture root: transcript paths in it may be relative to this folder, its rows never age
    /// out of the Done section and its Codex files aren't filtered by date.
    pub fixture: Option<PathBuf>,
}

impl DataRoot {
    /// The user's real folders. `None` when the environment doesn't say where home is.
    pub fn real() -> Option<DataRoot> {
        let home = paths::home()?;
        Some(DataRoot {
            codex_home: home.join(".codex"),
            data_dir: paths::data_dir()?,
            home,
            fixture: None,
        })
    }

    /// A fixture root: `<dir>/home/.claude`, `<dir>/data/lanes.json`, `<dir>/.codex`.
    pub fn fixture(dir: &Path) -> DataRoot {
        DataRoot {
            home: dir.join("home"),
            data_dir: dir.join("data"),
            codex_home: dir.join(".codex"),
            fixture: Some(dir.to_path_buf()),
        }
    }

    pub fn roots(&self) -> Roots {
        Roots::new(&self.home, &self.data_dir)
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
        out.push(self.codex_home.join("sessions"));
        out
    }

    /// A transcript path from a job's state: relative ones belong to a fixture root.
    fn resolve(&self, path: &str) -> PathBuf {
        let path = PathBuf::from(path);
        match &self.fixture {
            Some(base) if path.is_relative() => base.join(path),
            _ => path,
        }
    }

    /// Every session, needs input first, then working, then done, newest first in each: Claude
    /// Code's across the main folder and the lanes, then Codex's rollouts (30 newest, 14 days).
    pub fn sessions(&self, now_ms: i64) -> Vec<Session> {
        let probes = Probes::from_proc();
        let mut monitor = Monitor::default();
        if self.fixture.is_some() {
            monitor.completed_window_ms = i64::MAX / 2;
        }
        let mut out: Vec<Session> = monitor
            .poll(&self.roots(), &probes, now_ms)
            .into_iter()
            .map(|agent| Session {
                transcript: agent.transcript_path.as_deref().map(|p| self.resolve(p)),
                agent,
                tool: Tool::Claude,
            })
            .collect();
        let days = self.fixture.is_none().then_some(14);
        for path in codex_list::rollouts(&self.codex_home.join("sessions"), days, now_ms, 30) {
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
    fn fixture_root_layout() {
        let root = DataRoot::fixture(Path::new("/f"));
        assert_eq!(root.home, PathBuf::from("/f/home"));
        assert_eq!(root.roots().main_dir, PathBuf::from("/f/home/.claude"));
        assert_eq!(root.roots().lanes_file, PathBuf::from("/f/data/lanes.json"));
        assert_eq!(
            root.resolve("home/x.jsonl"),
            PathBuf::from("/f/home/x.jsonl")
        );
        assert_eq!(root.resolve("/abs/x.jsonl"), PathBuf::from("/abs/x.jsonl"));
        let folders = root.watch_folders();
        assert!(folders.contains(&PathBuf::from("/f/home/.claude/projects")));
        assert!(folders.contains(&PathBuf::from("/f/.codex/sessions")));
    }

    #[test]
    fn real_root_leaves_paths_alone() {
        let root = DataRoot {
            home: "/Users/me".into(),
            data_dir: "/Users/me/Library/Application Support/Notchling".into(),
            codex_home: "/Users/me/.codex".into(),
            fixture: None,
        };
        assert_eq!(root.resolve("rel.jsonl"), PathBuf::from("rel.jsonl"));
    }
}
