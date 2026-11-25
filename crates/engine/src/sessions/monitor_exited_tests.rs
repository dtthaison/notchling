use super::agent::{Agent, AgentGroup, AgentTarget};
use super::exited_store::ExitedStore;
use super::lanes::Roots;
use super::monitor::{Monitor, Probes};
use super::testutil::TempDir;
use std::path::PathBuf;

const NOW: i64 = 1_793_700_000_000;
const HOUR: i64 = 3_600_000;

const LANE: &str =
    r#"[{"name":"b","path":"TMP/lanes/b","accountID":"acc","email":"b@example.com"}]"#;

fn no_host(_: u32) -> Option<String> {
    None
}

fn poll_at(monitor: &mut Monitor, roots: &Roots, dead: &'static [u32], now: i64) -> Vec<Agent> {
    let is_alive = move |pid: u32| !dead.contains(&pid);
    let probes = Probes {
        is_alive: &is_alive,
        host_app: &no_host,
    };
    monitor.poll(roots, &probes, now)
}

fn poll(monitor: &mut Monitor, roots: &Roots, dead: &'static [u32]) -> Vec<Agent> {
    poll_at(monitor, roots, dead, NOW)
}

struct World {
    tmp: TempDir,
    roots: Roots,
    file: PathBuf,
}

impl World {
    fn new() -> World {
        let tmp = TempDir::new("monitor-exited");
        let lanes = LANE.replace("TMP", &tmp.root().display().to_string());
        tmp.write("data/lanes.json", &lanes);
        let roots = Roots::new(&tmp.path("home"), &tmp.path("data"));
        let file = tmp.path("data/daemon/exited-sessions.json");
        World { tmp, roots, file }
    }

    fn monitor(&self) -> Monitor {
        Monitor::default().with_exited_store(ExitedStore::new(self.file.clone()))
    }

    fn session(&self, rel: &str, id: &str, pid: u32) {
        self.tmp.write(
            rel,
            &format!(
                r#"{{"sessionId":"{id}","pid":{pid},"status":"busy","cwd":"/w/{id}","statusUpdatedAt":{}}}"#,
                NOW - 1000
            ),
        );
    }

    fn stored(&self) -> Vec<String> {
        let mut ids: Vec<String> = ExitedStore::new(self.file.clone())
            .load()
            .into_iter()
            .map(|a| a.id)
            .collect();
        ids.sort();
        ids
    }
}

fn exited(rows: &[Agent]) -> Vec<&Agent> {
    rows.iter().filter(|a| a.label == "Exited").collect()
}

fn ends_one_session(w: &World, monitor: &mut Monitor) {
    let file = w.tmp.path("lanes/b/sessions/100.json");
    w.session("lanes/b/sessions/100.json", "s", 100);
    poll(monitor, &w.roots, &[]);
    poll(monitor, &w.roots, &[100]);
    std::fs::remove_file(file).unwrap();
}

#[test]
fn an_exited_session_is_still_listed_after_the_monitor_restarts() {
    let w = World::new();
    let mut first = w.monitor();
    ends_one_session(&w, &mut first);
    let before = poll(&mut first, &w.roots, &[100]);
    assert_eq!(exited(&before).len(), 1);
    drop(first);

    let mut restarted = w.monitor();
    let after = poll(&mut restarted, &w.roots, &[]);

    assert_eq!(after, before);
    let row = &after[0];
    assert_eq!(
        (row.group, row.label.as_str()),
        (AgentGroup::Completed, "Exited")
    );
    assert_eq!(row.since_ms, NOW);
    assert_eq!(
        row.target,
        AgentTarget::Resume {
            session_id: "s".into(),
            cwd: "/w/s".into()
        }
    );
    assert_eq!(
        row.config_dir,
        Some(format!("{}/lanes/b", w.tmp.root().display()))
    );
}

#[test]
fn a_session_running_again_under_the_same_id_wins_over_the_stored_row() {
    let w = World::new();
    let mut first = w.monitor();
    ends_one_session(&w, &mut first);
    assert_eq!(w.stored(), vec!["s"]);
    drop(first);

    w.session("home/.claude/sessions/200.json", "s", 200);
    let mut restarted = w.monitor();
    let rows = poll(&mut restarted, &w.roots, &[]);

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].group, AgentGroup::Working);
    assert_ne!(rows[0].label, "Exited");
    assert!(w.stored().is_empty(), "the live row is not kept as exited");
}

#[test]
fn a_job_under_the_same_id_wins_over_the_stored_row() {
    let w = World::new();
    let mut first = w.monitor();
    ends_one_session(&w, &mut first);
    drop(first);

    w.tmp.write(
        "home/.claude/jobs/ab12cd34/state.json",
        r#"{"state":"working","sessionId":"s","cwd":"/w/s","updatedAt":"2026-11-03T08:00:00.000Z"}"#,
    );
    let mut restarted = w.monitor();
    let rows = poll(&mut restarted, &w.roots, &[]);

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].group, AgentGroup::Working);
    assert!(w.stored().is_empty());
}

