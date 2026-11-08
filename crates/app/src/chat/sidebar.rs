//! The sidebar's sections and rows, from the sessions the engine lists.

use notchling_engine::search::SessionTool;
use notchling_engine::sessions::AgentGroup;
use notchling_kit::Status;

use super::data::{Session, Tool};
use super::format::age;
use crate::search::wire::SessionRef;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidebarRow {
    pub id: String,
    pub name: String,
    /// The status line under the name: what it needs, or its label.
    pub status_line: String,
    pub age: String,
    pub status: Status,
    pub tool: Tool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub title: &'static str,
    pub rows: Vec<SidebarRow>,
}

/// The kit's status for a group.
pub fn status_of(group: AgentGroup) -> Status {
    match group {
        AgentGroup::NeedsInput => Status::Needs,
        AgentGroup::Working => Status::Working,
        AgentGroup::Completed => Status::Done,
    }
}

fn section_title(group: AgentGroup) -> &'static str {
    match group {
        AgentGroup::NeedsInput => "Needs input",
        AgentGroup::Working => "Working",
        AgentGroup::Completed => "Done",
    }
}

fn row(session: &Session, now_ms: i64) -> SidebarRow {
    let agent = &session.agent;
    let status_line = match &agent.detail {
        Some(detail) if !detail.is_empty() => detail.clone(),
        _ => agent.label.clone(),
    };
    SidebarRow {
        id: agent.id.clone(),
        name: agent.name.clone(),
        status_line,
        age: age(now_ms, agent.since_ms),
        status: status_of(agent.group),
        tool: session.tool,
    }
}

/// The sidebar row a search hit opens (`SessionRef` from the search panel): a Codex row is keyed
/// `codex:<session id>`, an Antigravity row `agy:<session id>`; a Claude Code row by Claude Code's
/// own session id, else by its transcript's file stem.
pub fn row_for(sessions: &[Session], target: &SessionRef) -> Option<String> {
    let id = target.session_id.as_str();
    let found = match target.tool {
        SessionTool::Codex => sessions
            .iter()
            .find(|s| s.tool == Tool::Codex && s.agent.id == format!("codex:{id}")),
        SessionTool::Antigravity => sessions
            .iter()
            .find(|s| s.tool == Tool::Antigravity && s.agent.id == format!("agy:{id}")),
        SessionTool::Claude => sessions
            .iter()
            .filter(|s| s.tool == Tool::Claude)
            .find(|s| {
                s.agent.session_id().as_deref() == Some(id)
                    || s.transcript
                        .as_deref()
                        .and_then(|p| p.file_stem())
                        .and_then(|stem| stem.to_str())
                        == Some(id)
            }),
    };
    found.map(|s| s.agent.id.clone())
}

/// Needs input, Working, Done, in that order; a group with no sessions has no section. The rows
/// keep the engine's order (newest first within a group).
pub fn sections(sessions: &[Session], now_ms: i64) -> Vec<Section> {
    [
        AgentGroup::NeedsInput,
        AgentGroup::Working,
        AgentGroup::Completed,
    ]
    .into_iter()
    .filter_map(|group| {
        let rows: Vec<SidebarRow> = sessions
            .iter()
            .filter(|s| s.agent.group == group)
            .map(|s| row(s, now_ms))
            .collect();
        (!rows.is_empty()).then(|| Section {
            title: section_title(group),
            rows,
        })
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::sessions::{Agent, AgentTarget};

    pub(crate) fn session(
        id: &str,
        group: AgentGroup,
        detail: Option<&str>,
        tool: Tool,
    ) -> Session {
        Session {
            agent: Agent {
                id: id.into(),
                name: format!("name {id}"),
                project: "p".into(),
                cwd: "/w/p".into(),
                group,
                label: match group {
                    AgentGroup::NeedsInput => "Needs input".into(),
                    AgentGroup::Working => "Working".into(),
                    AgentGroup::Completed => "Idle".into(),
                },
                detail: detail.map(String::from),
                since_ms: 1_000_000_000_000 - 120_000,
                target: AgentTarget::Job(id.into()),
                suggested_reply: None,
                transcript_path: None,
                host: "Background".into(),
                headless: false,
                config_dir: None,
                command: None,
                resume_session_id: None,
            },
            tool,
            transcript: None,
        }
    }

    #[test]
    fn sections_follow_group_order_and_skip_empty_groups() {
        let sessions = vec![
            session("a", AgentGroup::Completed, None, Tool::Claude),
            session(
                "b",
                AgentGroup::NeedsInput,
                Some("answer: Which?"),
                Tool::Claude,
            ),
            session("c", AgentGroup::Completed, None, Tool::Codex),
        ];
        let sections = sections(&sessions, 1_000_000_000_000);
        assert_eq!(
            sections.iter().map(|s| s.title).collect::<Vec<_>>(),
            ["Needs input", "Done"]
        );
        assert_eq!(sections[0].rows[0].status_line, "answer: Which?");
        assert_eq!(sections[0].rows[0].status, Status::Needs);
        assert_eq!(sections[1].rows[0].status_line, "Idle");
        assert_eq!(sections[1].rows[0].age, "2m");
        assert_eq!(sections[1].rows[1].tool, Tool::Codex);
    }

    #[test]
    fn no_sessions_no_sections() {
        assert!(sections(&[], 0).is_empty());
    }

    fn target(tool: SessionTool, id: &str) -> SessionRef {
        SessionRef {
            tool,
            session_id: id.into(),
        }
    }

    #[test]
    fn a_search_hit_finds_its_sidebar_row() {
        let mut by_transcript = session("job-1", AgentGroup::Working, None, Tool::Claude);
        by_transcript.transcript = Some("/claude/projects/-w-p/3f1c2a9e.jsonl".into());
        let mut resumed = session("job-2", AgentGroup::Completed, None, Tool::Claude);
        resumed.agent.target = AgentTarget::Resume {
            session_id: "7c1e".into(),
            cwd: "/w/p".into(),
        };
        let codex = session("codex:0199a1b2", AgentGroup::Completed, None, Tool::Codex);
        let agy = session(
            "agy:d674-13c2",
            AgentGroup::Completed,
            None,
            Tool::Antigravity,
        );
        let sessions = vec![by_transcript, resumed, codex, agy];
        assert_eq!(
            row_for(&sessions, &target(SessionTool::Claude, "3f1c2a9e")).as_deref(),
            Some("job-1"),
            "the transcript's stem"
        );
        assert_eq!(
            row_for(&sessions, &target(SessionTool::Claude, "7c1e")).as_deref(),
            Some("job-2"),
            "a resume target's id"
        );
        assert_eq!(
            row_for(&sessions, &target(SessionTool::Codex, "0199a1b2")).as_deref(),
            Some("codex:0199a1b2")
        );
        assert_eq!(
            row_for(&sessions, &target(SessionTool::Codex, "3f1c2a9e")),
            None
        );
        assert_eq!(
            row_for(&sessions, &target(SessionTool::Claude, "nope")),
            None
        );
        assert_eq!(
            row_for(&sessions, &target(SessionTool::Antigravity, "d674-13c2")).as_deref(),
            Some("agy:d674-13c2")
        );
        assert_eq!(
            row_for(&sessions, &target(SessionTool::Antigravity, "x")),
            None
        );
    }
}
