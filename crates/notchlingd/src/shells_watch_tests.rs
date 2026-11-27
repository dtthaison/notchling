use super::*;
use crate::inbox_ops_tests::{rig as inbox_rig, Rig as InboxRig};
use notchling_engine::sessions::Roots;
use notchling_engine::transcript::model::BackgroundShell;
use notchling_engine::transcript::shells_progress::SNOOZE_S;
use notchling_engine::transcript::shells_stop::Outcome;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

const SESSION: &str = "5e5510aa-0000-4000-8000-000000000002";
const PATH: &str = "/tmp/nl-fake/b1.output";
const T0: u64 = 1_800_000_000;

struct Env {
    watch: Arc<ShellsWatch>,
    inbox: InboxRig,
    acting: Arc<Acting>,
    clock: Arc<AtomicU64>,
    live: Arc<Mutex<Vec<i32>>>,
    bytes: Arc<AtomicU64>,
    cpu_ms: Arc<AtomicU64>,
    ps_calls: Arc<AtomicUsize>,
    ps_fails: Arc<AtomicBool>,
    stop_fails: Arc<AtomicBool>,
    started: Arc<AtomicU64>,
    stopped: Arc<Mutex<Vec<(String, String)>>>,
    finished: Arc<Mutex<HashMap<String, String>>>,
}

fn env() -> Env {
    let inbox = inbox_rig();
    let acting = Arc::new(Acting::in_memory(true));
    let clock = Arc::new(AtomicU64::new(T0));
    let live = Arc::new(Mutex::new(vec![77]));
    let bytes = Arc::new(AtomicU64::new(10));
    let cpu_ms = Arc::new(AtomicU64::new(4_000));
    let ps_calls = Arc::new(AtomicUsize::new(0));
    let ps_fails = Arc::new(AtomicBool::new(false));
    let stop_fails = Arc::new(AtomicBool::new(false));
    let started = Arc::new(AtomicU64::new(T0 - 3_000));
    let stopped: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
    let finished: Arc<Mutex<HashMap<String, String>>> = Arc::default();
    let ops = Arc::new(
        ShellsOps::new(Roots::new(
            &PathBuf::from("/nonexistent"),
            &PathBuf::from("/nonexistent"),
        ))
        .with_seams(
            {
                let live = live.clone();
                Arc::new(move |paths| {
                    paths
                        .iter()
                        .map(|p| (p.clone(), live.lock().unwrap().clone()))
                        .collect()
                })
            },
            {
                let stopped = stopped.clone();
                let live = live.clone();
                let fails = stop_fails.clone();
                Arc::new(move |request, _| {
                    stopped
                        .lock()
                        .unwrap()
                        .push((request.session.to_string(), request.path.to_string()));
                    if fails.load(Ordering::SeqCst) {
                        return Ok(Outcome {
                            targets: 1,
                            killed: 0,
                            survivors: 0,
                            left: vec![77],
                        });
                    }
                    live.lock().unwrap().clear();
                    Ok(Outcome {
                        targets: 1,
                        killed: 1,
                        survivors: 0,
                        left: Vec::new(),
                    })
                })
            },
            {
                let clock = clock.clone();
                Arc::new(move || clock.load(Ordering::SeqCst) as f64)
            },
            None,
        )
        .with_session_owner(Arc::new(|_| Some(4242))),
    );
    let candidates: Candidates = Arc::new(|| {
        vec![Candidate {
            name: "Build \u{7} site".into(),
            transcript: format!("/home/x/.claude/projects/-work/{SESSION}.jsonl"),
        }]
    });
    let watch = ShellsWatch::new(ops, inbox.ops.clone(), acting.clone(), candidates).with_seams(
        {
            let finished = finished.clone();
            let started = started.clone();
            Arc::new(move |_| {
                Arc::new(Conversation {
                    shells: vec![BackgroundShell {
                        id: "b1".into(),
                        command: "cargo watch".into(),
                        title: String::new(),
                        output_path: PATH.into(),
                        description: String::new(),
                        started: Some(started.load(Ordering::SeqCst) as f64),
                    }],
                    finished_agents: finished.lock().unwrap().clone(),
                    ..Conversation::default()
                })
            })
        },
        {
            let (calls, fails, cpu) = (ps_calls.clone(), ps_fails.clone(), cpu_ms.clone());
            Arc::new(move || {
                calls.fetch_add(1, Ordering::SeqCst);
                if fails.load(Ordering::SeqCst) {
                    return None;
                }
                Some(vec![
                    CpuProc {
                        pid: 77,
                        ppid: 1,
                        cpu_ms: cpu.load(Ordering::SeqCst),
                    },
                    CpuProc {
                        pid: 900,
                        ppid: 1,
                        cpu_ms: 1_000_000,
                    },
                ])
            })
        },
        {
            let bytes = bytes.clone();
            Arc::new(move |_| Some(bytes.load(Ordering::SeqCst)))
        },
    );
    Env {
        watch: Arc::new(watch),
        inbox,
        acting,
        clock,
        live,
        bytes,
        cpu_ms,
        ps_calls,
        ps_fails,
        stop_fails,
        started,
        stopped,
        finished,
    }
}

