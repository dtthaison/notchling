//! Ported from `MachinesPollerTests.swift`: the poller's rounds, reports, `previous`, the
//! announcements and the refresh that is asked for while one runs. The Swift timer tests and the
//! `ProcessRunner` tests are not here: the caller owns the timer, and `process.rs` has its own tests.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use super::fakes_jobs::{bad_code, lock, machine, ok, AnswerRunner, HoldingRunner};
use super::poller::{waiting_text, MachinePoller};
use super::registry::{Machine, SdWorkerSettings};
use super::runner::Runner;
use super::shell;
use super::status::parse_status_date;

const NOW: f64 = 1_800_000_000.0;

type Machines = Arc<Mutex<Vec<Machine>>>;

fn status_json(name: &str, needs_you: &[(&str, i64)], booted: Option<&str>) -> String {
    let items: Vec<String> = needs_you
        .iter()
        .map(|(repo, n)| format!("{{\"repo\":\"{repo}\",\"issue\":{n},\"reason\":\"question\"}}"))
        .collect();
    let boot = booted
        .map(|b| format!(",\"booted_at\":\"{b}\""))
        .unwrap_or_default();
    format!(
        "{{\"machine\":\"{name}\",\"version\":\"1.19.1\",\"needs_you\":[{}]{boot}}}",
        items.join(",")
    )
}

fn poller_over(runner: Arc<dyn Runner>, machines: &Machines) -> MachinePoller {
    let list = machines.clone();
    MachinePoller::new(runner, move || lock(&list).clone(), || NOW)
}

fn poller(runner: &Arc<AnswerRunner>, machines: Vec<Machine>) -> (MachinePoller, Machines) {
    let list: Machines = Arc::new(Mutex::new(machines));
    (poller_over(runner.clone(), &list), list)
}

/// The same answer each time, changed by the test between rounds.
fn shared(initial: &str) -> (Arc<Mutex<String>>, Arc<AnswerRunner>) {
    let text = Arc::new(Mutex::new(initial.to_string()));
    let t = text.clone();
    (
        text,
        Arc::new(AnswerRunner::new(move |_| ok(&lock(&t).clone()))),
    )
}

// SSH (the strings the poller sends)

#[test]
fn ssh_argv_quotes_remote() {
    let m = machine("vps");
    let argv = shell::ssh_argv(
        &m,
        &[
            "python3",
            "/opt/my sd/sd.py",
            "it's",
            "worker",
            "status",
            "--json",
        ],
    );
    // `--` ends the options, so a host written as `-oProxyCommand=…` is a destination, never an option
    assert_eq!(
        argv[..7],
        [
            "/usr/bin/ssh",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=8",
            "--",
            "agent@vps.tail"
        ]
    );
    assert_eq!(argv.len(), 8);
    // one remote argument, in which the path with a space and the quote stay single words
    assert_eq!(
        argv[7],
        "'python3' '/opt/my sd/sd.py' 'it'\\''s' 'worker' 'status' '--json'"
    );
}

#[test]
fn git_url() {
    assert_eq!(
        shell::git_url(&machine("vps"), "/home/agent/repo.git").as_deref(),
        Some("ssh://agent@vps.tail/home/agent/repo.git")
    );
}

/// Without the leading slash the path would be read as part of the host (`ssh://u@h.evil.com/x`).
#[test]
fn git_url_refuses_a_path_that_is_not_absolute() {
    for path in [
        "",
        ".evil.com/x",
        "@other/x",
        "relative/dir",
        "-oProxyCommand=x",
        " /x",
    ] {
        assert_eq!(shell::git_url(&machine("vps"), path), None, "{path}");
    }
}

// refresh

#[test]
fn refresh_runs_status_command_on_every_machine() {
    let runner = Arc::new(AnswerRunner::quiet());
    let (p, _) = poller(&runner, vec![machine("a"), machine("b")]);
    p.refresh(false);
    let calls = runner.calls();
    assert_eq!(calls.len(), 2);
    for name in ["a", "b"] {
        let call = calls
            .iter()
            .find(|c| c.contains(&format!("agent@{name}.tail")))
            .expect("a call");
        let m = machine(name);
        assert_eq!(
            call,
            &shell::ssh_script(&m, "'python3' '/opt/sd dir/sd.py' worker status --json")
        );
    }
}

