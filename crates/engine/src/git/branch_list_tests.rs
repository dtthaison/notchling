use super::branch_list::{no_worktree, rows, Bases, BranchRow, ListInput, Work, WorktreeOf};
use super::nearest::Candidate;
use super::refs::{Counts, Lines, RefRow, Track};
use super::worktrees::WorktreeEntry;
use std::cell::RefCell;
use std::collections::HashMap;

fn sha(c: char) -> String {
    std::iter::repeat_n(c, 40).collect()
}

fn cand(rev: &str, c: char) -> Candidate {
    Candidate {
        rev: rev.to_string(),
        commit: sha(c),
    }
}

fn counts(ahead: u64, behind: u64) -> Option<Option<Counts>> {
    Some(Some(Counts { ahead, behind }))
}

fn unrelated() -> Option<Option<Counts>> {
    Some(None)
}

fn lines(added: u64, removed: u64, files: u64) -> Option<Option<Lines>> {
    Some(Some(Lines {
        added,
        removed,
        files,
    }))
}

fn rr(refname: &str, c: char, upstream: &str) -> RefRow {
    RefRow {
        refname: refname.to_string(),
        commit: sha(c),
        committed_ms: Some(1_700_000_000_000),
        upstream: (!upstream.is_empty()).then(|| upstream.to_string()),
        track: Track::default(),
        subject: "a subject".to_string(),
    }
}

fn entry(path: &str, branch: Option<&str>, head: char) -> WorktreeEntry {
    WorktreeEntry {
        path: path.to_string(),
        head: Some(sha(head)),
        branch: branch.map(str::to_string),
        detached: branch.is_none(),
        ..WorktreeEntry::default()
    }
}

fn place(path: &str, main: bool, work: Option<Work>) -> WorktreeOf {
    WorktreeOf {
        path: path.to_string(),
        real: path.to_string(),
        main,
        current: false,
        missing: false,
        work,
    }
}

struct World {
    refs: Vec<RefRow>,
    worktrees: Vec<(WorktreeEntry, WorktreeOf)>,
    remotes: Vec<(String, bool)>,
    bases: Bases,
    facts: HashMap<String, (Option<i64>, String)>,
    counts: HashMap<(String, String), Option<Counts>>,
    lines: HashMap<(String, String), Option<Lines>>,
    lines_asked: RefCell<Vec<(String, String)>>,
}

impl World {
    fn new() -> World {
        World {
            refs: vec![],
            worktrees: vec![],
            remotes: vec![("origin".to_string(), true)],
            bases: Bases {
                default: Some(cand("origin/main", 'a')),
                integration: Some(cand("int", 'b')),
            },
            facts: HashMap::new(),
            counts: HashMap::new(),
            lines: HashMap::new(),
            lines_asked: RefCell::new(vec![]),
        }
    }

    fn count(&mut self, rev: &str, head: char, value: Option<Option<Counts>>) {
        if let Some(value) = value {
            self.counts.insert((rev.to_string(), sha(head)), value);
        }
    }

    fn line(&mut self, commit: char, head: char, value: Option<Option<Lines>>) {
        if let Some(value) = value {
            self.lines.insert((sha(commit), sha(head)), value);
        }
    }

    fn run(&self) -> Vec<BranchRow> {
        let input = ListInput {
            refs: &self.refs,
            worktrees: &self.worktrees,
            remotes: &self.remotes,
            bases: &self.bases,
            detached_facts: &self.facts,
        };
        rows(
            &input,
            &|c, head| self.counts.get(&(c.rev.clone(), head.to_string())).copied(),
            &|c, head| {
                self.lines_asked
                    .borrow_mut()
                    .push((c.commit.clone(), head.to_string()));
                self.lines
                    .get(&(c.commit.clone(), head.to_string()))
                    .copied()
            },
        )
    }
}

fn find<'a>(list: &'a [BranchRow], name: &str) -> &'a BranchRow {
    list.iter()
        .find(|row| row.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no row {name}"))
}

fn names(list: &[BranchRow]) -> Vec<String> {
    list.iter().filter_map(|row| row.name.clone()).collect()
}

fn base_world() -> World {
    let mut world = World::new();
    world.refs = vec![
        rr("refs/heads/main", 'a', "origin/main"),
        rr("refs/heads/int", 'b', "origin/int"),
        rr("refs/remotes/origin/main", 'a', ""),
        rr("refs/remotes/origin/int", 'b', ""),
    ];
    world
}

