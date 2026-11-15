use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};

use super::{adapter, of};
use crate::chat::data::{Session, Tool};
use crate::cluster::renders::STANDBY;

fn row(id: &str, command: Option<&str>) -> Session {
    Session {
        agent: Agent {
            id: id.into(),
            name: "make test".into(),
            project: "api".into(),
            cwd: "/srv/api".into(),
            group: AgentGroup::Working,
            label: "Working".into(),
            detail: None,
            since_ms: 1,
            target: AgentTarget::Process(42),
            suggested_reply: None,
            transcript_path: None,
            host: "Shell".into(),
            headless: false,
            config_dir: None,
            command: command.map(str::to_owned),
            resume_session_id: None,
            at_prompt: false,
            node: None,
        },
        tool: Tool::Claude,
        transcript: None,
    }
}

#[test]
fn a_shell_job_row_stops_through_shell_stop_with_its_qualified_id() {
    let remote = row(&format!("{STANDBY}/shell:j-1"), Some("make test"));
    let request = adapter(&remote).stop(&remote.agent.id);
    assert_eq!(request.op, "shell.stop");
    assert_eq!(request.args["id"], format!("{STANDBY}/shell:j-1"));
    assert!(adapter(&remote).working(&remote));

    let local = row("shell:j-2", Some("make test"));
    let request = adapter(&local).stop("shell:j-2");
    assert_eq!(request.op, "shell.stop");
    assert_eq!(request.args["id"], "shell:j-2");
}

#[test]
fn only_a_shell_job_row_gets_the_shell_adapter() {
    let session = row(&format!("{STANDBY}/job-1"), None);
    assert_eq!(
        adapter(&session).placeholder(),
        of(Tool::Claude).placeholder()
    );
    assert_eq!(
        adapter(&session).stop("x").op,
        of(Tool::Claude).stop("x").op
    );
    let command = row("job-3", Some("make"));
    assert_eq!(
        adapter(&command).placeholder(),
        of(Tool::Claude).placeholder()
    );
}
