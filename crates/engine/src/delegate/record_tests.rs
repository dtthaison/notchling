use super::*;
use crate::delegate::test_support::scratch;
use std::time::Duration;

fn options(tool: Tool, prompt: &str) -> Options {
    Options {
        tool,
        prompt: prompt.into(),
        model: Some("gpt-5.5".into()),
        effort: Some("high".into()),
        folder: "/work".into(),
        out: None,
        write: false,
        session: None,
        timeout: Duration::from_secs(30),
    }
}

fn started(tool: Tool, now: f64, pid: u32) -> Delegation {
    Delegation::new(
        "parent-1",
        "/work",
        &options(tool, "Review the diff\nmore"),
        now,
        pid,
    )
}

fn dead(_: u32) -> bool {
    false
}
fn alive(_: u32) -> bool {
    true
}

#[test]
fn the_description_is_the_first_line_cut_to_80_characters() {
    assert_eq!(
        describe("\n\n  Review the diff \nsecond line"),
        "Review the diff"
    );
    assert_eq!(describe("   \n"), "Delegated job");
    let long = "é".repeat(200);
    let d = describe(&long);
    assert_eq!(d.chars().count(), DESCRIPTION_CHARS);
    assert!(d.ends_with('…'));
    assert_eq!(describe(&"a".repeat(80)), "a".repeat(80));
}

#[test]
fn a_new_record_is_pending_until_the_session_is_known() {
    let mut r = started(Tool::Codex, 100.0, 4242);
    assert_eq!(r.file_name(), "codex-pending-4242.json");
    assert_eq!(r.row_id(), None);
    assert_eq!(r.description, "Review the diff");
    r.session = Some("019a-b".into());
    assert_eq!(r.file_name(), "codex-019a-b.json");
    assert_eq!(r.row_id().as_deref(), Some("codex:019a-b"));
    let mut agy = started(Tool::Agy, 100.0, 1);
    agy.session = Some("c0ffee".into());
    assert_eq!(agy.row_id().as_deref(), Some("agy:c0ffee"));
}

#[test]
fn a_jobs_id_survives_its_session_getting_an_id() {
    let mut r = started(Tool::Codex, 100.5, 4242);
    let before = r.id();
    assert_eq!(before, "codex-4242-100500");
    r.session = Some("sess".into());
    assert_eq!(r.id(), before);
}

#[test]
fn a_session_id_that_could_name_a_path_is_not_used() {
    let mut o = options(Tool::Codex, "x");
    o.session = Some("../../etc/passwd".into());
    let r = Delegation::new("p", "/w", &o, 1.0, 7);
    assert_eq!(
        r.session, None,
        "a --session that isn't a plain id isn't recorded"
    );
    let dir = scratch("rec-unsafe");
    let rec = Recorder::start(&dir, r, 1.0);
    rec.session_known("../x", None, None);
    let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
    assert_eq!(names.len(), 1);
    assert_eq!(
        names[0].file_name().to_string_lossy(),
        "codex-pending-7.json"
    );
}

#[test]
fn state_follows_the_end_the_process_and_the_age() {
    let r = started(Tool::Codex, 1000.0, 9);
    assert_eq!(r.state(1010.0, &alive), State::Running);
    assert_eq!(
        r.state(1010.0, &dead),
        State::Ended(End::Stopped),
        "no end written and the process gone"
    );
    assert_eq!(
        r.state(1000.0 + STALE_AFTER_SECS + 1.0, &alive),
        State::Ended(End::Stopped),
        "a reused pid doesn't keep it running for ever"
    );
    let mut done = r.clone();
    done.ended = Some(1020.0);
    done.outcome = Some(End::Failed);
    assert_eq!(done.state(1030.0, &alive), State::Ended(End::Failed));
    assert_eq!(done.state(1030.0, &dead), State::Ended(End::Failed));
}