#[test]
fn a_missing_store_starts_empty_and_writes_nothing() {
    let w = World::new();
    let mut monitor = w.monitor();
    assert!(poll(&mut monitor, &w.roots, &[]).is_empty());
    assert!(!w.file.exists());
}

#[test]
fn a_corrupt_store_starts_empty_and_the_next_exit_replaces_it() {
    for damaged in [
        "",
        "not json",
        "{\"exited\":[{\"id\":\"s\"",
        "[1,2,3]",
        "{\"exited\":\"x\"}",
        "{\"exited\":null}",
        "\u{0}\u{0}\u{0}",
    ] {
        let w = World::new();
        w.tmp.write("data/daemon/exited-sessions.json", damaged);
        let mut monitor = w.monitor();
        assert!(
            poll(&mut monitor, &w.roots, &[]).is_empty(),
            "starts empty over {damaged:?}"
        );
        ends_one_session(&w, &mut monitor);
        assert_eq!(w.stored(), vec!["s"], "rewritten over {damaged:?}");
    }
}

#[test]
fn a_damaged_row_is_skipped_and_the_others_are_restored() {
    let w = World::new();
    w.tmp.write(
        "data/daemon/exited-sessions.json",
        &format!(
            r#"{{"exited":[1,"x",{{"id":"","sinceMs":{NOW}}},{{"name":"no id","sinceMs":{NOW}}},{{"id":"no-time"}},{{"id":"ok","cwd":"/w/batoan","sinceMs":{NOW}}}]}}"#
        ),
    );
    let mut monitor = w.monitor();
    let rows = poll(&mut monitor, &w.roots, &[]);

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "ok");
    assert_eq!(rows[0].name, "batoan");
    assert_eq!(rows[0].project, "batoan");
    assert_eq!(rows[0].label, "Exited");
}

#[test]
fn rows_older_than_the_window_are_dropped_and_forgotten() {
    let w = World::new();
    w.tmp.write(
        "data/daemon/exited-sessions.json",
        &format!(
            r#"{{"exited":[{{"id":"old","cwd":"/w/old","sinceMs":{}}},{{"id":"recent","cwd":"/w/recent","sinceMs":{}}}]}}"#,
            NOW - 25 * HOUR,
            NOW - HOUR
        ),
    );
    let mut monitor = w.monitor();
    let rows = poll(&mut monitor, &w.roots, &[]);

    assert_eq!(
        rows.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
        vec!["recent"]
    );
    assert_eq!(w.stored(), vec!["recent"]);
}

#[test]
fn a_list_that_did_not_change_is_not_written_again() {
    let w = World::new();
    let mut monitor = w.monitor();
    ends_one_session(&w, &mut monitor);
    assert_eq!(w.stored(), vec!["s"]);
    w.tmp.write("data/daemon/exited-sessions.json", "marker");

    poll(&mut monitor, &w.roots, &[100]);
    poll(&mut monitor, &w.roots, &[100]);

    assert_eq!(std::fs::read_to_string(&w.file).unwrap(), "marker");
}

const JOB_UPDATED: &str = "2026-11-03T08:00:00.000Z";

