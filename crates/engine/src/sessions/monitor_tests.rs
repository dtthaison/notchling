//! `Monitor::poll` over temp-dir trees. Nothing reads the real `~/.claude`.

use super::agent::{Agent, AgentGroup, AgentTarget};
use super::lanes::Roots;
use super::monitor::{Monitor, Probes};
use super::testutil::TempDir;

const NOW: i64 = 1_793_700_000_000; // 2026-11-03

fn alive(_: u32) -> bool {
    true
}
fn no_host(_: u32) -> Option<String> {
    None
}

fn poll(monitor: &mut Monitor, roots: &Roots, dead: &'static [u32]) -> Vec<Agent> {
    let is_alive = move |pid: u32| !dead.contains(&pid);
    let probes = Probes { is_alive: &is_alive, host_app: &no_host };
    monitor.poll(roots, &probes, NOW)
}

struct World {
    tmp: TempDir,
    roots: Roots,
}

fn world(lanes_json: Option<&str>) -> World {
    let tmp = TempDir::new("monitor");
    if let Some(json) = lanes_json {
        let json = json.replace("TMP", &tmp.root().display().to_string());
        tmp.write("data/lanes.json", &json);
    }
    let roots = Roots::new(&tmp.path("home"), &tmp.path("data"));
    World { tmp, roots }
}

const LANE: &str = r#"[{"name":"b","path":"TMP/lanes/b","accountID":"acc","email":"b@example.com"}]"#;

fn iso(day: u32) -> String {
    format!("2026-11-{day:02}T08:00:00.000Z")
}

