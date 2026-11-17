use crate::git_args::FRESH_READS;
use crate::git_cache::{
    pick_reads, Budget, GitCache, Listed, Numbers, IDLE_DROP_MS, LIST_FOR_MS, MISSING_READS,
    NUMBERS_CAP, READ_STOP_MS, STALE_MS,
};
use notchling_engine::git::branch_list::Work;
use notchling_engine::git::refs::{Counts, Lines};
use std::collections::{HashMap, HashSet};
use std::path::Path;

fn commit(n: usize) -> String {
    format!("{n:040x}")
}

fn counts(ahead: u64, behind: u64) -> Option<Counts> {
    Some(Counts { ahead, behind })
}

fn lines(added: u64) -> Option<Lines> {
    Some(Lines {
        added,
        removed: 1,
        files: 1,
    })
}

fn work_at(read_at: i64) -> Work {
    Work {
        read_at,
        ..Work::default()
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_string()).collect()
}

#[test]
fn numbers_keep_the_2000_most_recent() {
    let mut numbers = Numbers::default();
    let base = commit(0);
    for n in 1..=NUMBERS_CAP {
        numbers.put_counts(&base, &commit(n), counts(n as u64, 0));
    }
    assert_eq!(numbers.counts(&base, &commit(1)), Some(counts(1, 0)));
    numbers.put_counts(&base, &commit(NUMBERS_CAP + 1), counts(9, 9));
    assert_eq!(numbers.counts(&base, &commit(1)), Some(counts(1, 0)));
    assert_eq!(numbers.counts(&base, &commit(2)), None);
    assert_eq!(numbers.counts(&base, &commit(3)), Some(counts(3, 0)));
    assert_eq!(
        numbers.counts(&base, &commit(NUMBERS_CAP + 1)),
        Some(counts(9, 9))
    );
}

#[test]
fn numbers_never_hold_more_than_the_cap() {
    let mut numbers = Numbers::default();
    let base = commit(0);
    for n in 1..=NUMBERS_CAP * 2 {
        numbers.put_counts(&base, &commit(n), counts(1, 1));
    }
    let held = (1..=NUMBERS_CAP * 2)
        .filter(|n| numbers.counts(&base, &commit(*n)).is_some())
        .count();
    assert_eq!(held, NUMBERS_CAP);
}

#[test]
fn unrelated_pair_is_remembered() {
    let mut numbers = Numbers::default();
    let (base, head) = (commit(1), commit(2));
    assert_eq!(numbers.related(&base, &head), None);
    numbers.put_related(&base, &head, false);
    assert_eq!(numbers.related(&base, &head), Some(false));
    assert_eq!(numbers.counts(&base, &head), Some(None));
    assert_eq!(numbers.lines(&base, &head), Some(None));
    assert_eq!(numbers.related(&head, &base), None);
}

#[test]
fn related_true_is_remembered() {
    let mut numbers = Numbers::default();
    let (base, head) = (commit(1), commit(2));
    numbers.put_related(&base, &head, true);
    assert_eq!(numbers.related(&base, &head), Some(true));
    assert_eq!(numbers.counts(&base, &head), None);
}

#[test]
fn unrelated_mark_survives_put_counts() {
    let mut numbers = Numbers::default();
    let (base, head) = (commit(1), commit(2));
    numbers.put_related(&base, &head, false);
    numbers.put_counts(&base, &head, counts(1, 1));
    assert_eq!(numbers.counts(&base, &head), Some(None));
    assert_eq!(numbers.related(&base, &head), Some(false));
    numbers.put_related(&base, &head, true);
    assert_eq!(numbers.related(&base, &head), Some(false));
}

#[test]
fn lines_none_marks_unrelated() {
    let mut numbers = Numbers::default();
    let (base, head) = (commit(1), commit(2));
    numbers.put_counts(&base, &head, counts(1, 1));
    assert_eq!(numbers.counts(&base, &head), Some(counts(1, 1)));
    numbers.put_lines(&base, &head, None);
    assert_eq!(numbers.lines(&base, &head), Some(None));
    assert_eq!(numbers.counts(&base, &head), Some(None));
    assert_eq!(numbers.related(&base, &head), Some(false));
}

#[test]
fn lines_some_is_cached_and_means_related() {
    let mut numbers = Numbers::default();
    let (base, head) = (commit(1), commit(2));
    assert_eq!(numbers.lines(&base, &head), None);
    numbers.put_lines(&base, &head, lines(5));
    assert_eq!(numbers.lines(&base, &head), Some(lines(5)));
    assert_eq!(numbers.related(&base, &head), Some(true));
}

#[test]
fn counts_none_without_a_mark_is_cached_as_unknown_counts() {
    let mut numbers = Numbers::default();
    let (base, head) = (commit(1), commit(2));
    numbers.put_counts(&base, &head, None);
    assert_eq!(numbers.counts(&base, &head), Some(None));
    assert_eq!(numbers.related(&base, &head), None);
    numbers.put_counts(&base, &head, counts(2, 0));
    assert_eq!(numbers.counts(&base, &head), Some(counts(2, 0)));
}

fn listed() -> Listed {
    Listed::default()
}

