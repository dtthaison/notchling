use super::*;

#[cfg(unix)]
#[test]
fn boot_ms_moves_forward() {
    let first = boot_ms().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let second = boot_ms().unwrap();
    assert!(second >= first, "{second} < {first}");
    assert!(first > 0);
}

#[cfg(unix)]
#[test]
fn boot_id_is_stable_within_a_run() {
    let a = boot_id().unwrap();
    let b = boot_id().unwrap();
    assert_eq!(a, b);
    assert!(!a.is_empty());
}

#[cfg(windows)]
#[test]
fn windows_says_unsupported() {
    assert!(matches!(boot_ms(), Err(crate::Error::Unsupported(_))));
    assert!(matches!(boot_id(), Err(crate::Error::Unsupported(_))));
}