#[test]
fn polls_only_machines_with_an_sd_worker_job() {
    let runner = Arc::new(AnswerRunner::quiet());
    let idle = Machine::new("idle", "idle.tail", "agent");
    assert_eq!(idle.sd_path(), "", "a machine with no job has no sd path");
    let (p, _) = poller(&runner, vec![machine("a"), idle]);
    p.refresh(false);
    assert_eq!(runner.calls().len(), 1);
    assert!(!runner
        .calls()
        .iter()
        .any(|c| c.contains(&"agent@idle.tail".to_string())));
    assert!(p.report("a").is_some());
    assert!(p.report("idle").is_none());
}

#[test]
fn a_job_without_an_sd_path_is_not_polled_yet() {
    let runner = Arc::new(AnswerRunner::quiet());
    let mut early = Machine::new("early", "early.tail", "agent");
    early.set_sd_worker(Some(SdWorkerSettings::new("early")));
    let (p, _) = poller(&runner, vec![early]);
    p.refresh(false);
    assert!(
        runner.calls().is_empty(),
        "Plugins installed has not told the sd path yet"
    );
    assert!(p.report("early").is_none());
}

#[test]
fn a_machine_whose_job_was_removed_loses_its_report() {
    let runner = Arc::new(AnswerRunner::new(|_| ok(&status_json("a", &[], None))));
    let (p, list) = poller(&runner, vec![machine("a")]);
    p.refresh(false);
    assert!(p.report("a").is_some());
    lock(&list)[0].set_sd_worker(None);
    p.refresh(false);
    assert!(p.report("a").is_none());
    assert_eq!(runner.calls().len(), 1);
}

#[test]
fn status_command_expands_a_tilde_path() {
    let runner = Arc::new(AnswerRunner::quiet());
    let mut m = machine("a");
    if let Some(s) = m.sd_worker_mut() {
        s.sd_path = "~/sd/sd.py".into();
    }
    let (p, _) = poller(&runner, vec![m.clone()]);
    p.refresh(false);
    // the script is `'python3' "$HOME"/'sd/sd.py' worker status --json`, quoted once more for `sh -c`
    let call = runner.calls().remove(0);
    assert_eq!(
        call,
        shell::ssh_script(&m, "'python3' \"$HOME\"/'sd/sd.py' worker status --json")
    );
    assert!(call.last().is_some_and(|s| s.contains("\"$HOME\"/")));
}

/// A python or sd path the machine reported is quoted, never a command of its own.
#[test]
fn a_hostile_sd_path_stays_one_quoted_word() {
    let runner = Arc::new(AnswerRunner::quiet());
    let mut m = machine("a");
    if let Some(s) = m.sd_worker_mut() {
        s.sd_path = "/x/sd.py'; touch /tmp/pwned; echo '".into();
        s.python = "$(id)".into();
    }
    let (p, _) = poller(&runner, vec![m]);
    p.refresh(false);
    let call = runner.calls().remove(0);
    assert_eq!(call.len(), 8, "one remote argument");
    let inner = r"'$(id)' '/x/sd.py'\''; touch /tmp/pwned; echo '\''' worker status --json";
    assert_eq!(call, shell::ssh_script(&machine("a"), inner));
}

#[test]
fn remote_127_is_not_ssh_not_found() {
    let runner = Arc::new(AnswerRunner::new(|argv| {
        if argv.contains(&"agent@gone.tail".to_string()) {
            bad_code("/usr/bin/ssh not found", 127, "")
        } else {
            bad_code(
                "sh: 1: /opt/sd dir/sd.py: not found\nsecond line\n",
                127,
                "",
            )
        }
    }));
    let (p, _) = poller(&runner, vec![machine("gone"), machine("nosd")]);
    p.refresh(false);
    assert_eq!(
        p.report("gone").and_then(|r| r.error).as_deref(),
        Some("ssh not found")
    );
    assert_eq!(
        p.report("nosd").and_then(|r| r.error).as_deref(),
        Some("not found on the machine: sh: 1: /opt/sd dir/sd.py: not found")
    );
}

