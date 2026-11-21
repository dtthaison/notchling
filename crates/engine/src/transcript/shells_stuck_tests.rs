use super::*;

const SELF_LOOP: &str = "while pgrep -f cargo-build-worker > /dev/null; do sleep 5; done; echo done";
const QUOTED: &str = "while pgrep -f \"npm run dev\"; do sleep 10; done";
const BRACKET: &str = "while pgrep -f '[c]argo-build-worker'; do sleep 5; done";
const WRAPPED: &str = "bash -c 'until ! pgrep -fl vite; do sleep 2; done && echo ok'";
const NAME_ONLY: &str = "while pgrep vite >/dev/null; do sleep 5; done";
const EXACT: &str = "while pgrep -fx 'sleep 100'; do sleep 5; done";
const BUILD: &str = "cargo build --release 2>&1 | tail -20";
const PLAIN_SLEEP: &str = "sleep 600 && echo done";

#[test]
fn a_pgrep_f_pattern_in_its_own_command_matches_itself() {
    assert!(matches_itself(SELF_LOOP));
    assert!(matches_itself(QUOTED));
    assert!(matches_itself(WRAPPED));
    assert_eq!(
        looks_stuck(SELF_LOOP, Some(10)),
        Some(Stuck::Itself),
        "at any age"
    );
    assert_eq!(looks_stuck(SELF_LOOP, None), Some(Stuck::Itself));
}

#[test]
fn the_bracket_trick_the_exact_flag_and_a_name_only_pgrep_do_not_match_themselves() {
    assert!(!matches_itself(BRACKET));
    assert!(!matches_itself(EXACT));
    assert!(!matches_itself(NAME_ONLY));
    assert!(!matches_itself(BUILD));
}

#[test]
fn patterns_come_out_of_the_command() {
    assert_eq!(pgrep_patterns(SELF_LOOP), vec!["cargo-build-worker"]);
    assert_eq!(pgrep_patterns(QUOTED), vec!["npm run dev"]);
    assert_eq!(pgrep_patterns(BRACKET), vec!["[c]argo-build-worker"]);
    assert_eq!(pgrep_patterns(WRAPPED), vec!["vite"]);
    assert_eq!(
        pgrep_patterns("while pgrep -u me -f worker; do sleep 1; done"),
        vec!["worker"]
    );
    assert_eq!(
        pgrep_patterns("while pgrep -af worker; do sleep 1; done"),
        vec!["worker"]
    );
    assert!(pgrep_patterns(NAME_ONLY).is_empty());
}

#[test]
fn a_wait_loop_needs_a_loop_pgrep_and_sleep() {
    assert!(is_wait_loop(SELF_LOOP));
    assert!(is_wait_loop(NAME_ONLY));
    assert!(is_wait_loop(WRAPPED));
    assert!(!is_wait_loop(PLAIN_SLEEP));
    assert!(!is_wait_loop("pgrep -f vite; sleep 5"));
    assert!(!is_wait_loop("while true; do sleep 5; done"));
    assert!(!is_wait_loop("echo meanwhile pgrep sleep"));
}

#[test]
fn a_wait_loop_looks_stuck_only_after_thirty_minutes() {
    assert_eq!(looks_stuck(NAME_ONLY, Some(STUCK_AFTER_S)), None);
    assert_eq!(looks_stuck(NAME_ONLY, None), None);
    assert_eq!(
        looks_stuck(NAME_ONLY, Some(STUCK_AFTER_S + 1)),
        Some(Stuck::LongLoop)
    );
    assert_eq!(looks_stuck(BUILD, Some(99_999)), None);
    assert_eq!(looks_stuck(PLAIN_SLEEP, Some(99_999)), None);
}

#[test]
fn running_time_comes_from_the_start() {
    assert_eq!(running_secs(Some(1000.0), 1047.0), Some(47));
    assert_eq!(running_secs(Some(1000.0), 900.0), Some(0));
    assert_eq!(running_secs(None, 900.0), None);
    assert_eq!(running_secs(Some(0.0), 900.0), None);
}
