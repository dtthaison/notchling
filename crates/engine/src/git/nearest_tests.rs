use super::nearest::{candidates, pick, short_name, Candidate};
use super::refs::Counts;

const SHA: &str = "0823e49772e044ef8d683209cb2c931dec786b96";

fn cand(rev: &str) -> Candidate {
    Candidate {
        rev: rev.to_string(),
        commit: SHA.to_string(),
    }
}

fn counts(ahead: u64, behind: u64) -> Option<Counts> {
    Some(Counts { ahead, behind })
}

fn remotes() -> Vec<String> {
    vec!["origin".to_string()]
}

fn revs(list: &[Candidate]) -> Vec<&str> {
    list.iter().map(|c| c.rev.as_str()).collect()
}

#[test]
fn fewest_ahead_wins() {
    let scored = vec![
        (cand("origin/main"), counts(40, 0)),
        (cand("int"), counts(12, 900)),
    ];
    let (won, got) = pick(&scored).unwrap();
    assert_eq!(won.rev, "int");
    assert_eq!(
        got,
        Counts {
            ahead: 12,
            behind: 900
        }
    );
}

#[test]
fn then_fewest_behind() {
    let scored = vec![
        (cand("origin/main"), counts(3, 50)),
        (cand("int"), counts(3, 7)),
    ];
    assert_eq!(pick(&scored).unwrap().0.rev, "int");
}

#[test]
fn then_candidate_order() {
    let scored = vec![
        (cand("origin/main"), counts(3, 7)),
        (cand("int"), counts(3, 7)),
        (cand("origin/lane"), counts(3, 7)),
    ];
    assert_eq!(pick(&scored).unwrap().0.rev, "origin/main");
}

#[test]
fn branch_made_at_main_gets_main() {
    let scored = vec![
        (cand("origin/main"), counts(0, 0)),
        (cand("agent-run-oss"), counts(0, 1482)),
    ];
    let (won, got) = pick(&scored).unwrap();
    assert_eq!(won.rev, "origin/main");
    assert_eq!(
        got,
        Counts {
            ahead: 0,
            behind: 0
        }
    );
}

#[test]
fn never_its_own_base() {
    let list = candidates(
        Some("agent-run-oss"),
        None,
        Some(&cand("origin/main")),
        Some(&cand("agent-run-oss")),
        &remotes(),
    );
    assert_eq!(revs(&list), ["origin/main"]);
}

#[test]
fn upstream_with_own_name_is_no_candidate() {
    let list = candidates(
        Some("agent/ask"),
        Some(&cand("origin/agent/ask")),
        Some(&cand("origin/main")),
        Some(&cand("agent-run-oss")),
        &remotes(),
    );
    assert_eq!(revs(&list), ["origin/main", "agent-run-oss"]);
    let list = candidates(
        Some("agent/ask"),
        Some(&cand("origin/agent-run-oss")),
        Some(&cand("origin/main")),
        None,
        &remotes(),
    );
    assert_eq!(revs(&list), ["origin/main", "origin/agent-run-oss"]);
}

#[test]
fn candidate_order_is_default_integration_upstream() {
    let list = candidates(
        Some("lane"),
        Some(&cand("origin/other")),
        Some(&cand("origin/main")),
        Some(&cand("int")),
        &remotes(),
    );
    assert_eq!(revs(&list), ["origin/main", "int", "origin/other"]);
}

#[test]
fn default_compared_by_short_name() {
    let list = candidates(
        Some("main"),
        None,
        Some(&cand("origin/main")),
        Some(&cand("int")),
        &remotes(),
    );
    assert_eq!(revs(&list), ["int"]);
    assert_eq!(short_name("origin/main", &remotes()), "main");
    assert_eq!(short_name("origin/agent/ask", &remotes()), "agent/ask");
    assert_eq!(short_name("main", &remotes()), "main");
    assert_eq!(short_name("upstream/main", &remotes()), "upstream/main");
    let nested = vec!["origin".to_string(), "origin/fork".to_string()];
    assert_eq!(short_name("origin/fork/x", &nested), "x");
}

#[test]
fn detached_head_has_no_own_name() {
    let list = candidates(
        None,
        None,
        Some(&cand("origin/main")),
        Some(&cand("int")),
        &remotes(),
    );
    assert_eq!(revs(&list), ["origin/main", "int"]);
}

#[test]
fn upstream_equal_to_another_candidate_is_listed_once() {
    let list = candidates(
        Some("lane"),
        Some(&cand("origin/main")),
        Some(&cand("origin/main")),
        None,
        &remotes(),
    );
    assert_eq!(revs(&list), ["origin/main"]);
}

#[test]
fn unrelated_candidates_never_win() {
    let scored = vec![(cand("origin/main"), None), (cand("int"), counts(500, 500))];
    assert_eq!(pick(&scored).unwrap().0.rev, "int");
    let none = vec![(cand("origin/main"), None), (cand("int"), None)];
    assert!(pick(&none).is_none());
}

#[test]
fn no_candidates_no_base() {
    assert!(pick(&[]).is_none());
    let list = candidates(Some("x"), None, None, None, &remotes());
    assert!(list.is_empty());
}