impl Env {
    fn minutes(&self, n: u64) {
        self.every(60, n);
    }

    fn every(&self, seconds: u64, n: u64) {
        for _ in 0..n {
            self.clock.fetch_add(seconds, Ordering::SeqCst);
            self.watch.tick();
        }
    }

    fn open(&self) -> Vec<Item> {
        self.inbox
            .ops
            .own_items()
            .into_iter()
            .filter(|i| i.kind == Kind::ShellStuck && i.state == State::Open)
            .collect()
    }

    fn all(&self) -> Vec<Item> {
        self.inbox
            .ops
            .own_items()
            .into_iter()
            .filter(|i| i.kind == Kind::ShellStuck)
            .collect()
    }
}

#[test]
fn a_quiet_idle_shell_raises_one_item_after_the_window_and_only_one() {
    let e = env();
    e.watch.tick();
    e.minutes(8);
    assert!(e.all().is_empty());
    e.minutes(2);
    let open = e.open();
    assert_eq!(open.len(), 1);
    let item = &open[0];
    assert_eq!(item.title, "Shell Not Making Progress");
    assert_eq!(
        item.detail,
        "A shell in \u{201c}Build  site\u{201d} has made no progress for 10 min."
    );
    assert_eq!(item.source.session.as_deref(), Some(SESSION));
    assert!(item.id.starts_with(&format!("shell-stuck:{SESSION}:b1:")));
    let ids: Vec<ActionId> = item.actions.iter().map(|a| a.id).collect();
    assert_eq!(ids, vec![ActionId::Stop, ActionId::KeepWaiting]);
    assert!(item.expires_ms.is_none());
    e.minutes(5);
    assert_eq!(e.all().len(), 1);
}

#[test]
fn a_shell_that_writes_or_computes_raises_nothing() {
    let e = env();
    e.watch.tick();
    for step in 0..15 {
        e.bytes.fetch_add(1, Ordering::SeqCst);
        e.minutes(1);
        assert!(e.all().is_empty(), "step {step}");
    }
    let busy = env();
    busy.watch.tick();
    for _ in 0..15 {
        busy.cpu_ms.fetch_add(60_000, Ordering::SeqCst);
        busy.minutes(1);
    }
    assert!(busy.all().is_empty());
}