#[test]
fn refresh_stores_status_and_error() {
    let runner = Arc::new(AnswerRunner::new(|argv| {
        if argv.contains(&"agent@bad.tail".to_string()) {
            bad_code(
                "ssh: connect to host bad.tail: Operation timed out\n",
                255,
                "",
            )
        } else {
            ok(&status_json("good", &[], None))
        }
    }));
    let (p, _) = poller(&runner, vec![machine("good"), machine("bad")]);
    p.refresh(false);
    let good = p.report("good").expect("good");
    assert_eq!(
        good.status.as_ref().map(|s| s.machine.as_str()),
        Some("good")
    );
    assert_eq!(good.error, None);
    assert_eq!(good.at, Some(NOW));
    assert!(good.succeeded());
    let bad = p.report("bad").expect("bad");
    assert_eq!(bad.status, None);
    assert_eq!(
        bad.error.as_deref(),
        Some("ssh: connect to host bad.tail: Operation timed out")
    );
    assert_eq!(bad.at, Some(NOW));
}

#[test]
fn unreadable_output_is_an_error() {
    let runner = Arc::new(AnswerRunner::new(|_| ok("not json")));
    let (p, _) = poller(&runner, vec![machine("a")]);
    p.refresh(false);
    let r = p.report("a").expect("a report");
    assert_eq!(r.status, None);
    assert_eq!(r.error.as_deref(), Some("unreadable status"));
}

#[test]
fn timeout_and_missing_ssh_have_readable_errors() {
    let runner = Arc::new(AnswerRunner::new(|argv| {
        if argv.contains(&"agent@slow.tail".to_string()) {
            bad_code("", 124, "")
        } else {
            bad_code("/usr/bin/ssh not found", 127, "")
        }
    }));
    let (p, _) = poller(&runner, vec![machine("slow"), machine("gone")]);
    p.refresh(false);
    assert_eq!(
        p.report("slow").and_then(|r| r.error).as_deref(),
        Some("timed out")
    );
    assert_eq!(
        p.report("gone").and_then(|r| r.error).as_deref(),
        Some("ssh not found")
    );
}

#[test]
fn an_exit_code_with_no_text_is_named() {
    let runner = Arc::new(AnswerRunner::new(|_| bad_code("  \n", 255, "")));
    let (p, _) = poller(&runner, vec![machine("a")]);
    p.refresh(false);
    assert_eq!(
        p.report("a").and_then(|r| r.error).as_deref(),
        Some("ssh exited 255")
    );
}

#[test]
fn machine_removed_from_registry_is_dropped() {
    let runner = Arc::new(AnswerRunner::new(|_| ok(&status_json("x", &[], None))));
    let (p, list) = poller(&runner, vec![machine("a"), machine("b")]);
    p.refresh(false);
    *lock(&list) = vec![machine("a")];
    p.refresh(false);
    assert!(p.report("a").is_some());
    assert!(p.report("b").is_none());
    assert!(p.previous_of("b").is_none());
}

// previous

#[test]
fn previous_holds_the_report_before_the_last_one() {
    let (text, runner) = shared(&status_json("a", &[], Some("2026-11-05T01:00:00Z")));
    let (p, _) = poller(&runner, vec![machine("a")]);
    p.refresh(false);
    assert!(p.previous_of("a").is_none());
    *lock(&text) = status_json("a", &[], Some("2026-11-06T01:00:00Z"));
    p.refresh(false);
    let booted =
        |r: Option<super::poller::LastReport>| r.and_then(|r| r.status).and_then(|s| s.booted_at);
    assert_eq!(
        booted(p.previous_of("a")),
        parse_status_date("2026-11-05T01:00:00Z")
    );
    assert_eq!(
        booted(p.report("a")),
        parse_status_date("2026-11-06T01:00:00Z")
    );
}

#[test]
fn failed_refresh_keeps_the_old_previous_and_never_stores_an_error_there() {
    let fail = Arc::new(Mutex::new(false));
    let f = fail.clone();
    let runner = Arc::new(AnswerRunner::new(move |_| {
        if *lock(&f) {
            bad_code("down", 255, "")
        } else {
            ok(&status_json("a", &[("o/r", 1)], None))
        }
    }));
    let (p, _) = poller(&runner, vec![machine("a")]);
    p.refresh(false); // good
    *lock(&fail) = true;
    p.refresh(false); // first failure: the good report becomes previous
    assert_eq!(p.report("a").and_then(|r| r.error).as_deref(), Some("down"));
    let needs =
        |r: Option<super::poller::LastReport>| r.and_then(|r| r.status).map(|s| s.needs_you.len());
    assert_eq!(needs(p.previous_of("a")), Some(1));
    assert_eq!(p.previous_of("a").and_then(|r| r.error), None);
    p.refresh(false); // second failure: previous must stay the good report
    assert_eq!(needs(p.previous_of("a")), Some(1));
    assert_eq!(p.previous_of("a").and_then(|r| r.error), None);
    assert_eq!(p.report("a").and_then(|r| r.error).as_deref(), Some("down"));
}

