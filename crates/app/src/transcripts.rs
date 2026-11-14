use std::collections::HashMap;
use std::path::{Path, PathBuf};

use notchling_engine::sessions::Agent;
use notchling_engine::transcript::continued::find_earlier;
use notchling_engine::transcript::locate::{find_transcript, successor};

pub const MISS_RETRY: i64 = 30_000;

#[derive(Clone, Debug, Default)]
pub struct Locator {
    base: Option<PathBuf>,
    missed: HashMap<String, i64>,
}

impl Locator {
    pub fn new(base: Option<PathBuf>) -> Locator {
        Locator {
            base,
            missed: HashMap::new(),
        }
    }

    pub fn find(&mut self, agent: &Agent, projects: &Path, now_ms: i64) -> Option<PathBuf> {
        if agent.command.is_some() {
            return None;
        }
        let linked = agent
            .transcript_path
            .as_deref()
            .map(|p| self.resolve(p))
            .filter(|p| p.is_file());
        if let Some(path) = linked {
            let text = path.to_string_lossy();
            return Some(successor(&text, &agent.name).map_or_else(|| path.clone(), PathBuf::from));
        }
        let stem = agent
            .transcript_path
            .as_deref()
            .map(Path::new)
            .filter(|p| p.is_absolute())
            .and_then(Path::file_stem)
            .map(|stem| stem.to_string_lossy().into_owned());
        if self
            .missed
            .get(&agent.id)
            .is_some_and(|at| now_ms - at < MISS_RETRY)
        {
            return self.earlier(agent, stem.as_deref(), projects);
        }
        let moved = stem
            .as_deref()
            .and_then(|stem| find_transcript(&projects.to_string_lossy(), stem, &agent.cwd));
        let found =
            moved.or_else(|| find_transcript(&projects.to_string_lossy(), &agent.id, &agent.cwd));
        match found {
            Some(path) => Some(PathBuf::from(path)),
            None => {
                self.missed.insert(agent.id.clone(), now_ms);
                self.earlier(agent, stem.as_deref(), projects)
            }
        }
    }

    fn earlier(&self, agent: &Agent, stem: Option<&str>, projects: &Path) -> Option<PathBuf> {
        let projects = projects.to_string_lossy();
        stem.into_iter()
            .chain(Some(agent.id.as_str()))
            .find_map(|id| find_earlier(&projects, id, &agent.cwd))
            .map(PathBuf::from)
    }

    pub fn retain(&mut self, agents: &[Agent]) {
        self.missed
            .retain(|id, _| agents.iter().any(|a| &a.id == id));
    }

    pub fn has_missed(&self, id: &str) -> bool {
        self.missed.contains_key(id)
    }

    fn resolve(&self, path: &str) -> PathBuf {
        let path = PathBuf::from(path);
        match &self.base {
            Some(base) if path.is_relative() => base.join(path),
            _ => path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::sessions::{AgentGroup, AgentTarget};

    fn agent(id: &str) -> Agent {
        Agent {
            id: id.into(),
            name: "worker".into(),
            project: "app".into(),
            cwd: "/work/app".into(),
            group: AgentGroup::Completed,
            label: "Idle".into(),
            detail: None,
            since_ms: 0,
            target: AgentTarget::Process(1),
            suggested_reply: None,
            transcript_path: None,
            host: "Terminal".into(),
            headless: false,
            config_dir: None,
            command: None,
            resume_session_id: None,
            at_prompt: false,
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("notchling-locator-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_relative_link_path_belongs_to_the_fixture_root() {
        let dir = scratch("relative");
        let projects = dir.join("claude/projects");
        std::fs::create_dir_all(projects.join("-work-app")).unwrap();
        let file = projects.join("-work-app/s1.jsonl");
        std::fs::write(&file, "{}\n").unwrap();
        let mut ghost = agent("not-the-file-name");
        ghost.transcript_path = Some("claude/projects/-work-app/s1.jsonl".into());

        let mut fixture = Locator::new(Some(dir.clone()));
        assert_eq!(fixture.find(&ghost, &projects, 0), Some(file));
        let mut real = Locator::new(None);
        assert_eq!(real.find(&ghost, &projects, 0), None, "no base, no file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_transcript_is_searched_again_only_after_the_retry_time() {
        let dir = scratch("miss");
        let projects = dir.join("projects");
        std::fs::create_dir_all(projects.join("-work-app")).unwrap();
        let mut locator = Locator::new(None);
        let row = agent("late");
        assert_eq!(locator.find(&row, &projects, 1_000), None);
        assert!(locator.has_missed("late"));

        let file = projects.join("-work-app/late.jsonl");
        std::fs::write(&file, "{}\n").unwrap();
        assert_eq!(locator.find(&row, &projects, 1_000 + MISS_RETRY - 1), None);
        assert_eq!(
            locator.find(&row, &projects, 1_000 + MISS_RETRY),
            Some(file)
        );

        locator.retain(&[]);
        assert!(!locator.has_missed("late"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_command_job_has_no_transcript_and_is_never_searched_for() {
        let dir = scratch("command");
        let projects = dir.join("projects");
        std::fs::create_dir_all(projects.join("-work-app")).unwrap();
        std::fs::write(projects.join("-work-app/cmd.jsonl"), "{}\n").unwrap();
        let mut row = agent("cmd");
        row.command = Some("make > run.log".into());
        let mut locator = Locator::new(None);
        assert_eq!(locator.find(&row, &projects, 0), None);
        assert!(!locator.has_missed("cmd"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
#[path = "transcripts_continued_tests.rs"]
mod continued_tests;