#[test]
fn local_and_remote_only_rows() {
    let mut world = base_world();
    world.refs.push(rr("refs/heads/feat", 'c', "origin/feat"));
    world.refs[4].track = Track {
        ahead: 2,
        behind: 1,
        gone: false,
    };
    world.refs.push(rr("refs/remotes/origin/feat", 'c', ""));
    world.refs.push(rr("refs/remotes/origin/only", 'd', ""));
    let list = world.run();
    assert_eq!(names(&list), ["main", "int", "feat", "only"]);
    let feat = find(&list, "feat");
    assert!(feat.local && !feat.remote_only);
    assert_eq!(feat.remote, None);
    assert_eq!(feat.reference.as_deref(), Some("refs/heads/feat"));
    assert_eq!(feat.head.as_deref(), Some("ccccccc"));
    assert_eq!(feat.committed_at, Some(1_700_000_000_000));
    assert_eq!(feat.subject.as_deref(), Some("a subject"));
    assert_eq!(feat.upstream.as_deref(), Some("origin/feat"));
    assert_eq!((feat.upstream_ahead, feat.upstream_behind), (2, 1));
    assert!(!feat.upstream_gone);
    let only = find(&list, "only");
    assert!(!only.local && only.remote_only);
    assert_eq!(only.remote.as_deref(), Some("origin"));
    assert_eq!(only.reference.as_deref(), Some("refs/remotes/origin/only"));
    assert_eq!(only.upstream, None);
}

#[test]
fn remote_duplicate_of_local_dropped() {
    let mut world = base_world();
    world.refs.push(rr("refs/heads/feat", 'c', ""));
    world.refs.push(rr("refs/remotes/origin/feat", 'c', ""));
    world.refs.push(rr("refs/remotes/origin/HEAD", 'a', ""));
    let list = world.run();
    assert_eq!(names(&list), ["main", "int", "feat"]);
    assert_eq!(list.len(), 3);
}

#[test]
fn remote_split_on_known_remote_with_slash_in_branch() {
    let mut world = base_world();
    world
        .refs
        .push(rr("refs/remotes/origin/agent/ask", 'c', ""));
    world.remotes.push(("origin/fork".to_string(), false));
    world.refs.push(rr("refs/remotes/origin/fork/x", 'd', ""));
    world.refs.push(rr("refs/remotes/stranger/y/z", 'e', ""));
    let list = world.run();
    let ask = find(&list, "agent/ask");
    assert_eq!(ask.remote.as_deref(), Some("origin"));
    assert_eq!(
        ask.reference.as_deref(),
        Some("refs/remotes/origin/agent/ask")
    );
    let fork = find(&list, "x");
    assert_eq!(fork.remote.as_deref(), Some("origin/fork"));
    let stranger = find(&list, "y/z");
    assert_eq!(stranger.remote.as_deref(), Some("stranger"));
}

#[test]
fn detached_worktree_is_a_row() {
    let mut world = base_world();
    world.worktrees.push((
        entry("/r/.claude/worktrees/d", None, 'd'),
        place("/r/.claude/worktrees/d", false, None),
    ));
    world.facts.insert(
        sha('d'),
        (Some(5_000), "detached\u{1b} subject".to_string()),
    );
    world.count("origin/main", 'd', counts(2, 3));
    world.count("int", 'd', counts(2, 9));
    world.line('a', 'd', lines(10, 2, 3));
    let list = world.run();
    let row = list.last().unwrap();
    assert!(row.detached);
    assert_eq!(row.name, None);
    assert_eq!(row.reference, None);
    assert!(!row.local && !row.remote_only);
    assert_eq!(row.head.as_deref(), Some("ddddddd"));
    assert_eq!(row.committed_at, Some(5_000));
    assert_eq!(row.subject.as_deref(), Some("detached subject"));
    assert_eq!(
        row.worktree.as_ref().unwrap().path,
        "/r/.claude/worktrees/d"
    );
    assert_eq!(row.base.as_deref(), Some("origin/main"));
    assert_eq!((row.ahead, row.behind), (Some(2), Some(3)));
    assert_eq!(row.lines.map(|l| l.added), Some(10));
}

#[test]
fn bases_never_classified() {
    let mut world = base_world();
    world.count("origin/main", 'b', counts(1482, 0));
    world.count("int", 'a', counts(0, 1482));
    world.count("origin/main", 'a', counts(0, 0));
    world.count("int", 'b', counts(0, 0));
    let list = world.run();
    for name in ["main", "int"] {
        let row = find(&list, name);
        assert!(row.is_base, "{name}");
        assert_eq!(row.merged_into, None);
        assert_eq!(row.lines, None);
        assert_eq!(row.base, None);
        assert_eq!((row.ahead, row.behind), (None, None));
        assert!(!row.pending);
        assert!(!row.unrelated);
    }
    assert!(world.lines_asked.borrow().is_empty());
}