#[test]
fn first_failure_leaves_no_previous() {
    let runner = Arc::new(AnswerRunner::new(|_| bad_code("down", 255, "")));
    let (p, _) = poller(&runner, vec![machine("a")]);
    p.refresh(false);
    p.refresh(false);
    assert!(p.previous_of("a").is_none());
}

// on_new_needs_you (the first good poll of a machine only takes stock: see "first poll")

type Fired = Arc<Mutex<Vec<String>>>;

fn watching(
    runner: &Arc<AnswerRunner>,
    machines: Vec<Machine>,
) -> (MachinePoller, Fired, Arc<Mutex<Vec<usize>>>) {
    let fired: Fired = Arc::default();
    let summaries: Arc<Mutex<Vec<usize>>> = Arc::default();
    let (f, s) = (fired.clone(), summaries.clone());
    let list: Machines = Arc::new(Mutex::new(machines));
    let l = list.clone();
    let p = MachinePoller::new(runner.clone(), move || lock(&l).clone(), || NOW)
        .on_new_needs_you(move |m, e| lock(&f).push(format!("{}/{}#{}", m.name, e.repo, e.issue)))
        .on_questions_waiting(move |n| lock(&s).push(n));
    (p, fired, summaries)
}

#[test]
fn new_needs_you_fires_once() {
    let (text, runner) = shared(&status_json("a", &[], None));
    let (p, fired, _) = watching(&runner, vec![machine("a")]);
    p.refresh(false);
    *lock(&text) = status_json("a", &[("o/r", 7)], None);
    p.refresh(false);
    p.refresh(false);
    assert_eq!(*lock(&fired), vec!["a/o/r#7"]);
}

#[test]
fn new_needs_you_fires_for_each_new_entry_and_per_machine() {
    let (text, runner) = shared(&status_json("x", &[], None));
    let (p, fired, _) = watching(&runner, vec![machine("a"), machine("b")]);
    p.refresh(false);
    *lock(&text) = status_json("x", &[("o/r", 7)], None);
    p.refresh(false);
    let set = |f: &Fired| lock(f).iter().cloned().collect::<BTreeSet<_>>();
    let seven: BTreeSet<String> = ["a/o/r#7", "b/o/r#7"].map(String::from).into();
    assert_eq!(set(&fired), seven);
    *lock(&text) = status_json("x", &[("o/r", 7), ("o/r", 8)], None);
    p.refresh(false);
    let both: BTreeSet<String> = ["a/o/r#7", "b/o/r#7", "a/o/r#8", "b/o/r#8"]
        .map(String::from)
        .into();
    assert_eq!(set(&fired), both);
}

#[test]
fn entry_that_leaves_and_comes_back_does_not_fire_again() {
    let (text, runner) = shared(&status_json("a", &[], None));
    let (p, fired, _) = watching(&runner, vec![machine("a")]);
    p.refresh(false);
    *lock(&text) = status_json("a", &[("o/r", 7)], None);
    p.refresh(false);
    *lock(&text) = status_json("a", &[], None);
    p.refresh(false);
    *lock(&text) = status_json("a", &[("o/r", 7)], None);
    p.refresh(false);
    assert_eq!(lock(&fired).len(), 1);
}

#[test]
fn failed_poll_fires_nothing() {
    let runner = Arc::new(AnswerRunner::new(|_| bad_code("down", 255, "")));
    let (p, fired, summaries) = watching(&runner, vec![machine("a")]);
    p.refresh(false);
    assert!(lock(&fired).is_empty());
    assert!(lock(&summaries).is_empty());
}

// first poll

