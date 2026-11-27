use super::*;
use crate::skills_family::{KEY_RULE, REASON_RULE};

const BINARY: &str = "/Apps/N";

fn text() -> String {
    content(BINARY)
}

fn lower() -> String {
    text().to_lowercase()
}

#[test]
fn dispatch_has_its_dry_run_and_names_repo_branch_and_task() {
    let text = text();
    assert!(text
        .lines()
        .any(|l| l.contains(&format!("'{BINARY}' dispatch")) && l.contains("--dry-run")));
    assert!(text.contains("repo"));
    assert!(text.contains("branch"));
    assert!(text.contains("task"));
}

#[test]
fn the_skill_follows_a_job_to_its_end() {
    let text = text();
    for line in [
        "job <id> --wait",
        "job tail",
        "job return",
        "job stop",
        "jobs --json",
    ] {
        assert!(text.contains(&format!("'{BINARY}' {line}")), "{line}");
    }
    let lower = lower();
    assert!(lower.contains("exit 124"));
    assert!(lower.contains("exit 6"));
}

#[test]
fn the_skill_says_the_cluster_switch_and_what_is_free() {
    let text = text();
    assert!(text.contains("switch_off"));
    assert!(text.contains("error.ask"));
    assert!(text.contains("job you sent"));
    assert!(text.contains("by state only"));
}

#[test]
fn the_skill_carries_the_rules() {
    let text = text();
    assert!(text.contains(REASON_RULE));
    assert!(text.contains(KEY_RULE));
}