#[test]
fn a_flagged_item_closes_when_the_shell_ends_or_makes_progress() {
    let e = env();
    e.watch.tick();
    e.minutes(10);
    assert_eq!(e.open().len(), 1);
    e.bytes.fetch_add(1, Ordering::SeqCst);
    e.minutes(1);
    assert!(e.open().is_empty());
    assert_eq!(e.all()[0].state, State::Withdrawn);

    let ended = env();
    ended.watch.tick();
    ended.minutes(10);
    assert_eq!(ended.open().len(), 1);
    ended.live.lock().unwrap().clear();
    ended.minutes(1);
    assert!(ended.open().is_empty());
    assert_eq!(ended.all()[0].state, State::Withdrawn);

    let done = env();
    done.watch.tick();
    done.minutes(10);
    done.finished
        .lock()
        .unwrap()
        .insert("b1".into(), "completed".into());
    done.minutes(1);
    assert!(done.open().is_empty());
}

#[test]
fn keep_waiting_snoozes_for_thirty_minutes_then_a_fresh_item_comes() {
    let e = env();
    e.watch.tick();
    e.minutes(10);
    let first = e.open().remove(0);
    let out = e
        .inbox
        .ops
        .decide(&first.id, ActionId::KeepWaiting, "mac")
        .unwrap();
    assert!(out.answered);
    e.watch.drain();
    e.minutes(29);
    assert!(e.open().is_empty());
    assert!(e.stopped.lock().unwrap().is_empty());
    e.minutes(SNOOZE_S / 60 - 29 + 1);
    let again = e.open();
    assert_eq!(again.len(), 1);
    assert_ne!(again[0].id, first.id);
    assert_eq!(e.all().len(), 2);
}

#[test]
fn stop_ends_that_shell_through_the_same_stopper_and_closes_the_item() {
    let e = env();
    e.watch.tick();
    e.minutes(10);
    let item = e.open().remove(0);
    let out = e.inbox.ops.decide(&item.id, ActionId::Stop, "mac").unwrap();
    assert!(out.answered);
    assert_eq!(out.answer, Some(ActionId::Stop));
    e.watch.drain();
    assert_eq!(
        e.stopped.lock().unwrap().as_slice(),
        &[(SESSION.to_string(), PATH.to_string())]
    );
    e.minutes(12);
    assert!(e.open().is_empty());
    assert_eq!(e.all().len(), 1);
}

#[test]
fn stop_is_refused_while_acting_is_off_and_the_item_stays_open() {
    let e = env();
    e.watch.tick();
    e.minutes(10);
    e.acting.set(false);
    let item = e.open().remove(0);
    let out = e.inbox.ops.decide(&item.id, ActionId::Stop, "mac").unwrap();
    assert!(!out.answered);
    e.watch.drain();
    assert!(e.stopped.lock().unwrap().is_empty());
    assert_eq!(e.open().len(), 1);
    let kept = e
        .inbox
        .ops
        .decide(&item.id, ActionId::KeepWaiting, "mac")
        .unwrap();
    assert!(kept.answered);
}

#[test]
fn a_stop_that_cannot_end_the_shell_does_not_raise_again_at_once() {
    let e = env();
    e.stop_fails.store(true, Ordering::SeqCst);
    e.watch.tick();
    e.minutes(10);
    let item = e.open().remove(0);
    e.inbox.ops.decide(&item.id, ActionId::Stop, "mac").unwrap();
    e.watch.drain();
    e.minutes(5);
    assert_eq!(e.stopped.lock().unwrap().len(), 1);
    assert!(e.open().is_empty());
    assert_eq!(e.all().len(), 1);
}

#[test]
fn one_ps_per_tick_and_none_when_nothing_runs() {
    let e = env();
    e.minutes(3);
    assert_eq!(e.ps_calls.load(Ordering::SeqCst), 3);
    e.live.lock().unwrap().clear();
    e.minutes(3);
    assert_eq!(e.ps_calls.load(Ordering::SeqCst), 3);
}

#[test]
fn no_process_table_means_no_flag() {
    let e = env();
    e.ps_fails.store(true, Ordering::SeqCst);
    e.minutes(20);
    assert!(e.all().is_empty());
}

