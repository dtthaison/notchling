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
}

impl Agent {
    pub fn can_reply(&self) -> bool {
        matches!(self.target, AgentTarget::Job(_)) && self.command.is_none()
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids() {
        let job = sample(
            AgentTarget::Job("ab12".into()),
            Some("/h/.claude/projects/p/1f2e-uuid.jsonl"),
        );
        assert_eq!(job.session_id().as_deref(), Some("1f2e-uuid"));
        assert_eq!(
            sample(AgentTarget::Job("ab12".into()), None).session_id(),
            None
        );
        let resume = sample(
            AgentTarget::Resume {
                session_id: "s1".into(),
                cwd: "/tmp".into(),
            },
            None,
        );
        assert_eq!(resume.session_id().as_deref(), Some("s1"));
        let empty = sample(
            AgentTarget::Resume {
                session_id: String::new(),
                cwd: "/tmp".into(),
            },
            None,
        );
        assert_eq!(empty.session_id(), None);
    }

    #[test]
    fn only_conversation_jobs_take_replies() {
        let mut a = sample(AgentTarget::Job("ab".into()), None);
        assert!(a.can_reply());
        a.command = Some("make".into());
        assert!(!a.can_reply());
        assert!(!sample(AgentTarget::Process(5), None).can_reply());
    }
}
