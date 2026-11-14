use super::model::{Control, Session, SessionState};
use crate::sessions::{Agent, AgentGroup, AgentTarget};

pub fn session(agent: &Agent) -> Session {
    let id = agent.session_id().unwrap_or_else(|| agent.id.clone());
    let control = match &agent.target {
        AgentTarget::Job(job) if agent.can_reply() => Control::Job(job.clone()),
        _ => Control::ViewOnly,
    };
    let state = match (&agent.target, agent.group) {
        (AgentTarget::Resume { .. }, _) => SessionState::Exited,
        (_, AgentGroup::NeedsInput) => SessionState::NeedsInput,
        (_, AgentGroup::Working) => SessionState::Running,
        (_, AgentGroup::Completed) => SessionState::Done,
    };
    let mut session = Session::new(&id, &agent.name, &agent.cwd, control, state);
    if !agent.project.is_empty() {
        session.project = agent.project.clone();
    }
    session.pid = match agent.target {
        AgentTarget::Process(pid) => i32::try_from(pid).ok(),
        _ => None,
    };
    let job = matches!(agent.target, AgentTarget::Job(_));
    session.entrypoint = if agent.headless {
        "sdk".into()
    } else if job {
        "job".into()
    } else {
        "cli".into()
    };
    session.kind = if job { "background" } else { "interactive" }.into();
    session.status_updated_ms = agent.since_ms as f64;
    session.started_ms = agent.since_ms as f64;
    session.transcript_path = agent.transcript_path.clone();
    session
}

pub fn sessions(agents: &[Agent]) -> Vec<Session> {
    agents.iter().filter(|a| !a.headless).map(session).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(target: AgentTarget, group: AgentGroup) -> Agent {
        Agent {
            id: "row-1".into(),
            name: "fix tests".into(),
            project: "app".into(),
            cwd: "/Users/example/src/app".into(),
            group,
            label: "Working".into(),
            detail: None,
            since_ms: 1_793_541_465_000,
            target,
            suggested_reply: None,
            transcript_path: Some("/h/.claude/projects/p/5f1c-uuid.jsonl".into()),
            host: "Background".into(),
            headless: false,
            config_dir: None,
            command: None,
            resume_session_id: None,
            at_prompt: false,
        }
    }

    #[test]
    fn a_background_job_takes_messages_under_its_session_id() {
        let s = session(&agent(AgentTarget::Job("ab12".into()), AgentGroup::Working));
        assert_eq!(s.id, "5f1c-uuid");
        assert_eq!(s.control, Control::Job("ab12".into()));
        assert_eq!(s.state, SessionState::Running);
        assert_eq!(s.entrypoint, "job");
        assert_eq!(s.project, "app");
        assert_eq!(s.status_updated_ms, 1_793_541_465_000.0);
    }

    #[test]
    fn terminals_exited_rows_and_exec_jobs_are_view_only() {
        let t = session(&agent(AgentTarget::Process(4242), AgentGroup::NeedsInput));
        assert_eq!(
            (t.control.clone(), t.state, t.pid),
            (Control::ViewOnly, SessionState::NeedsInput, Some(4242))
        );
        let gone = AgentTarget::Resume {
            session_id: "s9".into(),
            cwd: "/tmp".into(),
        };
        let e = session(&agent(gone, AgentGroup::Completed));
        assert_eq!((e.id.as_str(), e.state), ("s9", SessionState::Exited));
        let mut exec = agent(AgentTarget::Job("cd34".into()), AgentGroup::Completed);
        exec.command = Some("make".into());
        exec.transcript_path = None;
        let x = session(&exec);
        assert_eq!(
            (x.id.as_str(), x.control, x.state),
            ("row-1", Control::ViewOnly, SessionState::Done)
        );
    }

    #[test]
    fn headless_rows_are_left_out() {
        let mut p = agent(AgentTarget::Process(7), AgentGroup::Working);
        p.headless = true;
        let job = agent(AgentTarget::Job("ab12".into()), AgentGroup::Working);
        assert_eq!(sessions(&[p, job]).len(), 1);
    }
}
