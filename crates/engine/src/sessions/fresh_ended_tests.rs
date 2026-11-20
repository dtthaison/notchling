use super::*;
use crate::sessions::testutil::TempDir;
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::time::{Duration, UNIX_EPOCH};

const JOB: &str = "be50d36a";
const STATE: &str = "claude/jobs/be50d36a/state.json";
const LANE_STATE: &str = "lane-b/jobs/be50d36a/state.json";
const PID: u32 = 4_242;
const BEGAN_MS: i64 = 1_795_168_333_000;
const BEGAN: &str = "Fri Nov 20 09:52:13 2026";
const TORN: &str = "{ half a wri";

const ITS_OWN: Process = Process::Ours {
    started_ms: Some(BEGAN_MS),
    ticks: None,
};
const NEWER: Process = Process::Ours {
    started_ms: Some(BEGAN_MS + 3_600_000),
    ticks: None,
};

struct World {
    dir: TempDir,
    roots: Roots,
    processes: RefCell<HashMap<u32, Process>>,
    watch: RefCell<Watch>,
    now: Cell<i64>,
}

fn world(tag: &str) -> World {
    let dir = TempDir::new(tag);
    let lane = dir.path("lane-b");
    let lanes = json!([{
        "name": "b", "path": lane.to_string_lossy(), "accountID": "a2", "email": "b@example.com"
    }]);
    dir.write("lanes.json", &lanes.to_string());
    let roots = Roots {
        main_dir: dir.path("claude"),
        lanes_file: dir.path("lanes.json"),
    };
    World {
        dir,
        roots,
        processes: RefCell::default(),
        watch: RefCell::default(),
        now: Cell::new(0),
    }
}

impl World {
    fn ended(&self) -> Ended {
        let process = |pid: u32| {
            let known = self.processes.borrow().get(&pid).copied();
            known.unwrap_or(Process::Gone)
        };
        let settled = |mark: &Mark| self.watch.borrow_mut().settled(mark, self.now.get());
        let look = Look {
            process: &process,
            settled: &settled,
        };
        job_ended(&self.roots, &look, JOB)
    }

    fn later(&self, ms: i64) -> Ended {
        self.now.set(self.now.get() + ms);
        self.ended()
    }

    fn state(&self, state: &str) {
        self.dir.write(STATE, &job(state));
    }

    fn runs(&self, pid: u32, process: Process) {
        self.processes.borrow_mut().insert(pid, process);
    }

    fn stamp(&self, relative: &str, secs: u64) {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(self.dir.path(relative))
            .unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }
}

fn job(state: &str) -> String {
    json!({"state": state, "updatedAt": "2026-11-14T22:54:49.559Z"}).to_string()
}

fn session(pid: u32) -> serde_json::Value {
    json!({
        "pid": pid, "jobId": JOB, "status": "idle", "procStart": BEGAN,
        "startedAt": BEGAN_MS + 1_600
    })
}

const YEAR_2100_SECS: u64 = 4_102_444_800;

#[test]
fn a_stopped_record_with_no_process_of_the_jobs_is_stopped_at_once() {
    let w = world("ended-stopped");
    w.state("stopped");
    assert_eq!(w.ended(), Ended::Stopped);
    w.dir
        .write("claude/sessions/4242.json", &session(PID).to_string());
    assert_eq!(w.ended(), Ended::Stopped, "its process is gone");
}

#[test]
fn no_readable_record_is_never_stopped_however_old_its_files_are() {
    let w = world("ended-no-record");
    w.dir.write(STATE, TORN);
    for secs in [1_000, 1_794_000_000, YEAR_2100_SECS] {
        w.stamp(STATE, secs);
        assert_eq!(w.ended(), Ended::Unknown, "{secs}");
        assert_eq!(w.later(MID_WRITE_MS), Ended::Unknown, "{secs}");
        assert_eq!(w.later(86_400_000), Ended::Unknown, "{secs}");
    }
    std::fs::remove_file(w.dir.path(STATE)).unwrap();
    assert_eq!(w.ended(), Ended::Unknown, "a folder with no state file");
    assert_eq!(w.later(86_400_000), Ended::Unknown);
    w.dir
        .write("claude/sessions/4242.json", &session(PID).to_string());
    w.runs(PID, ITS_OWN);
    assert_eq!(w.ended(), Ended::No, "its process runs");
}

