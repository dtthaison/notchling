mod common;

use common::*;
use notchling_engine::sessions::jobs::agent_from_job;
use notchling_engine::transcript::locate::{
    begins_with_clear, find_in_folders, find_transcript, job_transcript, job_transcript_live,
    successor, JobPaths,
};
use serde_json::json;
use std::time::Duration;

const LINK: &str = "/h/.claude/projects/-Users-me-app/new-conv.jsonl";
const IN_WORKTREE: &str =
    "/h/.claude/projects/-Users-me-app--claude-worktrees-music/new-conv.jsonl";
const OLD_IN_WORKTREE: &str =
    "/h/.claude/projects/-Users-me-app--claude-worktrees-music/old-conv.jsonl";
const OLD_ELSEWHERE: &str =
    "/h/.claude/projects/-Users-me-app--claude-worktrees-sound/old-conv.jsonl";
const WORKTREE: &str = "/Users/me/app/.claude/worktrees/music";

#[test]
fn a_cleared_job_is_found_by_the_link_stem_in_its_worktree_folder() {
    let found = job_transcript(Some(LINK), Some("old-conv"), Some(WORKTREE), |p| {
        (p == IN_WORKTREE).then_some(9.0)
    });
    assert_eq!(
        found.as_deref(),
        Some(IN_WORKTREE),
        "the job's state names the new conversation in the project folder of its cwd, but Claude Code wrote the file in the worktree's folder"
    );
}

#[test]
fn the_older_conversation_does_not_win_over_the_new_one() {
    let dates = |p: &str| match p {
        OLD_IN_WORKTREE => Some(5.0),
        IN_WORKTREE => Some(9.0),
        OLD_ELSEWHERE => Some(99.0),
        _ => None,
    };
    assert_eq!(
        job_transcript(Some(LINK), Some("old-conv"), Some(WORKTREE), dates).as_deref(),
        Some(IN_WORKTREE)
    );
}

#[test]
fn the_recorded_path_stays_when_nothing_else_exists() {
    assert_eq!(
        job_transcript(Some(LINK), Some("old-conv"), Some(WORKTREE), |_| None).as_deref(),
        Some(LINK)
    );
}

#[test]
fn a_job_with_no_session_id_still_looks_for_the_stem_in_its_worktree() {
    let found = job_transcript(Some(LINK), None, Some(WORKTREE), |p| {
        (p == IN_WORKTREE).then_some(1.0)
    });
    assert_eq!(found.as_deref(), Some(IN_WORKTREE));
}

#[test]
fn the_recorded_path_wins_when_it_exists_and_is_newest() {
    let dates = |p: &str| match p {
        LINK => Some(20.0),
        IN_WORKTREE => Some(10.0),
        _ => None,
    };
    assert_eq!(
        job_transcript(Some(LINK), Some("old-conv"), Some(WORKTREE), dates).as_deref(),
        Some(LINK)
    );
}

const CWD_FILE: &str = "/h/.claude/projects/-Users-me-app/resume-conv.jsonl";
const CWD: &str = "/Users/me/app";

fn live<'a>(resume: Option<&'a str>, worktree: Option<&'a str>) -> JobPaths<'a> {
    JobPaths {
        link_scan_path: Some(LINK),
        session_id: Some("old-conv"),
        resume_session_id: resume,
        worktree_path: worktree,
        cwd: Some(CWD),
    }
}

#[test]
fn the_conversation_the_job_resumed_into_is_found_beside_the_cwd() {
    let found = job_transcript_live(&live(Some("resume-conv"), None), |p| {
        (p == CWD_FILE).then_some(9.0)
    });
    assert_eq!(found.as_deref(), Some(CWD_FILE));
}

#[test]
fn the_resumed_conversation_in_the_worktree_folder_is_found() {
    let in_worktree = "/h/.claude/projects/-Users-me-app--claude-worktrees-music/resume-conv.jsonl";
    let found = job_transcript_live(&live(Some("resume-conv"), Some(WORKTREE)), |p| {
        (p == in_worktree).then_some(9.0)
    });
    assert_eq!(found.as_deref(), Some(in_worktree));
}

#[test]
fn an_idle_older_conversation_never_hides_the_live_one() {
    let dates = |p: &str| match p {
        LINK => Some(5.0),
        CWD_FILE => Some(9.0),
        _ => None,
    };
    assert_eq!(
        job_transcript_live(&live(Some("resume-conv"), None), dates).as_deref(),
        Some(CWD_FILE)
    );
    let dates = |p: &str| match p {
        LINK => Some(20.0),
        CWD_FILE => Some(9.0),
        _ => None,
    };
    assert_eq!(
        job_transcript_live(&live(Some("resume-conv"), None), dates).as_deref(),
        Some(LINK),
        "the file written last is the live one, whichever id names it"
    );
}

