use super::renew::{renew_command, targets, ClaudeRenewer, LoginRenewer, Seat, RENEW_TIMEOUT};
use crate::control::{Call, FakeLauncher, LaunchError, LaunchOutput};
use crate::sessions::testutil::TempDir;
use crate::sessions::Roots;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

fn seat(account: &str, dir: Option<&str>) -> Seat {
    Seat {
        account: account.into(),
        config_dir: dir.map(String::from),
    }
}

fn set(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn only_expired_accounts_in_an_idle_place_are_renewed() {
    let seats = [
        seat("main", None),
        seat("a", Some("/lanes/a")),
        seat("b", Some("/lanes/b")),
        seat("c", Some("/lanes/c")),
    ];
    let picked = targets(&seats, &set(&["main", "a", "b"]), |dir| {
        dir == Some("/lanes/b")
    });
    assert_eq!(
        picked,
        vec![seat("main", None), seat("a", Some("/lanes/a"))]
    );
}

#[test]
fn a_busy_main_login_is_left_to_the_session_using_it() {
    let seats = [seat("main", None), seat("a", Some("/lanes/a"))];
    let picked = targets(&seats, &set(&["main", "a"]), |dir| dir.is_none());
    assert_eq!(picked, vec![seat("a", Some("/lanes/a"))]);
}

#[test]
fn nothing_expired_renews_nothing_and_an_account_is_never_listed_twice() {
    let seats = [seat("a", Some("/lanes/a")), seat("a", Some("/lanes/a"))];
    assert!(targets(&seats, &set(&[]), |_| false).is_empty());
    assert_eq!(targets(&seats, &set(&["a"]), |_| false).len(), 1);
}

#[test]
fn the_command_is_the_no_prompt_status_check_in_the_lane() {
    let lane = renew_command(Some("/lanes/a"));
    assert_eq!(lane.argv, vec!["claude", "auth", "status"]);
    assert_eq!(
        lane.env.get("CLAUDE_CONFIG_DIR"),
        Some(&Some("/lanes/a".to_string()))
    );
    let main = renew_command(None);
    assert_eq!(main.env.get("CLAUDE_CONFIG_DIR"), Some(&None));
}

fn renewer(root: &TempDir, launcher: Arc<FakeLauncher>) -> ClaudeRenewer {
    let roots = Roots {
        main_dir: root.path("main"),
        lanes_file: root.path("lanes.json"),
    };
    ClaudeRenewer::new(launcher, roots).with_alive(Arc::new(|pid| pid == 4242))
}

#[test]
fn renewing_runs_the_command_once_with_the_lane_and_a_30_second_limit() {
    let root = TempDir::new("renew-run");
    let launcher = Arc::new(FakeLauncher::new());
    let r = renewer(&root, launcher.clone());
    assert_eq!(r.renew(Some("/lanes/a")), Ok(()));
    assert_eq!(
        launcher.calls(),
        vec![Call::Run(renew_command(Some("/lanes/a")), RENEW_TIMEOUT)]
    );
    assert_eq!(RENEW_TIMEOUT, Duration::from_secs(30));
}

#[test]
fn a_command_that_fails_or_times_out_is_an_error() {
    let root = TempDir::new("renew-fail");
    let launcher = Arc::new(FakeLauncher::new());
    launcher.push_output(Ok(LaunchOutput::failed(1, "")));
    launcher.push_output(Err(LaunchError::Timeout(30)));
    let r = renewer(&root, launcher);
    assert!(r.renew(Some("/lanes/a")).is_err());
    assert!(r.renew(Some("/lanes/a")).is_err());
}

#[test]
fn a_live_session_file_makes_a_place_busy_and_a_dead_one_does_not() {
    let root = TempDir::new("renew-busy");
    let r = renewer(&root, Arc::new(FakeLauncher::new()));
    let lane = root.path("lane");
    let lane = lane.to_str().expect("utf8");
    assert!(!r.busy(Some(lane)));
    root.write("lane/sessions/9.json", r#"{"pid": 9}"#);
    assert!(!r.busy(Some(lane)), "pid 9 is not alive");
    root.write("lane/sessions/4242.json", r#"{"pid": 4242}"#);
    assert!(r.busy(Some(lane)));
    assert!(!r.busy(None));
    root.write("main/sessions/4242.json", r#"{"pid": 4242}"#);
    assert!(r.busy(None));
}