#[test]
fn only_the_default_and_integration_are_bases() {
    let mut world = base_world();
    world.refs.push(rr("refs/heads/feat", 'c', ""));
    world.refs.push(rr("refs/remotes/fork/main", 'd', ""));
    world.remotes.push(("fork".to_string(), false));
    let list = world.run();
    assert!(!find(&list, "feat").is_base);
    let mains: Vec<_> = list
        .iter()
        .filter(|row| row.name.as_deref() == Some("main"))
        .collect();
    assert_eq!(mains.len(), 1);
    assert!(mains[0].is_base);
    world.refs.remove(0);
    let list = world.run();
    let origin_main = list
        .iter()
        .find(|row| row.reference.as_deref() == Some("refs/remotes/origin/main"))
        .unwrap();
    assert!(origin_main.is_base);
    let fork_main = list
        .iter()
        .find(|row| row.reference.as_deref() == Some("refs/remotes/fork/main"))
        .unwrap();
    assert!(!fork_main.is_base);
}

#[test]
fn merged_into_integration_before_default() {
    let mut world = base_world();
    world.refs.push(rr("refs/heads/done", 'c', ""));
    world.count("origin/main", 'c', counts(0, 3));
    world.count("int", 'c', counts(0, 40));
    let list = world.run();
    let row = find(&list, "done");
    assert_eq!(row.merged_into.as_deref(), Some("int"));
    assert_eq!(row.base.as_deref(), Some("origin/main"));
    assert_eq!((row.ahead, row.behind), (Some(0), Some(3)));
    assert_eq!(row.lines, None);
    assert!(!row.pending);

    let mut world = base_world();
    world.refs.push(rr("refs/heads/old", 'c', ""));
    world.count("origin/main", 'c', counts(0, 3));
    world.count("int", 'c', counts(5, 40));
    let list = world.run();
    let row = find(&list, "old");
    assert_eq!(row.merged_into.as_deref(), Some("origin/main"));
}

#[test]
fn never_merged_into_own_short_name() {
    let mut world = World::new();
    world.remotes.push(("fork".to_string(), false));
    world.refs.push(rr("refs/remotes/fork/main", 'c', ""));
    world.refs.push(rr("refs/remotes/fork/int", 'c', ""));
    world.count("origin/main", 'c', counts(0, 0));
    world.count("int", 'c', counts(0, 0));
    world.line('b', 'c', lines(0, 0, 0));
    let list = world.run();
    let fork_main = list
        .iter()
        .find(|row| row.reference.as_deref() == Some("refs/remotes/fork/main"))
        .unwrap();
    assert_eq!(fork_main.merged_into.as_deref(), Some("int"));
    assert_eq!(fork_main.base.as_deref(), Some("int"));
    let fork_int = list
        .iter()
        .find(|row| row.reference.as_deref() == Some("refs/remotes/fork/int"))
        .unwrap();
    assert_eq!(fork_int.merged_into.as_deref(), Some("origin/main"));
    assert_eq!(fork_int.base.as_deref(), Some("origin/main"));
}

#[test]
fn fresh_lane_with_no_commits_is_merged() {
    let mut world = base_world();
    world.refs.push(rr("refs/heads/lane", 'a', ""));
    world.worktrees.push((
        entry("/r/.claude/worktrees/lane", Some("lane"), 'a'),
        place("/r/.claude/worktrees/lane", false, None),
    ));
    world.count("origin/main", 'a', counts(0, 0));
    world.count("int", 'a', counts(0, 1482));
    let list = world.run();
    let row = find(&list, "lane");
    assert_eq!(row.merged_into.as_deref(), Some("int"));
    assert_eq!(row.base.as_deref(), Some("origin/main"));
    assert_eq!((row.ahead, row.behind), (Some(0), Some(0)));
    assert_eq!(row.lines, None);
    assert!(!row.pending);
    assert!(row.worktree.is_some());
    assert!(world.lines_asked.borrow().is_empty());
}

