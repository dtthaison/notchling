use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::id::{NodeId, Tool};
use super::members::Term;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct JobSpec {
    pub node: NodeId,
    pub tool: Tool,
    pub cwd: String,
    pub args: Value,
    #[serde(default)]
    pub secrets: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Dispatching,
    Running,
    Done,
    Failed,
    Interrupted,
    Lost,
}

impl JobState {
    pub fn may_become(self, next: JobState) -> bool {
        use JobState::*;
        matches!(
            (self, next),
            (Dispatching, Running)
                | (Dispatching, Failed)
                | (Running, Done)
                | (Running, Failed)
                | (Running, Interrupted)
                | (Running, Lost)
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Job {
    pub id: String,
    pub key: String,
    pub spec: JobSpec,
    pub state: JobState,
    pub term: Term,
    pub created_ms: i64,
    pub session: Option<String>,
    pub detail: Option<String>,
}

pub fn job_id(key: &str) -> String {
    let digest = Sha256::digest(key.as_bytes());
    let mut out = String::from("j-");
    for byte in digest.iter().take(6) {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}