#[test]
fn a_young_shell_is_never_flagged() {
    let e = env();
    e.started.store(T0 - 100, Ordering::SeqCst);
    e.watch.tick();
    e.minutes(11);
    assert!(e.all().is_empty());
}

#[test]
fn nine_minutes_of_quiet_is_not_ten() {
    let e = env();
    e.watch.tick();
    e.minutes(9);
    assert!(e.all().is_empty());
    e.minutes(1);
    assert_eq!(e.open().len(), 1);

    let finer = env();
    finer.watch.tick();
    finer.every(30, 17);
    assert!(finer.all().is_empty(), "510 s");
    finer.every(30, 2);
    assert!(finer.all().is_empty(), "570 s");
    finer.every(30, 1);
    assert_eq!(finer.open().len(), 1, "600 s");
    assert!(finer.open()[0].detail.ends_with("no progress for 10 min."));
}

#[test]
fn a_stop_answered_before_a_tick_that_no_longer_flags_the_shell_still_stops_it() {
    let e = env();
    e.watch.tick();
    e.minutes(10);
    let item = e.open().remove(0);
    let out = e.inbox.ops.decide(&item.id, ActionId::Stop, "mac").unwrap();
    assert!(out.answered);
    e.bytes.fetch_add(1, Ordering::SeqCst);
    e.minutes(1);
    e.watch.drain();
    assert_eq!(
        e.stopped.lock().unwrap().as_slice(),
        &[(SESSION.to_string(), PATH.to_string())]
    );
    assert_eq!(e.all()[0].state, State::Answered);
    assert_eq!(e.all()[0].answer, Some(ActionId::Stop));
}

#[test]
fn keep_waiting_answered_before_a_tick_that_no_longer_flags_the_shell_still_snoozes_it() {
    let e = env();
    e.watch.tick();
    e.minutes(10);
    let item = e.open().remove(0);
    let out = e
        .inbox
        .ops
        .decide(&item.id, ActionId::KeepWaiting, "mac")
        .unwrap();
    assert!(out.answered);
    e.bytes.fetch_add(1, Ordering::SeqCst);
    e.minutes(1);
    e.watch.drain();
    e.minutes(15);
    assert!(e.open().is_empty());
    assert_eq!(e.all().len(), 1);
}

#[test]
fn items_left_open_by_an_earlier_run_are_withdrawn_and_never_duplicated() {
    let e = env();
    e.watch.tick();
    e.minutes(10);
    let old = e.open().remove(0);
    e.inbox.ops.flush();
    let later = e.inbox.clock.load(Ordering::SeqCst) + 5_000;
    let reopened = InboxOps::with_clock(
        e.inbox.dir.clone(),
        Arc::new(crate::acting_ops_tests::FakeRoom::default()),
        Arc::new(move || later),
    );
    reopened.load();
    let ours = |ops: &InboxOps| -> Vec<Item> {
        ops.own_items()
            .into_iter()
            .filter(|i| i.kind == Kind::ShellStuck)
            .collect()
    };
    assert_eq!(ours(&reopened).len(), 1);
    assert_eq!(ours(&reopened)[0].state, State::Withdrawn);
    let again = ShellsWatch::new(
        e.watch.ops.clone(),
        reopened.clone(),
        e.acting.clone(),
        e.watch.candidates.clone(),
    )
    .with_seams(
        e.watch.read.clone(),
        e.watch.cpu.clone(),
        e.watch.size.clone(),
    );
    e.clock.fetch_add(60, Ordering::SeqCst);
    again.tick();
    let rows = ours(&reopened);
    let open: Vec<&Item> = rows.iter().filter(|i| i.state == State::Open).collect();
    assert_eq!(open.len(), 1);
    assert_ne!(open[0].id, old.id);
    again.tick();
    assert_eq!(ours(&reopened).len(), 2);
    reopened.close();
}
