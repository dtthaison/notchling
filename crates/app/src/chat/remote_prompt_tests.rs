use std::collections::{HashMap, HashSet};

use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use serde_json::{json, Value};

use super::data::Tool;
use super::prompts::{permission_request, prompt_for, screen_read_key, Prompt};
use crate::acting::{Answer, Decision, Request};
use crate::cluster::renders::STANDBY;

fn remote_agent(id: &str, mirror: Option<&str>) -> Agent {
    Agent {
        id: format!("{STANDBY}/{id}"),
        name: "job".into(),
        project: "api".into(),
        cwd: "/srv/api".into(),
        group: AgentGroup::NeedsInput,
        label: "Needs input".into(),
        detail: Some("answer: Which db?".into()),
        since_ms: 1,
        target: AgentTarget::Job(id.into()),
        suggested_reply: None,
        transcript_path: mirror.map(str::to_owned),
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
    }
}

fn permission(id: &str, session: &str, path: Value) -> Value {
    json!({"id": id, "sessionId": session, "kind": "permission", "toolName": "Bash",
           "summary": "rm -rf build", "canAlwaysAllow": true, "createdMs": 7,
           "transcriptPath": path})
}

fn question(id: &str, session: &str) -> Value {
    json!({"id": id, "sessionId": session, "kind": "question", "toolName": "AskUserQuestion",
           "summary": "Which db?", "createdMs": 5, "transcriptPath": null,
           "question": {"items": [{"header": "DB", "question": "Which db?",
               "options": ["Postgres", "SQLite"], "details": ["", ""], "multiSelect": false}]}})
}

fn show(agent: &Agent, prompts: &[Value]) -> Option<Prompt> {
    prompt_for(
        agent,
        Tool::Claude,
        Some(prompts),
        &HashMap::new(),
        None,
        None,
        &HashSet::new(),
    )
}

#[test]
fn remote_prompt_shows_on_remote_row_without_always() {
    let mirror = "/data/cluster/mirror/abc/0123456789abcdef.jsonl";
    let agent = remote_agent("job-1", Some(mirror));
    let qualified = format!("{STANDBY}/p1");
    let by_path = permission(&qualified, &format!("{STANDBY}/other"), json!(mirror));
    let Some(Prompt::Permission(shown)) = show(&agent, &[by_path]) else {
        panic!("a permission card")
    };
    assert_eq!(shown.id, qualified);
    assert!(!shown.can_always_allow, "never Always from another node");

    let by_name = permission(&qualified, &agent.id, Value::Null);
    let Some(Prompt::Permission(shown)) = show(&agent, &[by_name]) else {
        panic!("named by the row's id")
    };
    assert!(!shown.can_always_allow);

    let other = remote_agent(
        "job-2",
        Some("/data/cluster/mirror/abc/ffffffffffffffff.jsonl"),
    );
    let theirs = permission(&qualified, &format!("{STANDBY}/zzz"), json!(mirror));
    assert_eq!(show(&other, &[theirs]), None, "another row's prompt");

    let bare = permission("p9", &agent.id, json!(mirror));
    assert_eq!(
        show(&agent, &[bare]),
        None,
        "a prompt with a bare id is not remote"
    );

    let local = Agent {
        id: "job-1".into(),
        ..remote_agent("job-1", None)
    };
    let remote_one = permission(&qualified, &format!("{STANDBY}/job-1"), Value::Null);
    assert_eq!(show(&local, &[remote_one]), None, "never on a local row");
}

#[test]
fn a_remote_row_reads_no_screen_and_takes_no_needs_text() {
    let agent = remote_agent("job-1", None);
    assert_eq!(screen_read_key(&agent, Tool::Claude), None);
    assert_eq!(
        show(&agent, &[]),
        None,
        "the owner's needs text is not a card"
    );
    let asked = question(&format!("{STANDBY}/q1"), &agent.id);
    let Some(Prompt::Question(q)) = show(&agent, &[asked]) else {
        panic!("a question card")
    };
    assert_eq!(q.items.len(), 1);
}

#[test]
fn remote_answer_uses_qualified_id() {
    let agent = remote_agent("job-1", None);
    let qualified = format!("{STANDBY}/q1");
    let Some(Prompt::Question(q)) = show(&agent, &[question(&qualified, &agent.id)]) else {
        panic!("a question card")
    };
    let request = Request::answer_for(&agent.id, &q, &[Answer::Options(vec![1])]);
    assert_eq!(request.op, "session.answer");
    assert_eq!(request.args["id"], json!(qualified));

    let Some(Prompt::Permission(held)) = show(
        &agent,
        &[permission(&format!("{STANDBY}/p1"), &agent.id, Value::Null)],
    ) else {
        panic!("a permission card")
    };
    for decision in [Decision::Allow, Decision::Deny] {
        let request = permission_request(&held, decision);
        assert_eq!(request.op, "session.permission");
        assert_eq!(request.args["id"], json!(format!("{STANDBY}/p1")));
        assert_eq!(request.args["decision"], json!(decision.wire()));
    }
}

#[test]
fn remote_prompt_matches_row_by_row_id() {
    let agent = remote_agent("job-1", None);
    let other = remote_agent("job-2", None);
    let qualified = format!("{STANDBY}/p1");
    let mut prompt = permission(&qualified, &format!("{STANDBY}/session-uuid"), Value::Null);
    prompt["rowId"] = json!(agent.id);
    let Some(Prompt::Permission(shown)) = show(&agent, std::slice::from_ref(&prompt)) else {
        panic!("shown on the row named by rowId")
    };
    assert_eq!(shown.id, qualified);
    assert!(!shown.can_always_allow);
    assert_eq!(show(&other, std::slice::from_ref(&prompt)), None);

    let mut elsewhere = permission(&qualified, &agent.id, Value::Null);
    elsewhere["rowId"] = json!(other.id);
    assert_eq!(
        show(&agent, &[elsewhere]),
        None,
        "a rowId naming another row wins over the session id"
    );
}