/// Notchling relaunches on every install; the questions already waiting must not each ring.
#[test]
fn the_first_good_poll_of_a_machine_gives_one_summary_and_no_single_announcements() {
    let (text, runner) = shared(&status_json(
        "a",
        &[("o/r", 7), ("o/r", 8), ("o/r", 9)],
        None,
    ));
    let (p, fired, summaries) = watching(&runner, vec![machine("a")]);
    p.refresh(false);
    assert!(
        lock(&fired).is_empty(),
        "no question rang one by one on the first poll"
    );
    assert_eq!(*lock(&summaries), vec![3]);

    p.refresh(false);
    assert_eq!(
        *lock(&summaries),
        vec![3],
        "the same questions are not summarised again"
    );
    *lock(&text) = status_json(
        "a",
        &[("o/r", 7), ("o/r", 8), ("o/r", 9), ("o/r", 10)],
        None,
    );
    p.refresh(false);
    assert_eq!(
        *lock(&fired),
        vec!["a/o/r#10"],
        "a question that arrives later is announced on its own"
    );
    assert_eq!(*lock(&summaries), vec![3]);
}

#[test]
fn the_summary_counts_every_machine_once() {
    let runner = Arc::new(AnswerRunner::new(|argv| {
        if argv.iter().any(|a| a.contains("a.tail")) {
            ok(&status_json("a", &[("o/r", 1), ("o/r", 2)], None))
        } else {
            ok(&status_json("b", &[("o/r", 1)], None))
        }
    }));
    let (p, _, summaries) = watching(&runner, vec![machine("a"), machine("b")]);
    p.refresh(false);
    assert_eq!(
        *lock(&summaries),
        vec![3],
        "one notification for the whole first round"
    );
}

#[test]
fn no_questions_means_no_summary() {
    let runner = Arc::new(AnswerRunner::new(|_| ok(&status_json("a", &[], None))));
    let (p, _, summaries) = watching(&runner, vec![machine("a")]);
    p.refresh(false);
    p.refresh(false);
    assert!(lock(&summaries).is_empty());
}

/// A machine that was away on the first round gets its own first poll when it shows up.
#[test]
fn a_machine_that_answers_later_has_its_own_first_poll() {
    let up = Arc::new(Mutex::new(false));
    let u = up.clone();
    let runner = Arc::new(AnswerRunner::new(move |_| {
        if *lock(&u) {
            ok(&status_json("a", &[("o/r", 5), ("o/r", 6)], None))
        } else {
            bad_code("down", 255, "")
        }
    }));
    let (p, fired, summaries) = watching(&runner, vec![machine("a")]);
    p.refresh(false);
    *lock(&up) = true;
    p.refresh(false);
    assert!(lock(&fired).is_empty());
    assert_eq!(*lock(&summaries), vec![2]);
}

#[test]
fn the_summary_words() {
    assert_eq!(waiting_text(1), "1 question is waiting");
    assert_eq!(waiting_text(4), "4 questions are waiting");
}

// refresh while one runs

#[test]
fn a_poll_that_was_running_at_sleep_does_not_swallow_the_wake_refresh() {
    let runner = Arc::new(HoldingRunner::new(&status_json("a", &[], None)));
    let list: Machines = Arc::new(Mutex::new(vec![machine("a")]));
    let p = poller_over(runner.clone(), &list);
    std::thread::scope(|scope| {
        let in_flight = scope.spawn(|| p.refresh(false));
        runner.wait_for_first_call();

        p.refresh(false); // an ordinary tick while one runs is still a no-op
        assert_eq!(runner.calls(), 1);
        p.refresh(true); // the wake: it must be remembered, not dropped
        assert_eq!(
            runner.calls(),
            1,
            "it does not run beside the poll in flight"
        );

        runner.release();
        in_flight.join().expect("the poll ends");
    });
    assert_eq!(runner.calls(), 2, "the poll ran again once the first ended");
    assert!(p.report("a").is_some_and(|r| r.status.is_some()));
}

#[test]
fn a_queued_refresh_runs_only_once() {
    let runner = Arc::new(HoldingRunner::new(&status_json("a", &[], None)));
    let list: Machines = Arc::new(Mutex::new(vec![machine("a")]));
    let p = poller_over(runner.clone(), &list);
    std::thread::scope(|scope| {
        let in_flight = scope.spawn(|| p.refresh(false));
        runner.wait_for_first_call();
        p.refresh(true);
        p.refresh(true);
        runner.release();
        in_flight.join().expect("the poll ends");
    });
    assert_eq!(runner.calls(), 2);
}

#[test]
fn a_refresh_after_the_last_one_ended_runs_again() {
    let runner = Arc::new(AnswerRunner::quiet());
    let (p, _) = poller(&runner, vec![machine("a")]);
    p.refresh(false);
    p.refresh(false);
    assert_eq!(runner.calls().len(), 2);
}
