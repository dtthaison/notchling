use super::agent::{Agent, AgentGroup, AgentTarget};
use notchling_platform::files::{ensure_private_dir, write_private_no_follow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Row {
    pub(super) id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    project: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    detail: Option<String>,
    since_ms: i64,
    #[serde(default)]
    config_dir: Option<String>,
    #[serde(default)]
    worktree_path: Option<String>,
}

impl Row {
    pub(super) fn of(agent: &Agent, since_ms: i64) -> Row {
        Row {
            id: agent.id.clone(),
            name: agent.name.clone(),
            project: agent.project.clone(),
            cwd: agent.cwd.clone(),
            detail: agent.detail.clone(),
            since_ms,
            config_dir: agent.config_dir.clone(),
            worktree_path: agent.worktree_path.clone(),
        }
    }

    pub(super) fn agent(self) -> Agent {
        Agent {
            target: AgentTarget::Resume {
                session_id: self.id.clone(),
                cwd: self.cwd.clone(),
            },
            id: self.id,
            name: self.name,
            project: self.project,
            cwd: self.cwd,
            group: AgentGroup::Completed,
            label: "Exited".to_string(),
            detail: self.detail,
            since_ms: self.since_ms,
            suggested_reply: None,
            transcript_path: None,
            host: String::new(),
            headless: false,
            config_dir: self.config_dir,
            command: None,
            resume_session_id: None,
            at_prompt: false,
            node: None,
            worktree_path: self.worktree_path,
        }
    }

    fn named_from_folder(mut self) -> Row {
        let folder = Path::new(&self.cwd)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if self.name.is_empty() {
            self.name = folder.clone();
        }
        if self.project.is_empty() {
            self.project = folder;
        }
        self
    }
}

#[derive(Serialize)]
struct Book<'a> {
    exited: &'a [Row],
}

#[derive(Deserialize, Default)]
struct Stored {
    #[serde(default)]
    exited: Vec<Value>,
}

pub struct ExitedStore {
    file: PathBuf,
}

impl ExitedStore {
    pub fn new(file: PathBuf) -> ExitedStore {
        ExitedStore { file }
    }

    pub fn load(&self) -> Vec<Agent> {
        let stored: Stored = std::fs::read(&self.file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        stored
            .exited
            .into_iter()
            .filter_map(|value| serde_json::from_value::<Row>(value).ok())
            .filter(|row| !row.id.is_empty())
            .map(|row| row.named_from_folder().agent())
            .collect()
    }

    pub fn save(&self, agents: &[Agent]) {
        let mut rows: Vec<Row> = agents.iter().map(|a| Row::of(a, a.since_ms)).collect();
        rows.sort_by(|a, b| b.since_ms.cmp(&a.since_ms).then(a.id.cmp(&b.id)));
        let Ok(bytes) = serde_json::to_vec(&Book { exited: &rows }) else {
            return;
        };
        if let Some(dir) = self.file.parent() {
            if ensure_private_dir(dir).is_err() {
                return;
            }
        }
        let _ = write_private_no_follow(&self.file, &bytes);
    }
}