#[test]
fn lists_a_main_job_and_a_lane_job_sorted_by_group_then_recency() {
    let w = world(Some(LANE));
    w.tmp.write(
        "home/.claude/jobs/aa11bb22/state.json",
        &format!(r#"{{"state":"done","sessionId":"s-main","name":"main","cwd":"/w/main","updatedAt":"{}"}}"#, iso(3)),
    );
    w.tmp.write(
        "lanes/b/jobs/cc33dd44/state.json",
        &format!(r#"{{"state":"working","sessionId":"s-lane","name":"lane","cwd":"/w/lane","updatedAt":"{}"}}"#, iso(3)),
    );
    let rows = poll(&mut Monitor::default(), &w.roots, &[]);
    assert_eq!(rows.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["s-lane", "s-main"]);
    assert_eq!(rows[0].group, AgentGroup::Working);
    assert_eq!(rows[0].config_dir.as_deref(), Some(format!("{}/lanes/b", w.tmp.root().display()).as_str()));
    assert_eq!(rows[1].config_dir, None);
}

#[test]
fn a_moved_job_shows_once_as_its_current_copy() {
    let w = world(Some(LANE));
    w.tmp.write(
        "home/.claude/jobs/ab12cd34/state.json",
        &format!(r#"{{"state":"stopped","sessionId":"conv","name":"x","cwd":"/w","updatedAt":"{}"}}"#, iso(2)),
    );
    w.tmp.write(
        "lanes/b/jobs/ab12cd34/state.json",
        &format!(r#"{{"state":"working","sessionId":"conv","name":"x","cwd":"/w","updatedAt":"{}"}}"#, iso(3)),
    );
    let mut monitor = Monitor::default();
    let rows = poll(&mut monitor, &w.roots, &[]);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].config_dir.is_some());
    assert_eq!(rows[0].group, AgentGroup::Working);
    assert_eq!(monitor.config_dir_for_session("conv"), rows[0].config_dir);
}

#[test]
fn moved_back_to_main_forgets_the_lane() {
    let w = world(Some(LANE));
    w.tmp.write(
        "lanes/b/jobs/ab12cd34/state.json",
        &format!(r#"{{"state":"stopped","sessionId":"conv","cwd":"/w","updatedAt":"{}"}}"#, iso(2)),
    );
    let mut monitor = Monitor::default();
    poll(&mut monitor, &w.roots, &[]);
    assert!(monitor.config_dir_for_session("conv").is_some());
    w.tmp.write(
        "home/.claude/jobs/ab12cd34/state.json",
        &format!(r#"{{"state":"running","sessionId":"conv","cwd":"/w","updatedAt":"{}"}}"#, iso(3)),
    );
    let rows = poll(&mut monitor, &w.roots, &[]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].config_dir, None);
    assert_eq!(monitor.config_dir_for_session("conv"), None);
}

#[test]
fn the_live_session_file_sets_the_job_status() {
    let w = world(None);
    w.tmp.write(
        "home/.claude/jobs/ab12cd34/state.json",
        &format!(r#"{{"state":"working","sessionId":"conv","cwd":"/w","updatedAt":"{}"}}"#, iso(3)),
    );
    w.tmp.write("home/.claude/sessions/100.json", r#"{"sessionId":"conv","jobId":"ab12cd34","pid":100,"status":"idle"}"#);
    let rows = poll(&mut Monitor::default(), &w.roots, &[]);
    assert_eq!(rows.len(), 1, "the session is the job's, not a second row");
    assert_eq!((rows[0].group, rows[0].label.as_str()), (AgentGroup::Completed, "Idle"));
}

#[test]
fn a_parked_terminal_session_is_shown_as_its_job() {
    let w = world(None);
    w.tmp.write(
        "home/.claude/jobs/ab12cd34/state.json",
        &format!(r#"{{"state":"working","sessionId":"conv","cwd":"/w","updatedAt":"{}"}}"#, iso(2)),
    );
    w.tmp.write("home/.claude/sessions/100.json", r#"{"sessionId":"term","parkedJobId":"ab12cd34","pid":100,"status":"idle"}"#);
    let rows = poll(&mut Monitor::default(), &w.roots, &[]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].target, AgentTarget::Job("ab12cd34".into()));
}

#[test]
fn headless_sessions_are_listed_as_claude_p() {
    let w = world(None);
    w.tmp.write(
        "home/.claude/sessions/100.json",
        &format!(r#"{{"sessionId":"s","pid":100,"status":"busy","cwd":"/w/app","entrypoint":"sdk-cli","statusUpdatedAt":{}}}"#, NOW - 1000),
    );
    let rows = poll(&mut Monitor::default(), &w.roots, &[]);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].headless);
    assert_eq!(rows[0].host, "claude -p");
    assert!(!rows[0].can_reply());
}

#[test]
fn an_ended_session_becomes_exited_and_stays() {
    let w = world(None);
    w.tmp.write(
        "home/.claude/sessions/100.json",
        &format!(r#"{{"sessionId":"s","pid":100,"status":"busy","cwd":"/w/app","statusUpdatedAt":{}}}"#, NOW - 1000),
    );
    let mut monitor = Monitor::default();
    let first = poll(&mut monitor, &w.roots, &[]);
    assert_eq!(first[0].group, AgentGroup::Working);
    // The process died: its file is still there but the pid is gone.
    let second = poll(&mut monitor, &w.roots, &[100]);
    assert_eq!(second.len(), 1);
    assert_eq!((second[0].group, second[0].label.as_str()), (AgentGroup::Completed, "Exited"));
    assert_eq!(second[0].target, AgentTarget::Resume { session_id: "s".into(), cwd: "/w/app".into() });
    let third = poll(&mut monitor, &w.roots, &[100]);
    assert_eq!(third.len(), 1, "an exited row stays");
}

#[test]
fn old_completed_rows_are_hidden() {
    let w = world(None);
    w.tmp.write(
        "home/.claude/jobs/aa11bb22/state.json",
        r#"{"state":"done","sessionId":"old","cwd":"/w","updatedAt":"2026-10-01T00:00:00.000Z"}"#,
    );
    w.tmp.write(
        "home/.claude/jobs/cc33dd44/state.json",
        r#"{"state":"working","sessionId":"old-working","cwd":"/w","updatedAt":"2026-10-01T00:00:00.000Z"}"#,
    );
    let rows = poll(&mut Monitor::default(), &w.roots, &[]);
    assert_eq!(rows.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["old-working"]);
}

#[test]
fn a_job_without_a_transcript_is_a_command_job() {
    let w = world(None);
    w.tmp.write(
        "home/.claude/jobs/ab12cd34/state.json",
        &format!(r#"{{"state":"working","template":"exec","intent":"make > run.log","cwd":"/w","updatedAt":"{}"}}"#, iso(3)),
    );
    let rows = poll(&mut Monitor::default(), &w.roots, &[]);
    assert_eq!(rows[0].command.as_deref(), Some("make > run.log"));
    assert_eq!(rows[0].transcript_path, None);
}

#[test]
fn missing_folders_give_an_empty_list() {
    let w = world(None);
    assert!(poll(&mut Monitor::default(), &w.roots, &[]).is_empty());
    let _ = (alive(1), no_host(1));
}
