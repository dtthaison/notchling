use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentGroup {
    NeedsInput,
    Working,
    Completed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentTarget {
    Job(String),
    Process(u32),
    Resume { session_id: String, cwd: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub project: String,
    pub cwd: String,
    pub group: AgentGroup,
    pub label: String,
    pub detail: Option<String>,
    pub since_ms: i64,
    pub target: AgentTarget,
    pub suggested_reply: Option<String>,
    pub transcript_path: Option<String>,
    pub host: String,
    pub headless: bool,
    pub config_dir: Option<String>,
    pub command: Option<String>,
    pub resume_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub at_prompt: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<NodeRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeRef {
    pub id: String,
    pub name: String,
    pub online: bool,
}

fn is_false(flag: &bool) -> bool {
    !*flag
}

impl Agent {
    pub fn can_reply(&self) -> bool {
        matches!(self.target, AgentTarget::Job(_)) && self.command.is_none()
    }

    pub fn turn_running(&self) -> bool {
        self.group == AgentGroup::Working && !self.at_prompt
    }

    pub fn takes_held_text(&self) -> bool {
        self.can_reply() && (self.group == AgentGroup::Completed || self.at_prompt)
    }

    pub fn outbox_key(&self) -> String {
        match (&self.target, &self.node) {
            (AgentTarget::Job(job), Some(node)) => format!("{}/{job}", node.id),
            (AgentTarget::Job(job), None) => job.clone(),
            _ => self.id.clone(),
        }
    }

    pub fn session_id(&self) -> Option<String> {
        match &self.target {
            AgentTarget::Resume { session_id, .. } => {
                (!session_id.is_empty()).then(|| session_id.clone())
            }
            _ => {
                let path = self.transcript_path.as_deref()?;
                let stem = std::path::Path::new(path).file_stem()?.to_str()?;
                (!stem.is_empty()).then(|| stem.to_string())
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn sample(target: AgentTarget, transcript: Option<&str>) -> Agent {
    Agent {
        id: "x".into(),
        name: "n".into(),
        project: "p".into(),
        cwd: "/tmp".into(),
        group: AgentGroup::Completed,
        label: "Idle".into(),
        detail: None,
        since_ms: 0,
        target,
        suggested_reply: None,
        transcript_path: transcript.map(String::from),
        host: String::new(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
    }
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