#[test]
fn a_broken_copy_is_judged_by_how_long_the_daemon_saw_it_unchanged_never_by_its_mtime() {
    for secs in [1_000, 1_794_000_000, YEAR_2100_SECS] {
        let w = world("ended-skew");
        w.state("stopped");
        w.dir.write(LANE_STATE, TORN);
        w.stamp(LANE_STATE, secs);
        assert_eq!(w.ended(), Ended::Unknown, "{secs}: first seen");
        assert_eq!(w.later(MID_WRITE_MS - 1), Ended::Unknown, "{secs}");
        assert_eq!(w.later(1), Ended::Stopped, "{secs}: left behind");
    }
}

#[test]
fn a_copy_still_being_written_starts_the_wait_again() {
    let w = world("ended-rewritten");
    w.state("stopped");
    w.dir.write(LANE_STATE, TORN);
    assert_eq!(w.ended(), Ended::Unknown);
    assert_eq!(w.later(MID_WRITE_MS - 1), Ended::Unknown);
    w.dir.write(LANE_STATE, "{ half a write, a bit mo");
    assert_eq!(w.later(1), Ended::Unknown, "it changed");
    assert_eq!(w.later(MID_WRITE_MS - 1), Ended::Unknown);
    w.dir.write(LANE_STATE, &job("working"));
    assert_eq!(w.later(1), Ended::No, "the newer copy is live");
}

#[test]
fn a_job_folder_with_no_state_file_holds_a_stop_back_only_while_it_is_new() {
    let w = world("ended-empty-folder");
    w.state("stopped");
    w.dir.write("lane-b/jobs/be50d36a/timeline.jsonl", "");
    assert_eq!(w.ended(), Ended::Unknown);
    assert_eq!(w.later(MID_WRITE_MS), Ended::Stopped);
}

#[test]
fn a_restarted_daemon_watches_again_and_still_gets_there() {
    let w = world("ended-restart");
    w.state("done");
    w.dir.write(LANE_STATE, TORN);
    assert_eq!(w.ended(), Ended::Unknown);
    assert_eq!(w.later(MID_WRITE_MS), Ended::Done);
    w.watch.replace(Watch::default());
    w.now.set(0);
    assert_eq!(w.ended(), Ended::Unknown);
    assert_eq!(w.later(MID_WRITE_MS), Ended::Done);
}

#[test]
fn a_process_of_the_jobs_makes_a_stopped_record_unknown_and_any_other_not_ended() {
    let w = world("ended-live");
    w.dir
        .write("claude/sessions/4242.json", &session(PID).to_string());
    w.runs(PID, ITS_OWN);
    for (state, want) in [
        ("stopped", Ended::Unknown),
        ("done", Ended::No),
        ("working", Ended::No),
    ] {
        w.state(state);
        assert_eq!(w.ended(), want, "{state}");
        assert_eq!(w.later(86_400_000), want, "{state}: for as long as it runs");
    }
    w.dir.write(STATE, TORN);
    assert_eq!(w.ended(), Ended::No, "no record, but it runs");
}

#[test]
fn a_pid_that_is_gone_or_another_users_is_not_the_jobs_process() {
    let w = world("ended-eperm");
    w.dir
        .write("claude/sessions/4242.json", &session(PID).to_string());
    for process in [Process::Gone, Process::Other] {
        w.runs(PID, process);
        w.state("stopped");
        assert_eq!(w.ended(), Ended::Stopped, "{process:?}");
        w.state("done");
        assert_eq!(w.ended(), Ended::Done, "{process:?}");
    }
}

