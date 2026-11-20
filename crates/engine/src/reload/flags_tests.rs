use super::{bypasses, mode_of};

fn flags(list: &[&str]) -> Vec<String> {
    list.iter().map(|f| f.to_string()).collect()
}

#[test]
fn the_last_permission_mode_wins() {
    assert_eq!(
        mode_of(&flags(&["--model", "opus", "--permission-mode", "plan"])),
        Some("plan".to_string())
    );
    assert_eq!(
        mode_of(&flags(&[
            "--permission-mode",
            "plan",
            "--permission-mode=auto"
        ])),
        Some("auto".to_string())
    );
}

#[test]
fn no_mode_is_none() {
    assert_eq!(mode_of(&flags(&["--model", "opus"])), None);
    assert_eq!(mode_of(&flags(&["--permission-mode"])), None);
}

#[test]
fn skipping_permissions_is_seen_and_keeps_the_mode_unread() {
    let flags = flags(&["--model", "opus", "--dangerously-skip-permissions"]);
    assert!(bypasses(&flags));
    assert_eq!(mode_of(&flags), None);
    assert!(!bypasses(&self::flags(&["--permission-mode", "plan"])));
}