#[test]
fn a_file_of_another_projects_folder_is_never_a_candidate() {
    let other = "/h/.claude/projects/-Users-me-elsewhere/resume-conv.jsonl";
    let found = job_transcript_live(&live(Some("resume-conv"), Some(WORKTREE)), |p| {
        (p == other).then_some(99.0)
    });
    assert_eq!(found.as_deref(), Some(LINK));
}

#[test]
fn the_agent_of_a_resumed_job_reads_the_resumed_conversation() {
    let job = json!({"state": "done", "cwd": CWD, "sessionId": "old-conv",
        "resumeSessionId": "resume-conv", "linkScanPath": LINK});
    let agent = agent_from_job(&job, "abc12345", None, &|p| (p == CWD_FILE).then_some(9.0));
    assert_eq!(agent.transcript_path.as_deref(), Some(CWD_FILE));
    assert_eq!(agent.resume_session_id.as_deref(), Some("resume-conv"));
}

#[test]
fn the_folder_lookup_never_scans_unrelated_projects() {
    let scratch = Scratch::new("folders");
    let projects = s(&scratch.path("projects"));
    scratch.write("projects/-unrelated/wanted.jsonl", "{}\n");
    scratch.write("projects/-work-demo/other.jsonl", "{}\n");
    let ids = ["wanted".to_string()];
    assert_eq!(
        find_in_folders(&projects, &ids, &["/work/demo"]),
        None,
        "another project's folder is not searched"
    );
    assert!(
        find_transcript(&projects, "wanted", "/work/demo").is_some(),
        "the older lookup does scan, which is why the courier no longer uses it"
    );
    scratch.write("projects/-work-demo/wanted.jsonl", "{}\n");
    assert_eq!(
        find_in_folders(&projects, &ids, &["/work/demo"]),
        Some(s(&scratch.path("projects/-work-demo/wanted.jsonl")))
    );
}

fn cleared(agent: &str) -> String {
    format!(r#"{{"type":"system","agentName":"{agent}","content":"SessionStart:clear"}}"#) + "\n"
}

fn tick() {
    std::thread::sleep(Duration::from_millis(30));
}

#[test]
fn a_long_chain_of_clears_is_followed_to_its_end() {
    let scratch = Scratch::new("chain6");
    let start = s(&scratch.write("start.jsonl", "{}\n"));
    let mut last = String::new();
    for n in 1..=6 {
        tick();
        last = s(&scratch.write(&format!("c{n}.jsonl"), &cleared("worker")));
    }
    assert_eq!(
        successor(&start, "worker"),
        Some(last),
        "six clears in a row are six hops, and two files made within 10 s of each other do not loop"
    );
}

#[test]
fn a_chain_longer_than_the_cap_gives_no_answer_instead_of_a_wrong_one() {
    let scratch = Scratch::new("chain40");
    let start = s(&scratch.write("start.jsonl", "{}\n"));
    for n in 10..=50 {
        std::thread::sleep(Duration::from_millis(3));
        scratch.write(&format!("c{n}.jsonl"), &cleared("worker"));
    }
    assert_eq!(successor(&start, "worker"), None);
}

#[test]
fn a_file_that_opens_with_a_clear_says_so() {
    let scratch = Scratch::new("opens-clear");
    let head = |lines: &[&str]| s(&scratch.write("f.jsonl", &(lines.join("\n") + "\n")));
    let typed = r#"{"type":"user","message":{"role":"user","content":"<command-name>/clear</command-name>\n<command-message>clear</command-message>"}}"#;
    assert!(begins_with_clear(&head(&[
        r#"{"type":"mode","mode":"normal"}"#,
        typed
    ])));
    let ran =
        r#"{"type":"system","subtype":"local_command","commandRun":{"command":"clear","args":""}}"#;
    assert!(begins_with_clear(&head(&[ran])));
    assert!(begins_with_clear(&head(&[
        r#"{"type":"system","content":"SessionStart:clear"}"#
    ])));
    assert!(!begins_with_clear(&head(&[
        r#"{"type":"user","message":{"role":"user","content":"hello"}}"#
    ])));
    assert!(!begins_with_clear(&s(&scratch.path("missing.jsonl"))));
}
