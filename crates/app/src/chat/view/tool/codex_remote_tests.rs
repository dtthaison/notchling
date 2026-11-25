use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};

use super::adapter;
use crate::chat::data::{Session, Tool};
use crate::cluster::renders::STANDBY;

fn row(id: &str, group: AgentGroup) -> Session {
    Session {
        agent: Agent {
            id: id.into(),
            name: "fix it".into(),
            project: "api".into(),
            cwd: "/srv/api".into(),
            group,
            label: "Working".into(),
            detail: None,
            since_ms: 1,
            target: AgentTarget::Process(0),
            suggested_reply: None,
            transcript_path: None,
            host: "Codex".into(),
            headless: false,
            config_dir: None,
            command: None,
            resume_session_id: None,
            at_prompt: false,
            node: None,
            worktree_path: None,
        },
        tool: Tool::Codex,
        transcript: None,
    }
}

#[test]
fn a_remote_codex_row_is_working_when_its_group_says_so() {
    let busy = row(&format!("{STANDBY}/codex:s1"), AgentGroup::Working);
    assert!(adapter(&busy).working(&busy));
    let idle = row(&format!("{STANDBY}/codex:s1"), AgentGroup::Completed);
    assert!(!adapter(&idle).working(&idle));
}

#[test]
fn a_local_codex_row_is_working_only_through_the_live_turn_events() {
    let local = row("codex:s1", AgentGroup::Working);
    assert!(!adapter(&local).working(&local));
}
