use super::*;

const CLAUDE_ROW: &str = "0b9e6c0e-1111-2222-3333-444455556666";
const LOST_JOB: &str = "j-eeeeeeeeeeee";

fn keys(list: &[&str]) -> Vec<String> {
    list.iter().map(|k| k.to_string()).collect()
}

#[test]
fn an_unmapped_row_is_served_with_the_live_masks_while_a_known_job_is_lost() {
    let r = rig("some-other-session");
    r.masks.mark_granted_as(LOST_JOB, &keys(&["lost-session"]));
    r.grant("NL-MARKER-1");
    r.append("said NL-MARKER-1\n");
    let out = r.tail("some-other-session", 0).expect("served");
    assert!(!served(&out).contains("NL-MARKER"), "the union masks it");
    assert!(served(&out).contains("[secret api-key]"));
}

#[test]
fn an_unmapped_row_with_nothing_live_is_served_as_is_while_a_known_job_is_lost() {
    let r = rig("some-other-session");
    r.masks.mark_granted_as(LOST_JOB, &keys(&["lost-session"]));
    r.append("plain\n");
    assert!(served(&r.tail("some-other-session", 0).unwrap()) == "plain\n");
}

#[test]
fn a_lost_jobs_claude_row_is_refused_by_its_short_id_even_with_no_cluster_state() {
    let r = rig_short(CLAUDE_ROW, Some("c1"));
    *r.fake.state.lock().unwrap() = None;
    r.masks.mark_granted_as(JOB, &keys(&["c1"]));
    r.append("said NL-MARKER-1\n");
    let err = r.tail(CLAUDE_ROW, 0).expect_err("refused");
    assert!(err.message.contains(JOB));
    assert!(!err.message.contains("NL-MARKER"));
}

#[test]
fn a_lost_jobs_claude_row_is_refused_when_the_state_has_not_linked_its_session() {
    let r = rig_short(CLAUDE_ROW, Some("c1"));
    r.masks.mark_granted_as(JOB, &keys(&["c1"]));
    r.append("said NL-MARKER-1\n");
    assert!(r.tail(CLAUDE_ROW, 0).is_err(), "the key finds the job");
}

#[test]
fn a_lost_jobs_claude_row_is_found_by_its_own_session_id_too() {
    let r = rig(CLAUDE_ROW);
    r.masks.mark_granted_as(JOB, &keys(&[CLAUDE_ROW]));
    r.append("said NL-MARKER-1\n");
    assert!(r.tail(CLAUDE_ROW, 0).is_err());
}

#[test]
fn a_lost_job_with_no_known_session_still_refuses_unmapped_rows() {
    let r = rig("some-other-session");
    r.masks.mark_granted(LOST_JOB);
    r.append("anything\n");
    let err = r.tail("some-other-session", 0).expect_err("refused");
    assert_eq!(err.message, UNMAPPED_LOST);
}

#[test]
fn a_lost_shell_jobs_log_is_served_as_written() {
    let r = rig(&shell_id());
    r.masks.mark_granted_as(JOB, &keys(&[&shell_id()]));
    r.append("already [secret api-key] masked at write\n");
    let out = r.tail(&shell_id(), 0).expect("served");
    assert!(served(&out) == "already [secret api-key] masked at write\n");
    assert_eq!(out["next"], out["size"]);
}

fn prompt(id_: &str, session: &str, text: &str) -> Value {
    json!({
        "id": id_,
        "sessionId": session,
        "kind": "permission",
        "summary": text,
        "input": {"command": text},
    })
}

fn prompts_rig(list: Vec<Value>) -> Rig {
    let mut r = rig_short(CLAUDE_ROW, Some("c1"));
    let mut running = job(JOB, "worker", JobState::Running);
    running.session = Some("c1".to_string());
    *r.fake.state.lock().unwrap() = Some(cluster("vault", &[running]));
    r.tails = Arc::new(MaskedTails::new(
        Arc::new(Conversation(Value::Array(list))),
        Arc::new(Rows {
            path: r.file.clone(),
            id: CLAUDE_ROW.to_string(),
            short: Some("c1".to_string()),
        }),
        (r.masks.clone(), r.fake.clone()),
        Arc::new(|_: &str| true),
        r.local(),
    ));
    r
}

fn listed(r: &Rig) -> Result<Vec<Value>, ApiError> {
    let out = r.tails.call("prompts.list", &json!({}), &origin())?;
    Ok(out.as_array().cloned().unwrap_or_default())
}

#[test]
fn a_prompt_list_masks_each_prompts_input_and_summary() {
    let r = prompts_rig(vec![
        prompt("p1", CLAUDE_ROW, "curl -H NL-MARKER-1 x"),
        prompt("p2", "unknown-session", "echo NL-MARKER-1"),
        prompt("p3", "unknown-session", "ls"),
    ]);
    r.grant("NL-MARKER-1");
    let list = listed(&r).expect("served");
    assert_eq!(list.len(), 3);
    assert!(!Value::Array(list.clone()).to_string().contains("NL-MARKER"));
    assert!(list[0]["input"]["command"]
        .as_str()
        .unwrap()
        .contains("[secret api-key]"));
    assert!(list[1]["summary"]
        .as_str()
        .unwrap()
        .contains("[secret api-key]"));
    assert_eq!(list[2]["input"]["command"], "ls", "left alone");
}

