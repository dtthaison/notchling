use super::remote_control::{refused, remote_control, Action};
use super::sidebar_node::Nodes;
use crate::acting::Control;
use crate::cluster::model::ClusterModel;
use crate::cluster::renders::{status, ME, STANDBY, WORKER};

fn nodes() -> Nodes {
    Nodes::from_cluster(&ClusterModel::from_status(&status(), ME))
}

fn remote(node: &str) -> String {
    format!("{node}/job-1")
}

#[test]
fn remote_control_refuses_each_action_when_offline() {
    let nodes = nodes();
    for row in [remote(WORKER), remote("ffffffffffffffff")] {
        for action in Action::ALL {
            let control = remote_control(&row, &nodes, action).expect("a remote row is gated");
            assert!(!control.enabled, "{action:?} on {row}");
        }
    }
    for action in Action::ALL {
        assert_eq!(
            remote_control("local-job", &nodes, action),
            None,
            "a bare id is local: {action:?}"
        );
    }
    let none = Nodes::default();
    assert!(refused(remote_control(
        &remote(STANDBY),
        &none,
        Action::Stop
    )));
}

#[test]
fn an_online_node_takes_a_reply_a_stop_an_answer_and_a_bring_back() {
    let nodes = nodes();
    for action in [
        Action::Reply,
        Action::Stop,
        Action::Answer,
        Action::BringBack,
    ] {
        assert_eq!(
            remote_control(&remote(STANDBY), &nodes, action),
            None,
            "{action:?}"
        );
    }
}

#[test]
fn remote_row_offers_no_terminal_or_move() {
    let nodes = nodes();
    for action in [
        Action::Remove,
        Action::Move,
        Action::Terminal,
        Action::Slash,
        Action::Files,
        Action::Opinion,
    ] {
        let control = remote_control(&remote(STANDBY), &nodes, action);
        assert_eq!(
            control,
            Some(Control {
                enabled: false,
                tooltip: Some("Not available for a session on another machine yet."),
            }),
            "{action:?}"
        );
    }
}

fn agent(id: &str, command: Option<&str>) -> notchling_engine::sessions::Agent {
    use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
    Agent {
        id: id.into(),
        name: "job".into(),
        project: "api".into(),
        cwd: "/srv/api".into(),
        group: AgentGroup::Working,
        label: "Working".into(),
        detail: None,
        since_ms: 1,
        target: AgentTarget::Process(1),
        suggested_reply: None,
        transcript_path: None,
        host: "Shell".into(),
        headless: false,
        config_dir: None,
        command: command.map(str::to_owned),
        resume_session_id: None,
        at_prompt: false,
        node: None,
    }
}

#[test]
fn drops_and_reveals_are_refused_on_remote_and_shell_job_rows() {
    use super::remote_control::files_control;
    let nodes = nodes();
    for id in [remote(STANDBY), remote(WORKER), remote("ffffffffffffffff")] {
        let held = files_control(&agent(&id, None), &nodes).expect("gated");
        assert!(!held.enabled, "{id}");
    }
    let shell = files_control(&agent("shell:j-1", Some("make")), &nodes).expect("gated");
    assert!(!shell.enabled);
    assert_eq!(files_control(&agent("job-1", None), &nodes), None);
    assert_eq!(
        files_control(&agent("job-1", Some("not a shell id")), &nodes),
        None
    );
}

#[test]
fn no_attach_affordance_on_remote_or_shell_job_rows() {
    use super::remote_control::takes_files;
    use super::sidebar::sidebar_model;
    for id in [remote(STANDBY), remote(WORKER)] {
        assert!(!takes_files(&agent(&id, None)), "{id}");
    }
    assert!(!takes_files(&agent("shell:j-1", Some("make"))));
    assert!(!takes_files(&agent(&remote_shell(), Some("make"))));
    assert!(takes_files(&agent("job-1", None)));

    let rows = [
        agent("job-1", None),
        agent(&remote(STANDBY), None),
        agent("shell:j-1", Some("make")),
    ];
    let sessions: Vec<_> = rows
        .into_iter()
        .map(|agent| super::data::Session {
            agent,
            tool: super::data::Tool::Claude,
            transcript: None,
        })
        .collect();
    let model = sidebar_model(
        &sessions,
        1_000_000_000_000,
        None,
        &crate::accounts::store::Snapshot::default(),
        None,
    );
    let flags: Vec<(String, bool)> = model
        .sections
        .iter()
        .flat_map(|s| &s.rows)
        .map(|r| (r.id.clone(), r.takes_files))
        .collect();
    assert_eq!(flags.len(), 3);
    for (id, takes) in flags {
        assert_eq!(takes, id == "job-1", "{id}");
    }
}

fn remote_shell() -> String {
    format!("{STANDBY}/shell:j-9")
}
