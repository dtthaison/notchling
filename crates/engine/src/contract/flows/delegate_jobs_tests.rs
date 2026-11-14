use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::delegate::options::{Options, Tool as DelegateTool};
use crate::sessions::testutil::TempDir;

fn job(
    tool: DelegateTool,
    parent: &str,
    session: Option<&str>,
    started: f64,
    pid: u32,
) -> Delegation {
    let options = Options {
        tool,
        prompt: "Review the diff\nthen say so".into(),
        model: Some("gpt-5.5".into()),
        effort: Some("high".into()),
        folder: "/work".into(),
        out: None,
        write: false,
        session: None,
        timeout: Duration::from_secs(30),
    };
    let mut d = Delegation::new(parent, "/work", &options, started, pid);
    d.session = session.map(str::to_owned);
    d
}

fn finished(mut d: Delegation, end: End) -> Delegation {
    d.ended = Some(d.started + 5.0);
    d.outcome = Some(end);
    d
}

fn roots(dir: &TempDir) -> Roots {
    Roots {
        codex_sessions: dir.path("codex/sessions"),
        antigravity: dir.path("agy"),
        ..Roots::default()
    }
}

fn parent(id: &str) -> SessionRef {
    SessionRef {
        id: id.into(),
        tool: Tool::Claude,
        transcript: None,
        name: String::new(),
        finished: Arc::default(),
        session: id.into(),
    }
}

fn alive(_: u32) -> bool {
    true
}

fn dead(_: u32) -> bool {
    false
}

#[test]
fn only_the_jobs_a_session_handed_over_are_its_subagents() {
    let dir = TempDir::new("jobs-parent");
    let records = vec![
        job(DelegateTool::Codex, "mine", Some("c1"), 100.0, 1),
        job(DelegateTool::Agy, "other", Some("a1"), 110.0, 2),
        job(DelegateTool::Agy, "mine", Some("a2"), 120.0, 3),
    ];
    let list = subagents(&records, &parent("mine"), &roots(&dir), 130.0, &alive);
    let seen: Vec<_> = list.iter().map(|a| (a.origin, a.started)).collect();
    assert_eq!(
        seen,
        [
            (Origin::delegated(Tool::Codex), 100.0),
            (Origin::delegated(Tool::Antigravity), 120.0)
        ]
    );
    assert!(list
        .iter()
        .all(|a| a.id.starts_with("job:") && a.parent == "mine"));
    assert!(subagents(&records, &parent("nobody"), &roots(&dir), 130.0, &alive).is_empty());
}

#[test]
fn a_job_reads_as_a_subagent_with_the_tools_name_model_and_status() {
    let dir = TempDir::new("jobs-row");
    let rollout = dir.write(
        "codex/sessions/2026/11/12/rollout-2026-11-12T09-00-00-c1.jsonl",
        "{}\n",
    );
    let at = |d: &Delegation| subagent(d, &parent("m"), &roots(&dir), 130.0, &alive);
    let running = at(&job(DelegateTool::Codex, "m", Some("c1"), 100.0, 1));
    assert_eq!(running.status, Status::Running);
    assert_eq!(running.agent_type, "Codex");
    assert_eq!(running.description, "Review the diff");
    assert_eq!(running.model.as_deref(), Some("gpt-5.5"));
    assert_eq!(running.path, rollout.to_string_lossy());
    assert_eq!(running.origin, Origin::delegated(Tool::Codex));
    let done = finished(job(DelegateTool::Codex, "m", Some("c1"), 100.0, 1), End::Ok);
    assert_eq!(at(&done).status, Status::Completed);
    let failed = finished(job(DelegateTool::Codex, "m", None, 100.0, 1), End::Failed);
    assert_eq!(at(&failed).status, Status::Failed);
    assert_eq!(at(&failed).path, "", "no session yet, no transcript yet");
    let stopped = finished(job(DelegateTool::Codex, "m", None, 100.0, 1), End::Stopped);
    assert_eq!(at(&stopped).status, Status::Stopped);
}

#[test]
fn a_job_whose_command_died_without_saying_so_is_not_running() {
    let dir = TempDir::new("jobs-dead");
    let orphan = subagent(
        &job(DelegateTool::Agy, "m", Some("a1"), 100.0, 9),
        &parent("m"),
        &roots(&dir),
        130.0,
        &dead,
    );
    assert_eq!(orphan.status, Status::Stopped);
}

#[test]
fn an_antigravity_job_reads_its_log_once_it_exists() {
    let dir = TempDir::new("jobs-agy");
    let record = job(DelegateTool::Agy, "m", Some("a1"), 100.0, 1);
    let at = || subagent(&record, &parent("m"), &roots(&dir), 130.0, &alive);
    assert_eq!(at().path, "");
    let log = dir.write("agy/brain/a1/.system_generated/logs/transcript.jsonl", "");
    assert_eq!(at().path, log.to_string_lossy());
    assert_eq!(at().agent_type, "Antigravity CLI");
}

#[test]
fn the_job_keeps_its_id_when_its_session_gets_one() {
    let dir = TempDir::new("jobs-id");
    let at = |d: &Delegation| subagent(d, &parent("m"), &roots(&dir), 101.0, &alive);
    let before = at(&job(DelegateTool::Codex, "m", None, 100.0, 5));
    let after = at(&job(DelegateTool::Codex, "m", Some("c9"), 100.0, 5));
    assert_eq!(before.id, after.id);
}

#[test]
fn a_session_is_matched_by_its_transcripts_id_and_by_those_its_clears_ended() {
    let dir = TempDir::new("jobs-clear");
    let named = |a: &str| format!(r#"{{"type":"agent-name","agentName":"{a}"}}"#) + "\n";
    dir.write("p/old-id.jsonl", &named("worker"));
    std::thread::sleep(Duration::from_millis(50));
    let new = dir.write(
        "p/new-id.jsonl",
        &(r#"{"type":"system","agentName":"worker","content":"SessionStart:clear"}"#.to_owned()
            + "\n"),
    );
    let ids = parent_ids(&new.to_string_lossy(), "worker");
    assert_eq!(ids, ["new-id", "old-id"]);
    let mut top = parent("new-id");
    top.transcript = Some(new);
    top.name = "worker".into();
    assert_eq!(parents_of(&top), ids);
    let records = vec![
        job(DelegateTool::Codex, "old-id", Some("c1"), 100.0, 1),
        job(DelegateTool::Codex, "new-id", Some("c2"), 200.0, 2),
        job(DelegateTool::Codex, "unrelated", Some("c3"), 150.0, 3),
    ];
    assert_eq!(
        subagents(&records, &top, &roots(&dir), 300.0, &alive).len(),
        2
    );
}

#[test]
fn the_sessions_of_jobs_are_hidden_and_records_come_from_the_folder() {
    let dir = TempDir::new("jobs-hidden");
    let records = vec![
        job(DelegateTool::Codex, "m", Some("c1"), 1.0, 1),
        job(DelegateTool::Agy, "m", Some("a1"), 2.0, 2),
        job(DelegateTool::Codex, "m", None, 3.0, 3),
    ];
    let hidden = hidden(&records);
    assert!(hidden.contains("codex:c1") && hidden.contains("agy:a1"));
    assert_eq!(hidden.len(), 2);
    assert!(read(dir.root()).is_empty());
    record::write(&folder(dir.root()), &records[0]).unwrap();
    assert_eq!(read(dir.root()).len(), 1);
}