#[test]
fn a_prompt_list_without_masks_is_served_as_is() {
    let reply = vec![prompt("p1", CLAUDE_ROW, "echo NL-MARKER-1")];
    let r = prompts_rig(reply.clone());
    assert_eq!(listed(&r).expect("served"), reply);
}

#[test]
fn a_prompt_of_a_job_that_lost_its_masks_is_not_served() {
    let r = prompts_rig(vec![
        prompt("p1", "lost-session", "echo NL-MARKER-1"),
        prompt("p2", "other-session", "ls"),
    ]);
    r.masks.mark_granted_as(LOST_JOB, &keys(&["lost-session"]));
    let list = listed(&r).expect("served");
    assert_eq!(list.len(), 1, "the lost job's prompt is held back");
    assert_eq!(list[0]["id"], "p2");
    assert!(!Value::Array(list).to_string().contains("NL-MARKER"));
}

#[test]
fn a_prompt_list_that_is_not_a_list_passes_through() {
    let mut r = prompts_rig(Vec::new());
    r.tails = Arc::new(MaskedTails::new(
        Arc::new(Conversation(json!({"error": "x"}))),
        Arc::new(Rows {
            path: r.file.clone(),
            id: CLAUDE_ROW.to_string(),
            short: None,
        }),
        (r.masks.clone(), r.fake.clone()),
        Arc::new(|_: &str| true),
        r.local(),
    ));
    let out = r.tails.call("prompts.list", &json!({}), &origin()).unwrap();
    assert_eq!(out, json!({"error": "x"}));
}

fn row(id_: &str, short: Option<&str>, text: &str) -> Value {
    let mut row = json!({
        "id": id_,
        "name": "build",
        "detail": text,
        "suggestedReply": text,
        "command": text,
    });
    if let Some(short) = short {
        row["target"] = json!({"job": short});
    }
    row
}

fn sessions_rig(list: Vec<Value>) -> Rig {
    let mut r = prompts_rig(Vec::new());
    r.tails = Arc::new(MaskedTails::new(
        Arc::new(Conversation(Value::Array(list))),
        Arc::new(Rows {
            path: r.file.clone(),
            id: CLAUDE_ROW.to_string(),
            short: Some("c1".to_string()),
        }),
        (r.masks.clone(), r.fake.clone()),
        Arc::new(|_: &str| true),
        r.local(),
    ));
    r
}

fn sessions(r: &Rig) -> Vec<Value> {
    let out = r
        .tails
        .call("node.sessions", &json!({}), &origin())
        .unwrap();
    out.as_array().cloned().unwrap_or_default()
}

#[test]
fn a_session_row_text_is_masked_for_its_job_and_for_unmapped_rows() {
    let r = sessions_rig(vec![
        row(CLAUDE_ROW, Some("c1"), "curl NL-MARKER-1"),
        row("unknown-session", None, "echo NL-MARKER-1"),
        row("quiet", None, "ls"),
    ]);
    r.grant("NL-MARKER-1");
    let list = sessions(&r);
    assert_eq!(list.len(), 3);
    assert!(!Value::Array(list.clone()).to_string().contains("NL-MARKER"));
    assert!(list[0]["detail"]
        .as_str()
        .unwrap()
        .contains("[secret api-key]"));
    assert!(list[1]["command"]
        .as_str()
        .unwrap()
        .contains("[secret api-key]"));
    assert_eq!(list[2]["detail"], "ls");
}

#[test]
fn a_session_row_of_a_job_that_lost_its_masks_keeps_its_row_but_not_its_text() {
    let r = sessions_rig(vec![
        row("lost-session", None, "echo NL-MARKER-1"),
        row("quiet", None, "ls"),
    ]);
    r.masks.mark_granted_as(LOST_JOB, &keys(&["lost-session"]));
    let list = sessions(&r);
    assert_eq!(list.len(), 2, "the row stays");
    assert_eq!(list[0]["id"], "lost-session");
    assert_eq!(list[0]["name"], "build");
    for field in ["detail", "suggestedReply", "command"] {
        assert!(list[0].get(field).is_none(), "{field} is dropped");
    }
    assert_eq!(list[1]["detail"], "ls");
}

#[test]
fn a_session_row_whose_masked_text_no_longer_parses_is_dropped_whole() {
    let r = sessions_rig(vec![
        json!({"id": "unknown-session", "name": "build", "label": "x"}),
        row("quiet", None, "ls"),
    ]);
    r.grant("\"id\":\"unknown-session\"");
    let list = sessions(&r);
    assert_eq!(list.len(), 1, "the broken row is gone, not half kept");
    assert_eq!(list[0]["id"], "quiet");
}

#[test]
fn rows_are_masked_with_one_snapshot_of_the_state() {
    let r = sessions_rig(vec![
        row("a", None, "echo NL-MARKER-1"),
        row("b", None, "echo NL-MARKER-1"),
    ]);
    r.grant("NL-MARKER-1");
    let list = sessions(&r);
    assert_eq!(list.len(), 2);
    assert!(!Value::Array(list).to_string().contains("NL-MARKER"));
}