#[test]
fn lines_only_for_not_merged() {
    let mut world = base_world();
    world.refs.push(rr("refs/heads/work", 'c', ""));
    world.refs.push(rr("refs/heads/done", 'd', ""));
    world.count("origin/main", 'c', counts(3, 1482));
    world.count("int", 'c', counts(3, 0));
    world.count("origin/main", 'd', counts(0, 3));
    world.count("int", 'd', counts(0, 0));
    world.line('b', 'c', lines(30, 4, 5));
    let list = world.run();
    let work = find(&list, "work");
    assert_eq!(work.base.as_deref(), Some("int"));
    assert_eq!(
        work.lines,
        Some(Lines {
            added: 30,
            removed: 4,
            files: 5
        })
    );
    assert_eq!(work.merged_into, None);
    assert_eq!(find(&list, "done").lines, None);
    assert_eq!(find(&list, "main").lines, None);
    assert_eq!(
        world.lines_asked.borrow().as_slice(),
        [(sha('b'), sha('c'))]
    );
}

#[test]
fn zero_ahead_of_a_non_base_needs_no_lines_read() {
    let mut world = base_world();
    world.refs.push(rr("refs/heads/lane", 'c', "origin/parent"));
    world.refs.push(rr("refs/remotes/origin/parent", 'd', ""));
    world.count("origin/main", 'c', counts(4, 0));
    world.count("int", 'c', counts(4, 2));
    world.count("origin/parent", 'c', counts(0, 7));
    let list = world.run();
    let row = find(&list, "lane");
    assert_eq!(row.base.as_deref(), Some("origin/parent"));
    assert_eq!(row.merged_into, None);
    assert_eq!(row.lines, Some(Lines::default()));
    assert!(world.lines_asked.borrow().is_empty());
}

#[test]
fn unknown_counts_are_pending() {
    let mut world = base_world();
    world.refs.push(rr("refs/heads/work", 'c', ""));
    world.count("origin/main", 'c', counts(3, 1482));
    let list = world.run();
    let row = find(&list, "work");
    assert!(row.pending);
    assert_eq!(row.base, None);
    assert_eq!((row.ahead, row.behind), (None, None));
    assert_eq!(row.merged_into, None);
    assert!(!row.unrelated);
    assert!(!find(&list, "main").pending);
}

#[test]
fn unknown_lines_are_pending_with_the_base_set() {
    let mut world = base_world();
    world.refs.push(rr("refs/heads/work", 'c', ""));
    world.count("origin/main", 'c', counts(3, 1482));
    world.count("int", 'c', counts(3, 0));
    let list = world.run();
    let row = find(&list, "work");
    assert!(row.pending);
    assert_eq!(row.base.as_deref(), Some("int"));
    assert_eq!(row.lines, None);
}

#[test]
fn unrelated_candidate_skipped_for_the_next() {
    let mut world = base_world();
    world.refs.push(rr("refs/heads/work", 'c', ""));
    world.count("origin/main", 'c', unrelated());
    world.count("int", 'c', counts(2, 9));
    world.line('b', 'c', lines(7, 1, 2));
    let list = world.run();
    let row = find(&list, "work");
    assert_eq!(row.base.as_deref(), Some("int"));
    assert!(!row.unrelated && !row.pending);

    let mut world = base_world();
    world.refs.push(rr("refs/heads/work", 'c', ""));
    world.count("origin/main", 'c', counts(1, 1));
    world.count("int", 'c', counts(2, 2));
    world.line('a', 'c', Some(None));
    world.line('b', 'c', lines(7, 1, 2));
    let list = world.run();
    let row = find(&list, "work");
    assert_eq!(row.base.as_deref(), Some("int"));
    assert_eq!((row.ahead, row.behind), (Some(2), Some(2)));
    assert_eq!(row.lines.map(|l| l.added), Some(7));
    assert!(!row.unrelated && !row.pending);
}

#[test]
fn every_candidate_unrelated_says_so() {
    let mut world = base_world();
    world.refs.push(rr("refs/heads/orphan", 'c', ""));
    world.count("origin/main", 'c', unrelated());
    world.count("int", 'c', unrelated());
    let list = world.run();
    let row = find(&list, "orphan");
    assert_eq!(row.base, None);
    assert!(row.unrelated);
    assert!(!row.pending);
    assert_eq!((row.ahead, row.behind), (None, None));
    assert_eq!(row.lines, None);
    assert_eq!(row.merged_into, None);

    let mut world = base_world();
    world.refs.push(rr("refs/heads/orphan", 'c', ""));
    world.count("origin/main", 'c', counts(1, 1));
    world.count("int", 'c', counts(1, 1));
    world.line('a', 'c', Some(None));
    world.line('b', 'c', Some(None));
    let list = world.run();
    let row = find(&list, "orphan");
    assert!(row.unrelated && !row.pending);
    assert_eq!(row.base, None);
}