#[test]
fn a_reused_pid_is_someone_else_only_when_every_start_fact_disagrees() {
    let w = world("ended-reused");
    w.state("stopped");
    let file = "claude/sessions/4242.json";
    w.dir.write(file, &session(PID).to_string());
    w.runs(PID, NEWER);
    assert_eq!(w.ended(), Ended::Stopped, "started an hour after the file");
    let stepped = Process::Ours {
        started_ms: Some(BEGAN_MS + 3_600_000),
        ticks: Some(88_123),
    };
    let mut counted = session(PID);
    counted["procStart"] = json!("88123");
    w.dir.write(file, &counted.to_string());
    w.runs(PID, stepped);
    assert_eq!(
        w.ended(),
        Ended::Unknown,
        "the clock moved, the ticks match"
    );
    counted["procStart"] = json!("77000");
    w.dir.write(file, &counted.to_string());
    assert_eq!(w.ended(), Ended::Stopped, "other ticks and a later start");
    let mut early = session(PID);
    early["startedAt"] = json!(BEGAN_MS - 3_600_000);
    w.dir.write(file, &early.to_string());
    w.runs(PID, ITS_OWN);
    assert_eq!(w.ended(), Ended::Unknown, "its recorded start matches");
    let mut undated = session(PID);
    undated["procStart"] = json!("no date");
    w.dir.write(file, &undated.to_string());
    assert_eq!(w.ended(), Ended::Unknown, "it began before startedAt");
    w.runs(PID, NEWER);
    assert_eq!(w.ended(), Ended::Stopped, "it began long after startedAt");
    undated["startedAt"] = json!("2026-11-20T10:52:14.000Z");
    w.dir.write(file, &undated.to_string());
    assert_eq!(w.ended(), Ended::Unknown, "an ISO start is read too");
}

#[test]
fn a_process_of_ours_with_no_start_to_compare_counts_as_the_jobs() {
    let w = world("ended-no-facts");
    w.state("stopped");
    w.dir
        .write("claude/sessions/4242.json", &session(PID).to_string());
    w.runs(
        PID,
        Process::Ours {
            started_ms: None,
            ticks: None,
        },
    );
    assert_eq!(w.ended(), Ended::Unknown);
    let bare = json!({"pid": PID, "jobId": JOB});
    w.dir.write("claude/sessions/4242.json", &bare.to_string());
    w.runs(PID, NEWER);
    assert_eq!(w.ended(), Ended::Unknown, "a file with no start in it");
}

#[test]
fn a_session_file_that_cannot_be_read_holds_only_while_it_may_be_a_running_process_mid_write() {
    for state in ["stopped", "done"] {
        let ended = match state {
            "stopped" => Ended::Stopped,
            _ => Ended::Done,
        };
        let w = world("ended-torn-session");
        w.state(state);
        w.dir.write("claude/sessions/4242.json", "{\"pid\": 42");
        assert_eq!(w.ended(), ended, "{state}: its pid is gone");
        w.runs(PID, Process::Other);
        assert_eq!(w.ended(), ended, "{state}: its pid is another user's");
        w.runs(PID, ITS_OWN);
        assert_eq!(w.ended(), Ended::Unknown, "{state}: its pid runs");
        assert_eq!(w.later(MID_WRITE_MS - 1), Ended::Unknown, "{state}");
        assert_eq!(w.later(1), ended, "{state}: unchanged, left behind");
        w.dir.write("lane-b/sessions/odd-name.json", "{\"pid\": 12");
        assert_eq!(w.ended(), Ended::Unknown, "{state}: named by no pid");
        assert_eq!(w.later(MID_WRITE_MS), ended, "{state}");
        w.dir
            .write("claude/sessions/4242.json", &session(PID).to_string());
        let live = match state {
            "stopped" => Ended::Unknown,
            _ => Ended::No,
        };
        assert_eq!(w.ended(), live, "{state}: the write finished");
    }
}

#[test]
fn a_sessions_folder_that_cannot_be_listed_is_unknown() {
    let w = world("ended-unlisted");
    w.state("stopped");
    w.dir.write("claude/sessions", "a file where a folder goes");
    assert_eq!(w.ended(), Ended::Unknown);
    assert_eq!(w.later(86_400_000), Ended::Unknown);
}

#[test]
fn a_live_job_no_job_or_an_odd_name_has_not_ended() {
    let w = world("ended-none");
    assert_eq!(w.ended(), Ended::No);
    w.state("working");
    assert_eq!(w.ended(), Ended::No);
    let look = Look {
        process: &|_| Process::Gone,
        settled: &|_| true,
    };
    for odd in ["", "../be50d36a", ".hidden", "a/b"] {
        assert_eq!(job_ended(&w.roots, &look, odd), Ended::No, "{odd}");
    }
}