#[test]
fn records_are_written_privately_read_back_and_sorted_oldest_first() {
    let dir = scratch("rec-rw");
    let mut late = started(Tool::Agy, 300.0, 2);
    late.session = Some("s2".into());
    let mut early = started(Tool::Codex, 100.0, 1);
    early.session = Some("s1".into());
    write(&dir, &late).unwrap();
    write(&dir, &early).unwrap();
    std::fs::write(dir.join("junk.json"), "not json").unwrap();
    std::fs::write(dir.join("note.txt"), "{}").unwrap();
    let read = read_all(&dir);
    assert_eq!(read, vec![early.clone(), late.clone()]);
    assert_eq!(hidden_rows(&read), vec!["codex:s1", "agy:s2"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("codex-s1.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "0600");
    }
    assert!(read_all(&dir.join("missing")).is_empty());
}

#[test]
fn a_record_from_a_newer_version_is_left_out() {
    let dir = scratch("rec-version");
    let mut r = started(Tool::Codex, 1.0, 1);
    r.session = Some("s".into());
    r.version = Delegation::VERSION + 1;
    write(&dir, &r).unwrap();
    assert!(read_all(&dir).is_empty());
}

#[test]
fn the_recorder_moves_the_pending_file_to_the_session_and_ends_it() {
    let dir = scratch("rec-flow");
    let rec = Recorder::start(&dir, started(Tool::Codex, 100.0, 55), 100.0);
    assert_eq!(read_all(&dir)[0].file_name(), "codex-pending-55.json");
    rec.session_known("sess-1", Some("gpt-5.5"), None);
    let after_id = read_all(&dir);
    assert_eq!(after_id.len(), 1, "the pending file is gone");
    assert_eq!(after_id[0].session.as_deref(), Some("sess-1"));
    assert_eq!(after_id[0].ended, None);

    let mut outcome = Outcome::new(Tool::Codex);
    outcome.session = Some("sess-1".into());
    outcome.effort = Some("low".into());
    rec.finish(&outcome, false, 130.0);
    let last = &read_all(&dir)[0];
    assert_eq!(last.ended, Some(130.0));
    assert_eq!(last.outcome, Some(End::Ok));
    assert_eq!(last.effort.as_deref(), Some("low"));
    assert_eq!(last.model.as_deref(), Some("gpt-5.5"));
}

#[test]
fn the_outcome_decides_the_end() {
    let mut o = Outcome::new(Tool::Agy);
    assert_eq!(end_of(&o, false), End::Ok);
    o.error = Some("boom".into());
    assert_eq!(end_of(&o, false), End::Failed);
    o.timed_out = true;
    assert_eq!(end_of(&o, false), End::Failed);
    assert_eq!(end_of(&o, true), End::Stopped);
}

#[test]
fn prune_removes_old_finished_records_and_keeps_running_and_recent_ones() {
    let dir = scratch("rec-prune");
    let now = 10.0 * KEEP_SECS;
    let mk = |tool: Tool, id: &str, at: f64, ended: bool| {
        let mut r = started(tool, at, 77);
        r.session = Some(id.into());
        if ended {
            r.ended = Some(at + 1.0);
            r.outcome = Some(End::Ok);
        }
        write(&dir, &r).unwrap();
    };
    mk(Tool::Codex, "old-done", now - KEEP_SECS - 10.0, true);
    mk(Tool::Codex, "recent-done", now - 100.0, true);
    mk(Tool::Agy, "old-dead", now - KEEP_SECS - 10.0, false);
    prune(&dir, now, &dead);
    let left: Vec<String> = read_all(&dir)
        .iter()
        .filter_map(Delegation::row_id)
        .collect();
    assert_eq!(left, vec!["codex:recent-done"]);
}

#[cfg(unix)]
#[test]
fn pid_alive_knows_this_process_and_not_a_dead_one() {
    assert!(pid_alive(std::process::id()));
    let mut child = std::process::Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .spawn()
        .unwrap();
    let pid = child.id();
    child.wait().unwrap();
    assert!(!pid_alive(pid));
    assert!(!pid_alive(0));
}
