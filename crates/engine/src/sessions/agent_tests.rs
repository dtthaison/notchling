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

#[test]
fn outbox_key_is_the_job_short_id_else_the_row_id() {
    let mut job = sample(AgentTarget::Job("ab12cd34".into()), None);
    job.id = "6f1c2a9e-3b7d-4e21-9a0c-5d8e7f6a1b2c".into();
    assert_eq!(job.outbox_key(), "ab12cd34");
    let mut codex = sample(AgentTarget::Process(7), None);
    codex.id = "codex:t1".into();
    assert_eq!(codex.outbox_key(), "codex:t1");
}

#[test]
fn a_remote_rows_outbox_key_carries_its_node() {
    let node = NodeRef {
        id: "0123456789abcdef".into(),
        name: "w1".into(),
        online: true,
    };
    let mut job = sample(AgentTarget::Job("ab12cd34".into()), None);
    job.id = "0123456789abcdef/6f1c2a9e".into();
    job.node = Some(node.clone());
    assert_eq!(job.outbox_key(), "0123456789abcdef/ab12cd34");
    let mut codex = sample(AgentTarget::Process(7), None);
    codex.id = "0123456789abcdef/codex:t1".into();
    codex.node = Some(node);
    assert_eq!(codex.outbox_key(), "0123456789abcdef/codex:t1");
}

#[test]
fn the_node_field_is_absent_unless_set_and_old_rows_read() {
    let mut a = sample(AgentTarget::Job("ab".into()), None);
    let plain = serde_json::to_value(&a).unwrap();
    assert!(plain.get("node").is_none(), "{plain}");
    let back: Agent = serde_json::from_value(plain).unwrap();
    assert_eq!(back.node, None);
    a.node = Some(NodeRef {
        id: "0123456789abcdef".into(),
        name: "w1".into(),
        online: false,
    });
    let row = serde_json::to_value(&a).unwrap();
    assert_eq!(row["node"]["online"], false);
    assert_eq!(serde_json::from_value::<Agent>(row).unwrap(), a);
}