#[test]
fn listed_reused_for_5s_then_stale() {
    let mut cache = GitCache::default();
    let repo = cache.repo(Path::new("/r/.git"), 1_000);
    repo.listed = Some((1_000, listed()));
    assert!(repo.listed_fresh(1_000).is_some());
    assert!(repo.listed_fresh(1_000 + LIST_FOR_MS - 1).is_some());
    assert!(repo.listed_fresh(1_000 + LIST_FOR_MS).is_none());
    repo.listed = None;
    assert!(repo.listed_fresh(1_000).is_none());
}

#[test]
fn idle_repo_dropped_after_10_minutes() {
    let mut cache = GitCache::default();
    let a = Path::new("/a/.git");
    let b = Path::new("/b/.git");
    cache.repo(a, 0).listed = Some((0, listed()));
    cache.repo(b, IDLE_DROP_MS - 1);
    assert!(cache.repo(a, IDLE_DROP_MS - 1).listed.is_some());
    cache.repo(a, IDLE_DROP_MS - 1).asked_ms = 0;
    cache.repo(b, IDLE_DROP_MS);
    assert!(cache.repo(a, IDLE_DROP_MS).listed.is_none());
}

#[test]
fn asking_a_repo_keeps_it() {
    let mut cache = GitCache::default();
    let a = Path::new("/a/.git");
    cache.repo(a, 0).listed = Some((0, listed()));
    cache.repo(a, IDLE_DROP_MS - 1);
    assert_eq!(
        cache.repo(a, IDLE_DROP_MS * 2 - 2).asked_ms,
        IDLE_DROP_MS * 2 - 2
    );
    assert!(cache.repo(a, IDLE_DROP_MS * 2 - 2).listed.is_some());
}

#[test]
fn missing_first_up_to_four() {
    let eligible = names(&["a", "b", "c", "d", "e", "f"]);
    let mut work = HashMap::new();
    work.insert("b".to_string(), work_at(0));
    let picked = pick_reads(&eligible, &work, &HashSet::new(), &[], 1_000_000);
    assert_eq!(picked, names(&["a", "c", "d", "e"]));
    assert_eq!(picked.len(), MISSING_READS);
}

#[test]
fn stalest_older_than_15s_when_none_missing() {
    let eligible = names(&["a", "b", "c"]);
    let now = 100_000;
    let mut work = HashMap::new();
    work.insert("a".to_string(), work_at(now - STALE_MS - 1));
    work.insert("b".to_string(), work_at(now - STALE_MS - 5_000));
    work.insert("c".to_string(), work_at(now - 1_000));
    let picked = pick_reads(&eligible, &work, &HashSet::new(), &[], now);
    assert_eq!(picked, names(&["b"]));
}

#[test]
fn nothing_when_all_fresh() {
    let eligible = names(&["a", "b"]);
    let now = 100_000;
    let mut work = HashMap::new();
    work.insert("a".to_string(), work_at(now - STALE_MS));
    work.insert("b".to_string(), work_at(now));
    assert!(pick_reads(&eligible, &work, &HashSet::new(), &[], now).is_empty());
}

#[test]
fn fresh_adds_up_to_two() {
    let eligible = names(&["a", "b", "c", "d"]);
    let now = 100_000;
    let work: HashMap<String, Work> = eligible
        .iter()
        .map(|name| (name.clone(), work_at(now)))
        .collect();
    let fresh = names(&["a", "b", "c"]);
    let picked = pick_reads(&eligible, &work, &HashSet::new(), &fresh, now);
    assert_eq!(picked, names(&["a", "b"]));
    assert_eq!(picked.len(), FRESH_READS);
}

#[test]
fn fresh_skips_ones_already_chosen() {
    let eligible = names(&["a", "b", "c"]);
    let work = HashMap::new();
    let fresh = names(&["a", "a", "z", "y"]);
    let picked = pick_reads(&eligible, &work, &HashSet::new(), &fresh, 1_000);
    assert_eq!(picked, names(&["a", "b", "c", "z", "y"]));
}

#[test]
fn reading_paths_skipped() {
    let eligible = names(&["a", "b", "c"]);
    let now = 100_000;
    let mut work = HashMap::new();
    work.insert("c".to_string(), work_at(0));
    let reading: HashSet<String> = ["a".to_string(), "c".to_string()].into();
    let picked = pick_reads(&eligible, &work, &reading, &names(&["a", "c", "q"]), now);
    assert_eq!(picked, names(&["b", "q"]));
}

#[test]
fn budget_stops_at_1500ms() {
    let budget = Budget::new(10_000, READ_STOP_MS);
    assert!(budget.may_start(10_000));
    assert!(budget.may_start(10_000 + READ_STOP_MS - 1));
    assert!(!budget.may_start(10_000 + READ_STOP_MS));
    assert!(!budget.may_start(20_000));
}

#[test]
fn constants_match_the_spec() {
    assert_eq!(NUMBERS_CAP, 2_000);
    assert_eq!(IDLE_DROP_MS, 600_000);
    assert_eq!(LIST_FOR_MS, 5_000);
    assert_eq!(STALE_MS, 15_000);
    assert_eq!(MISSING_READS, 4);
    assert_eq!(FRESH_READS, 2);
    assert_eq!(READ_STOP_MS, 1_500);
}
