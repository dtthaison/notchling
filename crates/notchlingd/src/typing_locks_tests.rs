use super::*;

#[test]
fn same_job_gets_same_lock() {
    let locks = TypingLocks::new();
    let first = locks.for_job("abcd1234");
    let second = locks.for_job("abcd1234");
    assert!(Arc::ptr_eq(&first, &second));
    assert!(!Arc::ptr_eq(&first, &locks.for_job("idle0001")));
}

#[test]
fn different_jobs_lock_independently() {
    let locks = TypingLocks::new();
    let a = locks.for_job("a");
    let b = locks.for_job("b");
    let _held = a.lock().unwrap();
    assert!(b.try_lock().is_ok());
    assert!(locks.for_job("a").try_lock().is_err());
}

#[test]
fn unused_locks_are_dropped() {
    let locks = TypingLocks::new();
    let kept = locks.for_job("kept");
    drop(locks.for_job("gone"));
    drop(locks.for_job("gone too"));
    assert_eq!(locks.len(), 2);
    let _ = locks.for_job("kept");
    assert_eq!(locks.len(), 1);
    drop(kept);
    let _ = locks.for_job("other");
    assert_eq!(locks.len(), 1);
}