fn job_state(sess: &str, resume: Option<&str>) -> String {
    let resume = resume.map_or(String::new(), |r| format!(r#""resumeSessionId":"{r}","#));
    format!(
        r#"{{"state":"working","sessionId":"{sess}",{resume}"cwd":"/w/s","updatedAt":"{JOB_UPDATED}"}}"#
    )
}

fn only_the_job(rows: &[Agent], job_session: &str) {
    assert_eq!(
        rows.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
        vec![job_session],
        "{rows:?}"
    );
    assert_eq!(rows[0].group, AgentGroup::Working);
}

#[test]
fn a_job_resuming_an_exited_session_takes_its_row_over() {
    let w = World::new();
    let mut first = w.monitor();
    ends_one_session(&w, &mut first);
    drop(first);

    w.tmp.write(
        "home/.claude/jobs/ab12cd34/state.json",
        &job_state("job-session", Some("s")),
    );
    w.tmp.write(
        "home/.claude/sessions/200.json",
        r#"{"sessionId":"s","jobId":"ab12cd34","pid":200,"status":"busy","kind":"bg"}"#,
    );
    let mut restarted = w.monitor();
    let rows = poll(&mut restarted, &w.roots, &[]);

    only_the_job(&rows, "job-session");
    assert!(w.stored().is_empty());
}

#[test]
fn a_job_resuming_an_exited_session_takes_its_row_over_before_its_session_file_exists() {
    let w = World::new();
    let mut first = w.monitor();
    ends_one_session(&w, &mut first);
    drop(first);

    w.tmp.write(
        "home/.claude/jobs/ab12cd34/state.json",
        &job_state("job-session", Some("s")),
    );
    let mut restarted = w.monitor();
    let rows = poll(&mut restarted, &w.roots, &[]);

    only_the_job(&rows, "job-session");
    assert!(w.stored().is_empty());
}

#[test]
fn a_parked_session_is_not_kept_as_exited_beside_its_job() {
    let w = World::new();
    let mut first = w.monitor();
    ends_one_session(&w, &mut first);
    drop(first);

    w.tmp.write(
        "home/.claude/jobs/ab12cd34/state.json",
        &job_state("job-session", None),
    );
    w.tmp.write(
        "home/.claude/sessions/200.json",
        r#"{"sessionId":"s","parkedJobId":"ab12cd34","pid":200,"status":"idle"}"#,
    );
    let mut restarted = w.monitor();
    let rows = poll(&mut restarted, &w.roots, &[]);

    only_the_job(&rows, "job-session");
    assert!(w.stored().is_empty());
}

#[test]
fn a_live_session_taken_over_by_a_job_never_becomes_exited() {
    let w = World::new();
    let mut monitor = w.monitor();
    w.session("home/.claude/sessions/100.json", "s", 100);
    let before = poll(&mut monitor, &w.roots, &[]);
    assert_eq!(before[0].id, "s");

    w.tmp.write(
        "home/.claude/jobs/ab12cd34/state.json",
        &job_state("job-session", Some("s")),
    );
    w.tmp.write(
        "home/.claude/sessions/100.json",
        r#"{"sessionId":"s","jobId":"ab12cd34","pid":100,"status":"busy","kind":"bg"}"#,
    );
    let rows = poll(&mut monitor, &w.roots, &[]);

    only_the_job(&rows, "job-session");
    assert!(w.stored().is_empty());
}

const JOB_OLD: &str = "2026-10-30T08:00:00.000Z";

fn job_in(state: &str, updated: &str, sess: &str, resume: Option<&str>) -> String {
    let resume = resume.map_or(String::new(), |r| format!(r#""resumeSessionId":"{r}","#));
    format!(
        r#"{{"state":"{state}","sessionId":"{sess}",{resume}"cwd":"/w/j","updatedAt":"{updated}"}}"#
    )
}

fn exit_standalone_s(w: &World, monitor: &mut Monitor) -> Vec<Agent> {
    ends_one_session(w, monitor);
    poll(monitor, &w.roots, &[100])
}

fn exited_ids(rows: &[Agent]) -> Vec<&str> {
    exited(rows).iter().map(|a| a.id.as_str()).collect()
}

#[test]
fn a_finished_job_that_resumed_a_session_does_not_hide_its_later_exit() {
    for state in ["done", "stopped"] {
        let w = World::new();
        w.tmp.write(
            "home/.claude/jobs/ab12cd34/state.json",
            &job_in(state, JOB_UPDATED, "job-session", Some("s")),
        );
        let mut monitor = w.monitor();
        let rows = exit_standalone_s(&w, &mut monitor);

        assert_eq!(exited_ids(&rows), vec!["s"], "{state}: {rows:?}");
        assert!(rows.iter().any(|a| a.id == "job-session"), "{state}");
        assert_eq!(w.stored(), vec!["s".to_string()], "{state}");
    }
}

#[test]
fn an_expired_job_that_resumed_a_session_does_not_hide_its_later_exit() {
    for state in ["running", "done"] {
        let w = World::new();
        w.tmp.write(
            "home/.claude/jobs/ab12cd34/state.json",
            &job_in(state, JOB_OLD, "job-session", Some("s")),
        );
        let mut monitor = w.monitor();
        let rows = exit_standalone_s(&w, &mut monitor);

        assert_eq!(exited_ids(&rows), vec!["s"], "{state}: {rows:?}");
        assert!(!rows.iter().any(|a| a.id == "job-session"), "{state}");
        assert_eq!(w.stored(), vec!["s".to_string()], "{state}");
    }
}

#[test]
fn an_expired_job_with_the_same_id_does_not_hide_the_exit_of_that_session() {
    let w = World::new();
    w.tmp.write(
        "home/.claude/jobs/ab12cd34/state.json",
        &job_in("done", JOB_OLD, "s", None),
    );
    let mut monitor = w.monitor();
    let rows = exit_standalone_s(&w, &mut monitor);

    assert_eq!(exited_ids(&rows), vec!["s"], "{rows:?}");
    assert_eq!(w.stored(), vec!["s".to_string()]);
}

#[test]
fn a_finished_job_leaves_the_stored_row_of_the_session_it_resumed() {
    let w = World::new();
    let mut first = w.monitor();
    ends_one_session(&w, &mut first);
    drop(first);

    w.tmp.write(
        "home/.claude/jobs/ab12cd34/state.json",
        &job_in("done", JOB_UPDATED, "job-session", Some("s")),
    );
    let mut restarted = w.monitor();
    let rows = poll(&mut restarted, &w.roots, &[]);

    assert_eq!(exited_ids(&rows), vec!["s"], "{rows:?}");
    assert_eq!(w.stored(), vec!["s".to_string()]);
}