#[test]
fn the_start_ps_prints_is_read_as_utc() {
    assert_eq!(lstart_ms(BEGAN), Some(BEGAN_MS));
    assert_eq!(
        lstart_ms("Fri Nov  6 09:52:13 2026"),
        Some(BEGAN_MS - 14 * 86_400_000)
    );
    for odd in [
        "",
        "88123",
        "Fri Nov 20 2026",
        "Fri Nov 20 09:52:13 2026 UTC",
    ] {
        assert_eq!(lstart_ms(odd), None, "{odd}");
    }
}

#[test]
fn the_watch_counts_from_the_first_sight_of_a_file_as_it_is_and_forgets_what_it_no_longer_sees() {
    let mut watch = Watch::default();
    let mark = |len: u64| Mark {
        path: PathBuf::from("/x/state.json"),
        stamp: Some((len, None)),
    };
    assert!(!watch.settled(&mark(3), 100));
    assert!(!watch.settled(&mark(3), 100 + MID_WRITE_MS - 1));
    assert!(watch.settled(&mark(3), 100 + MID_WRITE_MS));
    assert!(!watch.settled(&mark(4), 100 + MID_WRITE_MS), "it changed");
    assert!(watch.settled(&mark(4), 100 + 2 * MID_WRITE_MS));
    let long_after = 100 + 2 * MID_WRITE_MS + FORGET_MS;
    assert!(!watch.settled(&mark(4), long_after), "forgotten");
    let gone = Mark {
        path: PathBuf::from("/x/state.json"),
        stamp: None,
    };
    assert!(!watch.settled(&gone, long_after + 1), "now missing");
    assert!(watch.settled(&gone, long_after + 1 + MID_WRITE_MS));
}

#[test]
fn over_every_mix_stopped_needs_a_stopped_record_and_a_stopped_job_always_gets_there() {
    let records = [
        Some("stopped"),
        Some("done"),
        Some("working"),
        Some(TORN),
        None,
    ];
    let lanes = [None, Some("stopped"), Some(TORN), Some("working")];
    let processes = [
        None,
        Some(Process::Gone),
        Some(Process::Other),
        Some(NEWER),
        Some(ITS_OWN),
    ];
    let torn_sessions = [None, Some(Process::Gone), Some(ITS_OWN)];
    let text = |record: &str| match record {
        TORN => TORN.to_string(),
        state => job(state),
    };
    for record in records {
        for lane in lanes {
            for process in processes {
                for torn in torn_sessions {
                    let w = world("ended-mix");
                    match record {
                        Some(record) => w.dir.write(STATE, &text(record)),
                        None => w.dir.write("claude/jobs/be50d36a/timeline.jsonl", ""),
                    };
                    if let Some(lane) = lane {
                        w.dir.write(LANE_STATE, &text(lane));
                    }
                    if let Some(process) = process {
                        w.dir
                            .write("claude/sessions/4242.json", &session(PID).to_string());
                        w.runs(PID, process);
                    }
                    if let Some(process) = torn {
                        w.dir.write("lane-b/sessions/777.json", "{\"pid\": 7");
                        w.runs(777, process);
                    }
                    let case = format!("{record:?} {lane:?} {process:?} {torn:?}");
                    let readable: Vec<&str> = [record, lane]
                        .into_iter()
                        .flatten()
                        .filter(|said| *said != TORN)
                        .collect();
                    let all_stopped =
                        !readable.is_empty() && readable.iter().all(|said| *said == "stopped");
                    let live = process == Some(ITS_OWN);
                    let first = w.ended();
                    let mut last = first;
                    for _ in 0..3 {
                        last = w.later(MID_WRITE_MS);
                    }
                    for seen in [first, last] {
                        if seen == Ended::Stopped {
                            assert!(all_stopped && !live, "{case}");
                        }
                        if seen == Ended::Done {
                            assert!(readable.contains(&"done") && !live, "{case}");
                        }
                        if live {
                            assert!(matches!(seen, Ended::No | Ended::Unknown), "{case}");
                        }
                    }
                    if all_stopped && !live {
                        assert_eq!(last, Ended::Stopped, "{case}: bounded");
                    }
                    if readable == ["done"] && !live {
                        assert_eq!(last, Ended::Done, "{case}: bounded");
                    }
                    if readable.is_empty() && !live {
                        assert_eq!(last, Ended::Unknown, "{case}: no record to read");
                    }
                }
            }
        }
    }
}
