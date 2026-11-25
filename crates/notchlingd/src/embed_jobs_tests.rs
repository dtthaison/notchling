use crate::embed_fetch::{progress_json, REMOVING, STOPPING};
use crate::embed_jobs::{After, Jobs, Settle};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[test]
fn a_remover_keeps_a_tombstone_until_every_remover_leaves() {
    let mut jobs = Jobs::new();
    let cancel = jobs.admit("m", 100).unwrap().unwrap();
    assert!(!cancel.load(Ordering::SeqCst));

    jobs.register_remover("m");
    assert!(cancel.load(Ordering::SeqCst));
    assert!(matches!(
        jobs.settle("m", 100),
        Settle::Done(After::Nothing)
    ));
    jobs.close("m");
    assert!(!jobs.is_empty());
    assert!(!jobs.waiting("m"));
    assert_eq!(jobs.admit("m", 100).unwrap_err(), REMOVING);

    jobs.register_remover("m");
    assert_eq!(jobs.admit("m", 100).unwrap_err(), REMOVING);

    assert!(!jobs.leave("m"));
    assert!(!jobs.is_empty());
    assert_eq!(jobs.admit("m", 100).unwrap_err(), REMOVING);

    assert!(jobs.leave("m"));
    assert!(jobs.is_empty());
    assert!(jobs.admit("m", 100).unwrap().is_some());
}

#[test]
fn waiting_is_true_while_running_and_finishing_and_false_when_gone_or_absent() {
    let mut jobs = Jobs::new();
    assert!(!jobs.waiting("m"));

    jobs.admit("m", 100).unwrap();
    assert!(jobs.waiting("m"));

    jobs.register_remover("m");
    assert!(jobs.waiting("m"));

    assert!(matches!(
        jobs.settle("m", 100),
        Settle::Done(After::Nothing)
    ));
    assert!(jobs.waiting("m"));

    jobs.close("m");
    assert!(!jobs.waiting("m"));
    assert!(!jobs.is_empty());

    assert!(jobs.leave("m"));
    assert!(!jobs.waiting("m"));
    assert!(jobs.is_empty());
}

#[test]
fn start_during_finishing_returns_stopping_while_cancelled_running_restarts() {
    let mut jobs = Jobs::new();
    jobs.admit("m", 100).unwrap();
    assert!(matches!(jobs.settle("m", 100), Settle::Done(After::Report)));
    assert!(!jobs.is_empty());
    assert_eq!(jobs.admit("m", 100).unwrap_err(), STOPPING);
    jobs.close("m");
    assert!(jobs.is_empty());

    let first = jobs.admit("m", 100).unwrap().unwrap();
    assert!(jobs.cancel("m"));
    assert!(first.load(Ordering::SeqCst));
    assert!(jobs.admit("m", 100).unwrap().is_none());
    let second = match jobs.settle("m", 100) {
        Settle::Again(cancel) => cancel,
        Settle::Done(after) => panic!("expected restart, got {after:?}"),
    };
    assert!(!second.load(Ordering::SeqCst));
    assert!(!Arc::ptr_eq(&first, &second));
    assert!(jobs.waiting("m"));
    let listed = jobs.list(&HashMap::new());
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["state"], "downloading");
}

#[test]
fn timed_out_remover_restores_report_only_when_removers_reaches_zero() {
    let mut jobs = Jobs::new();
    jobs.admit("m1", 100).unwrap();
    jobs.register_remover("m1");
    assert!(!jobs.leave("m1"));
    assert!(matches!(
        jobs.settle("m1", 100),
        Settle::Done(After::Report)
    ));
    jobs.close("m1");

    jobs.admit("m2", 100).unwrap();
    jobs.register_remover("m2");
    jobs.register_remover("m2");
    assert!(!jobs.leave("m2"));
    assert!(matches!(
        jobs.settle("m2", 100),
        Settle::Done(After::Nothing)
    ));
}

#[test]
fn cancel_and_list_ignore_finishing_and_gone_entries() {
    let mut jobs = Jobs::new();
    jobs.admit("fin", 100).unwrap();
    jobs.progress("fin", 40);
    assert!(matches!(
        jobs.settle("fin", 100),
        Settle::Done(After::Report)
    ));

    jobs.admit("gone", 100).unwrap();
    jobs.register_remover("gone");
    assert!(matches!(
        jobs.settle("gone", 100),
        Settle::Done(After::Nothing)
    ));
    jobs.close("gone");

    let run_cancel = jobs.admit("run", 100).unwrap().unwrap();
    jobs.progress("run", 25);
    jobs.progress("fin", 90);
    jobs.progress("gone", 90);

    assert!(!jobs.cancel("fin"));
    assert!(!jobs.cancel("gone"));
    jobs.cancel_all();
    assert!(run_cancel.load(Ordering::SeqCst));

    let mut results: HashMap<String, Value> = HashMap::new();
    results.insert(
        "fin".to_string(),
        progress_json("fin", "ready", 100, 100, None),
    );
    let rows = jobs.list(&results);
    assert_eq!(
        rows,
        vec![
            progress_json("fin", "ready", 100, 100, None),
            progress_json("run", "downloading", 25, 100, None),
        ]
    );
}
