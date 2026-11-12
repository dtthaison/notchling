use super::*;
use crate::testutil::Scratch;
use notchling_engine::delegate::options::{Options, Tool};
use std::time::Duration;

fn job(tool: Tool, parent: &str, session: Option<&str>, started: f64, pid: u32) -> Delegation {
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

fn places(s: &Scratch) -> (PathBuf, PathBuf) {
    (s.path().join("codex/sessions"), s.path().join("agy"))
}

fn alive(_: u32) -> bool {
    true
}
fn dead(_: u32) -> bool {
    false
}

#[test]
fn only_the_jobs_a_session_handed_over_are_its_subagents() {
    let s = Scratch::new("deleg-parent");
    let (codex, agy) = places(&s);
    let p = Places {
        codex_sessions: &codex,
        antigravity: &agy,
    };
    let records = vec![
        job(Tool::Codex, "mine", Some("c1"), 100.0, 1),
        job(Tool::Agy, "other", Some("a1"), 110.0, 2),
        job(Tool::Agy, "mine", Some("a2"), 120.0, 3),
    ];
    let list = subagents(&records, &["mine".into()], &p, 130.0, &alive);
    let descriptions: Vec<_> = list.iter().map(|a| (a.origin, a.started)).collect();
    assert_eq!(
        descriptions,
        [(Origin::Codex, 100.0), (Origin::Antigravity, 120.0)],
        "oldest first, the other session's job left out"
    );
    assert!(subagents(&records, &["nobody".into()], &p, 130.0, &alive).is_empty());
}

#[test]
fn a_job_reads_as_a_subagent_with_the_tools_name_model_and_status() {
    let s = Scratch::new("deleg-row");
    let (codex, agy) = places(&s);
    let rollout = s.write(
        "codex/sessions/2026/11/12/rollout-2026-11-12T09-00-00-c1.jsonl",
        "{}\n",
    );
    let p = Places {
        codex_sessions: &codex,
        antigravity: &agy,
    };
    let running = subagent(
        &job(Tool::Codex, "m", Some("c1"), 100.0, 1),
        &p,
        130.0,
        &alive,
    );
    assert_eq!(running.status, Status::Running);
    assert_eq!(running.agent_type, "Codex");
    assert_eq!(running.description, "Review the diff");
    assert_eq!(running.model.as_deref(), Some("gpt-5.5"));
    assert_eq!(running.effort.as_deref(), Some("high"));
    assert_eq!(running.path, rollout.to_string_lossy());
    assert_eq!(running.origin, Origin::Codex);

    let done = subagent(
        &finished(job(Tool::Codex, "m", Some("c1"), 100.0, 1), End::Ok),
        &p,
        130.0,
        &alive,
    );
    assert_eq!(done.status, Status::Completed);
    let failed = subagent(
        &finished(job(Tool::Codex, "m", None, 100.0, 1), End::Failed),
        &p,
        130.0,
        &alive,
    );
    assert_eq!(failed.status, Status::Failed);
    assert_eq!(failed.path, "", "no session yet, no transcript yet");
    let stopped = subagent(
        &finished(job(Tool::Codex, "m", None, 100.0, 1), End::Stopped),
        &p,
        130.0,
        &alive,
    );
    assert_eq!(stopped.status, Status::Stopped);
}

#[test]
fn a_job_whose_command_died_without_saying_so_is_not_running() {
    let s = Scratch::new("deleg-dead");
    let (codex, agy) = places(&s);
    let p = Places {
        codex_sessions: &codex,
        antigravity: &agy,
    };
    let orphan = subagent(&job(Tool::Agy, "m", Some("a1"), 100.0, 9), &p, 130.0, &dead);
    assert_eq!(orphan.status, Status::Stopped);
}

#[test]
fn an_antigravity_job_reads_its_log_once_it_exists() {
    let s = Scratch::new("deleg-agy");
    let (codex, agy) = places(&s);
    let p = Places {
        codex_sessions: &codex,
        antigravity: &agy,
    };
    let record = job(Tool::Agy, "m", Some("a1"), 100.0, 1);
    assert_eq!(subagent(&record, &p, 130.0, &alive).path, "");
    let log = s.write("agy/brain/a1/.system_generated/logs/transcript.jsonl", "");
    let found = subagent(&record, &p, 130.0, &alive);
    assert_eq!(found.path, log.to_string_lossy());
    assert_eq!(found.agent_type, "Antigravity CLI");
}

#[test]
fn the_job_keeps_its_id_when_its_session_gets_one() {
    let s = Scratch::new("deleg-id");
    let (codex, agy) = places(&s);
    let p = Places {
        codex_sessions: &codex,
        antigravity: &agy,
    };
    let before = subagent(&job(Tool::Codex, "m", None, 100.0, 5), &p, 101.0, &alive);
    let after = subagent(
        &job(Tool::Codex, "m", Some("c9"), 100.0, 5),
        &p,
        102.0,
        &alive,
    );
    assert_eq!(before.id, after.id);
}

#[test]
fn the_sessions_of_jobs_are_hidden_from_the_sidebar_and_the_notch() {
    let records = vec![
        job(Tool::Codex, "m", Some("c1"), 1.0, 1),
        job(Tool::Agy, "m", Some("a1"), 2.0, 2),
        job(Tool::Codex, "m", None, 3.0, 3),
    ];
    let hidden = hidden(&records);
    assert!(hidden.contains("codex:c1") && hidden.contains("agy:a1"));
    assert_eq!(hidden.len(), 2, "a job with no session yet hides nothing");
}

#[test]
fn records_are_read_from_the_delegations_folder_of_the_data_folder() {
    let s = Scratch::new("deleg-read");
    assert!(read(s.path()).is_empty());
    record::write(
        &folder(s.path()),
        &job(Tool::Codex, "m", Some("c1"), 1.0, 1),
    )
    .unwrap();
    assert_eq!(read(s.path()).len(), 1);
}

fn named(agent: &str) -> String {
    format!(r#"{{"type":"agent-name","agentName":"{agent}"}}"#) + "\n"
}

fn cleared(agent: &str) -> String {
    format!(r#"{{"type":"system","agentName":"{agent}","content":"SessionStart:clear"}}"#) + "\n"
}

#[test]
fn a_session_is_matched_by_its_transcripts_id_and_by_those_its_clears_ended() {
    let s = Scratch::new("deleg-clear");
    s.write("p/old-id.jsonl", &named("worker"));
    std::thread::sleep(Duration::from_millis(50));
    let new = s.write("p/new-id.jsonl", &cleared("worker"));
    let ids = parent_ids(&new.to_string_lossy(), "worker");
    assert_eq!(ids, ["new-id", "old-id"]);

    // The job handed over before the clear still shows for the row you see now.
    let (codex, agy) = places(&s);
    let p = Places {
        codex_sessions: &codex,
        antigravity: &agy,
    };
    let records = vec![
        job(Tool::Codex, "old-id", Some("c1"), 100.0, 1),
        job(Tool::Codex, "new-id", Some("c2"), 200.0, 2),
        job(Tool::Codex, "unrelated", Some("c3"), 150.0, 3),
    ];
    let shown = subagents(&records, &ids, &p, 300.0, &alive);
    assert_eq!(shown.len(), 2);

    // A session no clear started is only itself.
    let lone = s.write("q/lone.jsonl", &named("worker"));
    assert_eq!(parent_ids(&lone.to_string_lossy(), "worker"), ["lone"]);
}