#[test]
fn no_candidates_is_neither_pending_nor_unrelated() {
    let mut world = World::new();
    world.bases = Bases::default();
    world.refs.push(rr("refs/heads/solo", 'c', ""));
    let row = world.run().remove(0);
    assert_eq!(row.base, None);
    assert!(!row.pending && !row.unrelated && !row.is_base);
}

#[test]
fn upstream_ref_is_a_candidate() {
    let mut world = base_world();
    world.refs.push(rr("refs/heads/lane", 'c', "origin/parent"));
    world.refs.push(rr("refs/remotes/origin/parent", 'd', ""));
    world.count("origin/main", 'c', counts(20, 0));
    world.count("int", 'c', counts(20, 5));
    world.count("origin/parent", 'c', counts(1, 3));
    world.line('d', 'c', lines(4, 0, 1));
    let list = world.run();
    let row = find(&list, "lane");
    assert_eq!(row.base.as_deref(), Some("origin/parent"));
    assert_eq!(row.lines.map(|l| l.files), Some(1));
}

#[test]
fn upstream_with_own_name_is_not_a_base() {
    let mut world = base_world();
    world.refs.push(rr("refs/heads/lane", 'c', "origin/lane"));
    world.refs.push(rr("refs/remotes/origin/lane", 'c', ""));
    world.count("origin/main", 'c', counts(6, 0));
    world.count("int", 'c', counts(6, 2));
    world.count("origin/lane", 'c', counts(0, 0));
    world.line('a', 'c', lines(1, 1, 1));
    let list = world.run();
    let row = find(&list, "lane");
    assert_eq!(row.base.as_deref(), Some("origin/main"));
    assert_eq!(row.merged_into, None);
}

#[test]
fn on_github_from_remote() {
    let mut world = base_world();
    world.remotes.push(("gitlab".to_string(), false));
    world.refs.push(rr("refs/remotes/origin/a", 'c', ""));
    world.refs.push(rr("refs/remotes/gitlab/b", 'd', ""));
    world.refs.push(rr("refs/heads/local", 'e', ""));
    let list = world.run();
    assert!(find(&list, "a").on_github);
    assert!(!find(&list, "b").on_github);
    assert!(!find(&list, "local").on_github);
}

#[test]
fn worktree_attached_to_its_branch() {
    let mut world = base_world();
    world.refs.push(rr("refs/heads/feat", 'c', ""));
    let work = Work {
        added: 3,
        removed: 1,
        files: 2,
        uncommitted_files: 1,
        changed_at: Some(9),
        read_at: 10,
    };
    world.worktrees.push((
        entry("/r/w", Some("feat"), 'c'),
        place("/r/w", false, Some(work.clone())),
    ));
    let list = world.run();
    let row = find(&list, "feat");
    let tree = row.worktree.as_ref().unwrap();
    assert_eq!(tree.path, "/r/w");
    assert_eq!(tree.work, Some(work));
    assert!(find(&list, "main").worktree.is_none());
}

#[test]
fn no_worktree_counts_rows_without_one() {
    let mut world = base_world();
    world.refs.push(rr("refs/heads/feat", 'c', ""));
    world.refs.push(rr("refs/remotes/origin/only", 'd', ""));
    world
        .worktrees
        .push((entry("/r", Some("int"), 'b'), place("/r", true, None)));
    world
        .worktrees
        .push((entry("/r/w", Some("feat"), 'c'), place("/r/w", false, None)));
    let list = world.run();
    assert_eq!(list.len(), 4);
    assert_eq!(no_worktree(&list), 2);
    assert_eq!(no_worktree(&[]), 0);
}

#[test]
fn reply_keys_match_the_wire() {
    let mut world = base_world();
    world.refs.push(rr("refs/heads/feat", 'c', ""));
    world.count("origin/main", 'c', counts(1, 0));
    world.count("int", 'c', counts(1, 0));
    world.line('a', 'c', lines(2, 1, 1));
    let list = world.run();
    let json = serde_json::to_value(find(&list, "feat")).unwrap();
    assert_eq!(json["ref"], "refs/heads/feat");
    assert!(json.get("reference").is_none());
    for key in [
        "name",
        "local",
        "remote",
        "remote_only",
        "on_github",
        "is_base",
        "worktree",
        "head",
        "detached",
        "committed_at",
        "subject",
        "upstream",
        "upstream_ahead",
        "upstream_behind",
        "upstream_gone",
        "base",
        "ahead",
        "behind",
        "unrelated",
        "merged_into",
        "lines",
        "pending",
    ] {
        assert!(json.get(key).is_some(), "{key}");
    }
}
