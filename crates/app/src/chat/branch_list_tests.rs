use serde_json::json;

use super::*;
use crate::chat::worktrees::sub_text;

const NOW: i64 = 100_000_000;
const MIN: i64 = 60_000;
const MAIN: &str = "/r/main";

fn branch(name: &str) -> Branch {
    Branch {
        name: Some(name.into()),
        reference: Some(format!("refs/heads/{name}")),
        local: true,
        base: Some("int".into()),
        ahead: Some(2),
        behind: Some(0),
        lines: Some(Lines {
            added: 10,
            removed: 2,
            files: 3,
        }),
        committed_at: Some(NOW - 6 * MIN),
        ..Branch::default()
    }
}

fn remote(name: &str) -> Branch {
    Branch {
        name: Some(name.into()),
        reference: Some(format!("refs/remotes/origin/{name}")),
        local: false,
        remote: Some("origin".into()),
        remote_only: true,
        on_github: true,
        ..branch(name)
    }
}

fn work(added: u64, removed: u64, files: u64, uncommitted: u64) -> Option<Work> {
    Some(Work {
        added,
        removed,
        files,
        uncommitted_files: uncommitted,
        changed_at: None,
        read_at: NOW,
    })
}

fn checked_out(mut b: Branch, path: &str, work: Option<Work>) -> Branch {
    b.worktree = Some(WorktreeOf {
        path: path.into(),
        real: path.into(),
        main: path == MAIN,
        current: false,
        missing: false,
        work,
    });
    b
}

fn current(mut b: Branch) -> Branch {
    if let Some(w) = b.worktree.as_mut() {
        w.current = true;
    }
    b
}

fn base_branch(name: &str) -> Branch {
    Branch {
        is_base: true,
        base: None,
        ahead: None,
        behind: None,
        lines: None,
        ..branch(name)
    }
}

fn merged(mut b: Branch) -> Branch {
    b.merged_into = Some("int".into());
    b.lines = None;
    b.ahead = Some(0);
    b
}

fn list(branches: Vec<Branch>) -> BranchList {
    BranchList {
        name: "notchling".into(),
        default: Some("origin/main".into()),
        integration: Some("int".into()),
        fetched_at: Some(NOW - 120 * MIN),
        truncated: false,
        pending: 0,
        branches,
    }
}

fn input(branches: Vec<Branch>) -> PanelInput {
    PanelInput {
        list: Some(Arc::new(list(branches))),
        failed: None,
        occupancy: Vec::new(),
        session_names: HashMap::new(),
        minute_ms: NOW,
    }
}

fn names(model: &PanelModel) -> Vec<String> {
    model
        .groups
        .iter()
        .flat_map(|(_, rows)| rows.iter().map(|r| r.name.clone()))
        .collect()
}

fn group_names(model: &PanelModel, group: Group) -> Vec<String> {
    model
        .groups
        .iter()
        .filter(|(g, _)| *g == group)
        .flat_map(|(_, rows)| rows.iter().map(|r| r.name.clone()))
        .collect()
}

fn row<'a>(model: &'a PanelModel, name: &str) -> &'a PanelRow {
    model
        .groups
        .iter()
        .flat_map(|(_, rows)| rows.iter())
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no row {name} in {:?}", names(model)))
}

fn sub(model: &PanelModel, name: &str) -> String {
    sub_text(&row(model, name).sub)
}

fn running(since: i64, id: &str) -> Option<Occupancy> {
    Some(Occupancy::Running {
        since_ms: since,
        open: Open::Session(id.into()),
        more: 0,
    })
}

#[test]
fn parse_reply_fields() {
    assert_eq!(parse_branch_list(&json!({"repo": false})), None);
    assert_eq!(parse_branch_list(&json!("x")), None);
    let reply = json!({
        "repo": true, "name": "notchling", "default": "origin/main", "integration": "int",
        "fetched_at": 1234, "truncated": true, "pending": 2,
        "branches": [
            {"name": "agent/ask", "ref": "refs/heads/agent/ask", "local": true, "remote": null,
             "remote_only": false, "on_github": false, "is_base": false,
             "worktree": {"path": "/r/lane", "real": "/real/lane", "main": false, "current": true,
                          "missing": true,
                          "work": {"added": 10, "removed": 2, "files": 3, "uncommitted_files": 1,
                                   "changed_at": 77, "read_at": 88}},
             "head": "abc1234", "detached": false, "committed_at": 99, "subject": "ask: store",
             "upstream": "origin/agent/ask", "upstream_ahead": 1, "upstream_behind": 2,
             "upstream_gone": false, "base": "int", "ahead": 3, "behind": 4, "unrelated": false,
             "merged_into": null, "lines": {"added": 5, "removed": 6, "files": 7},
             "pending": false},
            {"name": "held", "ref": "refs/remotes/origin/held", "local": false, "remote": "origin",
             "remote_only": true, "on_github": true, "is_base": true, "worktree": null,
             "detached": false, "upstream_gone": true, "base": null, "ahead": null,
             "unrelated": true, "merged_into": "origin/main", "lines": null, "pending": true},
            {"name": null, "ref": null, "worktree": {"path": "/r/det", "work": null},
             "head": "ffff111", "detached": true},
            {"name": null, "ref": null, "worktree": null}
        ]
    });
    let l = parse_branch_list(&reply).unwrap();
    assert_eq!(l.name, "notchling");
    assert_eq!(l.default.as_deref(), Some("origin/main"));
    assert_eq!(l.integration.as_deref(), Some("int"));
    assert_eq!(
        (l.fetched_at, l.truncated, l.pending),
        (Some(1234), true, 2)
    );
    assert_eq!(l.branches.len(), 3);
    let a = &l.branches[0];
    assert_eq!(a.name.as_deref(), Some("agent/ask"));
    assert_eq!(a.reference.as_deref(), Some("refs/heads/agent/ask"));
    assert!(a.local && !a.remote_only && !a.on_github && !a.is_base);
    let w = a.worktree.as_ref().unwrap();
    assert_eq!(
        (w.path.as_str(), w.real.as_str()),
        ("/r/lane", "/real/lane")
    );
    assert!(!w.main && w.current && w.missing);
    assert_eq!(
        w.work,
        work(10, 2, 3, 1).map(|w| Work {
            changed_at: Some(77),
            read_at: 88,
            ..w
        })
    );
    assert_eq!(
        (a.head.as_deref(), a.committed_at, a.subject.as_deref()),
        (Some("abc1234"), Some(99), Some("ask: store"))
    );
    assert_eq!(a.upstream.as_deref(), Some("origin/agent/ask"));
    assert_eq!(
        (a.upstream_ahead, a.upstream_behind, a.upstream_gone),
        (1, 2, false)
    );
    assert_eq!(
        (a.base.as_deref(), a.ahead, a.behind, a.unrelated),
        (Some("int"), Some(3), Some(4), false)
    );
    assert_eq!(
        a.lines,
        Some(Lines {
            added: 5,
            removed: 6,
            files: 7
        })
    );
    let r = &l.branches[1];
    assert!(r.remote_only && r.on_github && r.is_base && r.unrelated && r.pending);
    assert_eq!(r.remote.as_deref(), Some("origin"));
    assert_eq!(r.merged_into.as_deref(), Some("origin/main"));
    assert_eq!((r.base.clone(), r.ahead, r.lines), (None, None, None));
    assert!(r.upstream_gone);
    let d = &l.branches[2];
    assert!(d.detached && d.name.is_none());
    assert_eq!(d.worktree.as_ref().unwrap().real, "/r/det");
    assert_eq!(
        parse_branch_list(&json!({"repo": true}))
            .unwrap()
            .branches
            .len(),
        0
    );
}

#[test]
fn the_request_carries_the_folder() {
    let r = branch_list_request("/r");
    assert_eq!((r.op, r.args), ("git.branch_list", json!({"cwd": "/r"})));
}

#[test]
fn tabs_are_views_not_a_partition() {
    let dirty = checked_out(merged(branch("dirty")), "/r/lane-dirty", work(1, 1, 1, 3));
    let clean = checked_out(merged(branch("clean")), "/r/lane-clean", work(0, 0, 0, 0));
    let open = checked_out(branch("open"), "/r/lane-open", work(5, 5, 5, 0));
    let plain = merged(branch("plain"));
    let ahead = branch("ahead");
    let l = list(vec![
        dirty.clone(),
        clean.clone(),
        open.clone(),
        plain.clone(),
        ahead.clone(),
    ]);
    assert!(in_tab(&dirty, Tab::Working) && in_tab(&dirty, Tab::NotMerged));
    assert!(!in_tab(&dirty, Tab::Merged));
    assert!(in_tab(&clean, Tab::Working) && in_tab(&clean, Tab::Merged));
    assert!(!in_tab(&clean, Tab::NotMerged));
    assert!(in_tab(&open, Tab::Working) && in_tab(&open, Tab::NotMerged));
    assert!(!in_tab(&open, Tab::Merged));
    assert!(!in_tab(&plain, Tab::Working) && in_tab(&plain, Tab::Merged));
    assert!(!in_tab(&plain, Tab::NotMerged));
    assert!(!in_tab(&ahead, Tab::Working) && in_tab(&ahead, Tab::NotMerged));
    assert_eq!(tab_counts(&l), [3, 3, 2]);
}

#[test]
fn bases_only_in_working_without_worktree() {
    let main = base_branch("main");
    let int = checked_out(base_branch("int"), MAIN, None);
    for b in [&main, &int] {
        assert!(in_tab(b, Tab::Working));
        assert!(!in_tab(b, Tab::NotMerged));
        assert!(!in_tab(b, Tab::Merged));
    }
    let model = panel(&input(vec![main, int]), Tab::Working, "");
    assert_eq!(group_names(&model, Group::NoWorktree), vec!["main"]);
    assert_eq!(group_names(&model, Group::Worktrees), vec!["int"]);
    let mut merged_base = merged(base_branch("main"));
    merged_base.is_base = true;
    assert!(!in_tab(&merged_base, Tab::Merged));
}

#[test]
fn uncommitted_worktree_never_merged() {
    let dirty = checked_out(merged(branch("dirty")), "/r/lane", work(0, 0, 0, 2));
    assert!(!in_tab(&dirty, Tab::Merged));
    assert!(in_tab(&dirty, Tab::NotMerged));
}

#[test]
fn unread_worktree_waits_out_of_merged() {
    let unread = checked_out(merged(branch("lane")), "/r/lane", None);
    assert!(!in_tab(&unread, Tab::Merged));
    assert!(!in_tab(&unread, Tab::NotMerged));
    assert!(in_tab(&unread, Tab::Working));
    let read = checked_out(merged(branch("lane")), "/r/lane", work(0, 0, 0, 0));
    assert!(in_tab(&read, Tab::Merged));
}

#[test]
fn right_side_work_then_lines_then_pending() {
    let with_work = checked_out(branch("a"), "/r/a", work(100, 20, 4, 1));
    let lines_only = checked_out(branch("b"), "/r/b", None);
    let no_wt = branch("c");
    let mut waiting = branch("d");
    waiting.lines = None;
    waiting.pending = true;
    let mut waiting_wt = checked_out(branch("e"), "/r/e", None);
    waiting_wt.lines = None;
    waiting_wt.pending = true;
    let model = panel(
        &input(vec![with_work, lines_only, no_wt, waiting, waiting_wt]),
        Tab::NotMerged,
        "",
    );
    assert_eq!(row(&model, "a").right, Right::Lines(100, 20));
    assert_eq!(row(&model, "b").right, Right::Lines(10, 2));
    assert_eq!(row(&model, "c").right, Right::Lines(10, 2));
    assert_eq!(row(&model, "d").right, Right::Pending);
    assert_eq!(row(&model, "e").right, Right::Pending);
}

#[test]
fn merged_and_base_rows_have_no_numbers() {
    let clean = checked_out(merged(branch("clean")), "/r/clean", work(0, 0, 0, 0));
    let plain = merged(branch("plain"));
    let mut main = base_branch("main");
    main.upstream = Some("origin/main".into());
    let mut with_lines = base_branch("int");
    with_lines.lines = Some(Lines {
        added: 9,
        removed: 9,
        files: 9,
    });
    let model = panel(
        &input(vec![clean.clone(), plain, main.clone(), with_lines.clone()]),
        Tab::Merged,
        "",
    );
    assert_eq!(row(&model, "clean").right, Right::Empty);
    assert_eq!(row(&model, "plain").right, Right::Empty);
    let working = panel(&input(vec![clean, with_lines, main]), Tab::Working, "");
    assert_eq!(row(&working, "main").right, Right::Empty);
    assert_eq!(row(&working, "int").right, Right::Empty);
    assert_eq!(row(&working, "clean").right, Right::Empty);
    assert_eq!(
        sub(&working, "main"),
        "local \u{b7} last commit 6m ago \u{b7} even with origin"
    );
}

#[test]
fn groups_order() {
    let wt = |name: &str, path: &str| checked_out(branch(name), path, work(1, 1, 1, 0));
    let zed = wt("zed", "/r/lane-z");
    let alpha = wt("alpha", "/r/lane-a");
    let busy = wt("busy", "/r/lane-m");
    let mine = current(wt("mine", "/r/lane-q"));
    let mut old = branch("old");
    old.committed_at = Some(NOW - 90 * MIN);
    let mut fresh = branch("fresh");
    fresh.committed_at = Some(NOW - MIN);
    let mut unknown = branch("unknown");
    unknown.committed_at = None;
    let mut inp = input(vec![zed, alpha, busy, mine, old, unknown, fresh]);
    inp.occupancy = vec![
        ("/r/lane-m".into(), running(NOW - 41 * MIN, "s1")),
        ("/r/lane-a".into(), None),
    ];
    let model = panel(&inp, Tab::NotMerged, "");
    assert_eq!(
        model.groups.iter().map(|(g, _)| *g).collect::<Vec<_>>(),
        vec![Group::Worktrees, Group::NoWorktree]
    );
    assert_eq!(
        group_names(&model, Group::Worktrees),
        vec!["mine", "busy", "alpha", "zed"]
    );
    assert_eq!(
        group_names(&model, Group::NoWorktree),
        vec!["fresh", "old", "unknown"]
    );
    assert!(row(&model, "busy").working && !row(&model, "alpha").working);
    assert!(row(&model, "mine").current);
    let only = panel(&input(vec![branch("solo")]), Tab::NotMerged, "");
    assert_eq!(only.groups.len(), 1);
    assert_eq!(only.groups[0].0, Group::NoWorktree);
}

#[test]
fn filter_every_word_any_field_any_case() {
    let mut ask = checked_out(
        branch("agent/ask"),
        "/r/main/.claude/worktrees/lane-ask",
        work(1, 1, 1, 0),
    );
    ask.subject = Some("Store and WAIT".into());
    let other = branch("agent/ssh-join");
    let mut inp = input(vec![ask, other]);
    inp.occupancy = vec![(
        "/r/main/.claude/worktrees/lane-ask".into(),
        running(NOW - MIN, "s1"),
    )];
    inp.session_names
        .insert("s1".into(), "Lane: Build Ask".into());
    let shown = |q: &str| names(&panel(&inp, Tab::NotMerged, q));
    assert_eq!(shown("").len(), 2);
    assert_eq!(shown("ASK"), vec!["agent/ask"]);
    assert_eq!(shown("store wait"), vec!["agent/ask"]);
    assert_eq!(shown("lane-ask"), vec!["agent/ask"]);
    assert_eq!(shown("build ask"), vec!["agent/ask"]);
    assert_eq!(shown("ssh JOIN"), vec!["agent/ssh-join"]);
    assert_eq!(shown("ask zzz"), Vec::<String>::new());
    assert_eq!(shown("   "), shown(""));
}

#[test]
fn remote_only_github_or_remote_name() {
    let github = remote("held-fix");
    let mut gitlab = remote("other");
    gitlab.on_github = false;
    gitlab.remote = Some("upstream".into());
    let model = panel(&input(vec![github, gitlab]), Tab::NotMerged, "");
    assert!(sub(&model, "held-fix").starts_with("GitHub only"));
    assert!(sub(&model, "other").starts_with("upstream only"));
    assert!(!sub(&model, "held-fix").contains("local"));
}

#[test]
fn cloud_for_remote_only() {
    let model = panel(&input(vec![remote("r"), branch("l")]), Tab::NotMerged, "");
    assert!(row(&model, "r").cloud);
    assert!(!row(&model, "l").cloud);
}

#[test]
fn changes_off_for_refused_name() {
    let odd = branch("feat#1");
    let mut odd_ref = branch("fine");
    odd_ref.reference = Some("refs/heads/we ird".into());
    let ok = branch("agent/ask");
    let rem = remote("held");
    let model = panel(&input(vec![odd, odd_ref, ok, rem]), Tab::NotMerged, "");
    let odd = row(&model, "feat#1");
    assert!(!odd.buttons.changes);
    assert_eq!(odd.target, None);
    assert!(!row(&model, "fine").buttons.changes);
    let ok = row(&model, "agent/ask");
    assert!(ok.buttons.changes);
    assert_eq!(ok.target, Some(Target::Ref("refs/heads/agent/ask".into())));
    assert_eq!(
        row(&model, "held").target,
        Some(Target::Ref("refs/remotes/origin/held".into()))
    );
}

#[test]
fn worktree_targets_and_buttons() {
    let own = current(checked_out(branch("mine"), "/r/lane-me", work(1, 1, 1, 0)));
    let lane = checked_out(branch("lane"), "/r/lane", work(1, 1, 1, 0));
    let model = panel(&input(vec![own, lane]), Tab::Working, "");
    assert_eq!(row(&model, "mine").target, Some(Target::Own));
    assert_eq!(
        row(&model, "lane").target,
        Some(Target::Worktree("/r/lane".into()))
    );
    assert_eq!(row(&model, "lane").reveal.as_deref(), Some("/r/lane"));
    assert!(row(&model, "lane").buttons.finder);
    let loose = panel(&input(vec![branch("x")]), Tab::NotMerged, "");
    assert!(!row(&loose, "x").buttons.finder);
    assert_eq!(row(&loose, "x").reveal, None);
}

#[test]
fn changes_and_finder_off_for_missing_folder() {
    let mut gone = checked_out(branch("gone"), "/r/gone", None);
    gone.worktree.as_mut().unwrap().missing = true;
    let model = panel(&input(vec![gone]), Tab::Working, "");
    let r = row(&model, "gone");
    assert!(!r.buttons.changes && !r.buttons.finder);
    assert_eq!((r.target.clone(), r.reveal.clone()), (None, None));
}

#[test]
fn open_session_only_with_a_match() {
    let busy = checked_out(branch("busy"), "/r/busy", work(1, 1, 1, 0));
    let idle = checked_out(branch("idle"), "/r/idle", work(1, 1, 1, 0));
    let nobody = checked_out(branch("nobody"), "/r/nobody", work(1, 1, 1, 0));
    let agent = checked_out(branch("agent"), "/r/agent", work(1, 1, 1, 0));
    let mut inp = input(vec![busy, idle, nobody, agent]);
    inp.occupancy = vec![
        ("/r/busy".into(), running(NOW - MIN, "s1")),
        (
            "/r/idle".into(),
            Some(Occupancy::Idle {
                open: Open::Session("s2".into()),
            }),
        ),
        ("/r/nobody".into(), None),
        (
            "/r/agent".into(),
            Some(Occupancy::Running {
                since_ms: NOW - MIN,
                open: Open::Subagent("a1".into()),
                more: 0,
            }),
        ),
    ];
    let model = panel(&inp, Tab::Working, "");
    assert!(row(&model, "busy").buttons.open_session);
    assert_eq!(row(&model, "busy").open, Some(Open::Session("s1".into())));
    assert!(row(&model, "idle").buttons.open_session);
    assert!(!row(&model, "nobody").buttons.open_session);
    assert_eq!(row(&model, "nobody").open, None);
    assert_eq!(row(&model, "agent").open, Some(Open::Subagent("a1".into())));
}

#[test]
fn detached_row_name() {
    let det = Branch {
        name: None,
        reference: None,
        detached: true,
        head: Some("abc1234def".into()),
        worktree: Some(WorktreeOf {
            path: "/r/det".into(),
            real: "/r/det".into(),
            work: work(3, 1, 1, 0),
            ..WorktreeOf::default()
        }),
        base: Some("int".into()),
        ahead: Some(1),
        behind: Some(0),
        ..Branch::default()
    };
    let headless = Branch {
        head: None,
        worktree: Some(WorktreeOf {
            path: "/r/det2".into(),
            real: "/r/det2".into(),
            ..WorktreeOf::default()
        }),
        ..det.clone()
    };
    let model = panel(&input(vec![det, headless]), Tab::Working, "");
    let rows: Vec<&PanelRow> = model.groups[0].1.iter().collect();
    assert_eq!(rows[0].name, "Detached at abc1234");
    assert_eq!(rows[1].name, "detached");
    assert_eq!(rows[0].target, Some(Target::Worktree("/r/det".into())));
    assert_ne!(rows[0].key, rows[1].key);
    assert!(panel(&input(vec![]), Tab::Working, "").groups.is_empty());
}

#[test]
fn fetched_words() {
    let mut inp = input(vec![branch("a")]);
    let model = |inp: &PanelInput| panel(inp, Tab::NotMerged, "").fetched;
    assert_eq!(model(&inp), "Remote branches as of 2h ago");
    let mut l = list(vec![branch("a")]);
    l.fetched_at = Some(NOW - 10_000);
    inp.list = Some(Arc::new(l.clone()));
    assert_eq!(model(&inp), "Remote branches as of just now");
    l.fetched_at = None;
    inp.list = Some(Arc::new(l));
    assert_eq!(model(&inp), "Never fetched");
    assert_eq!(panel(&PanelInput::default(), Tab::Working, "").fetched, "");
}

#[test]
fn where_main_relative_or_absolute() {
    let main = checked_out(base_branch("int"), MAIN, None);
    let inside = checked_out(
        branch("ask"),
        "/r/main/.claude/worktrees/lane-ask",
        work(1, 1, 1, 0),
    );
    let outside = checked_out(branch("far"), "/elsewhere/lane-far", work(1, 1, 1, 0));
    let sibling = checked_out(branch("sib"), "/r/main-2", work(1, 1, 1, 0));
    let model = panel(
        &input(vec![main, inside, outside, sibling, branch("loose")]),
        Tab::Working,
        "",
    );
    assert!(sub(&model, "int").starts_with("main checkout"));
    assert!(sub(&model, "ask").starts_with(".claude/worktrees/lane-ask"));
    assert!(sub(&model, "far").starts_with("/elsewhere/lane-far"));
    assert!(sub(&model, "sib").starts_with("/r/main-2"));
    let all = panel(&input(vec![branch("loose")]), Tab::NotMerged, "");
    assert!(sub(&all, "loose").starts_with("local"));
}

#[test]
fn where_is_absolute_without_a_main_checkout() {
    let lane = checked_out(branch("ask"), "/r/main/lane", work(1, 1, 1, 0));
    let model = panel(&input(vec![lane]), Tab::Working, "");
    assert!(sub(&model, "ask").starts_with("/r/main/lane"));
}

#[test]
fn session_part_running_needs_input_idle_and_subagent_name() {
    let mk = |name: &str, path: &str| checked_out(branch(name), path, work(1, 1, 1, 0));
    let mut inp = input(vec![
        mk("a", "/r/a"),
        mk("b", "/r/b"),
        mk("c", "/r/c"),
        mk("d", "/r/d"),
    ]);
    inp.occupancy = vec![
        ("/r/a".into(), running(NOW - MIN, "sa")),
        (
            "/r/b".into(),
            Some(Occupancy::NeedsInput {
                open: Open::Session("sb".into()),
                more: 0,
            }),
        ),
        (
            "/r/c".into(),
            Some(Occupancy::Idle {
                open: Open::Session("sc".into()),
            }),
        ),
        (
            "/r/d".into(),
            Some(Occupancy::Running {
                since_ms: NOW - MIN,
                open: Open::Subagent("ad".into()),
                more: 0,
            }),
        ),
    ];
    for (id, name) in [
        ("sa", "Lane: Ask"),
        ("sb", "Lane: B"),
        ("sc", "Lane: C"),
        ("ad", "build the panel"),
    ] {
        inp.session_names.insert(id.into(), name.into());
    }
    let model = panel(&inp, Tab::Working, "");
    assert!(sub(&model, "a").contains("\u{201c}Lane: Ask\u{201d} running"));
    assert!(sub(&model, "b").contains("\u{201c}Lane: B\u{201d} needs input"));
    assert!(sub(&model, "c").contains("\u{201c}Lane: C\u{201d} idle"));
    assert!(sub(&model, "d").contains("\u{201c}build the panel\u{201d} running"));
    let kinds: Vec<_> = row(&model, "a")
        .sub
        .iter()
        .filter(|p| matches!(p, SubPart::Running(_)))
        .collect();
    assert_eq!(kinds.len(), 1);
    assert!(!row(&model, "c")
        .sub
        .iter()
        .any(|p| matches!(p, SubPart::Running(_))));
    let mut unnamed = inp.clone();
    unnamed.session_names.clear();
    let bare = panel(&unnamed, Tab::Working, "");
    assert!(!sub(&bare, "a").contains("running"));
    assert!(row(&bare, "a").buttons.open_session);
}

#[test]
fn ages_under_a_minute_say_just_now() {
    let mut fresh = branch("fresh");
    fresh.committed_at = Some(NOW - 10_000);
    fresh.subject = Some("tidy".into());
    let mut plain = branch("plain");
    plain.committed_at = Some(NOW - 30_000);
    plain.subject = None;
    let mut aged = branch("aged");
    aged.subject = Some("ask: store and wait".into());
    let mut blank = branch("blank");
    blank.subject = Some("\u{7}  ".into());
    let mut unknown = branch("unknown");
    unknown.committed_at = None;
    unknown.subject = Some("x".into());
    let model = panel(
        &input(vec![fresh, plain, aged, blank, unknown]),
        Tab::NotMerged,
        "",
    );
    assert!(sub(&model, "fresh").contains("last commit just now \u{201c}tidy\u{201d}"));
    assert!(sub(&model, "plain").contains("last commit just now"));
    assert!(!sub(&model, "plain").contains('\u{201c}'));
    assert!(sub(&model, "aged").contains("last commit 6m ago \u{201c}ask: store and wait\u{201d}"));
    assert!(sub(&model, "blank").contains("last commit 6m ago"));
    assert!(!sub(&model, "blank").contains('\u{201c}'));
    assert!(!sub(&model, "unknown").contains("last commit"));
}

#[test]
fn subjects_are_cut_and_plain() {
    let mut long = branch("long");
    long.subject = Some("x".repeat(500));
    let mut ctl = branch("ctl");
    ctl.subject = Some("a\nb\u{1b}[31mc".into());
    let model = panel(&input(vec![long, ctl]), Tab::NotMerged, "");
    let long_sub = sub(&model, "long");
    assert!(long_sub.contains(&"x".repeat(120)));
    assert!(!long_sub.contains(&"x".repeat(121)));
    assert!(sub(&model, "ctl").contains("\u{201c}ab[31mc\u{201d}"));
}

#[test]
fn sub_line_order_and_joins() {
    let mut b = checked_out(
        branch("agent/ask"),
        "/r/main/lane-ask",
        work(1240, 38, 23, 5),
    );
    b.subject = Some("ask: store and wait".into());
    b.upstream = Some("origin/agent/ask".into());
    b.upstream_ahead = 2;
    b.ahead = Some(2);
    let b = current(b);
    let mut inp = input(vec![checked_out(base_branch("int"), MAIN, None), b]);
    inp.occupancy = vec![
        ("/r/main".into(), None),
        ("/r/main/lane-ask".into(), running(NOW - 41 * MIN, "s")),
    ];
    inp.session_names.insert("s".into(), "Lane".into());
    let model = panel(&inp, Tab::Working, "");
    assert_eq!(
        sub(&model, "agent/ask"),
        "lane-ask \u{b7} this session \u{b7} \u{201c}Lane\u{201d} running \u{b7} 5 uncommitted \u{b7} last commit 6m ago \u{201c}ask: store and wait\u{201d} \u{b7} 2 to push \u{b7} not merged \u{b7} 23 files \u{b7} 2 ahead"
    );
    assert_eq!(row(&model, "agent/ask").right, Right::Lines(1240, 38));
}

#[test]
fn push_state_forms() {
    let mut gone = branch("gone");
    gone.upstream = Some("origin/gone".into());
    gone.upstream_gone = true;
    let mut none = branch("none");
    none.upstream = None;
    let mut even = branch("even");
    even.upstream = Some("origin/even".into());
    let mut behind = branch("behind");
    behind.upstream = Some("origin/behind".into());
    behind.upstream_behind = 3;
    let model = panel(
        &input(vec![gone, none, even, behind, remote("r")]),
        Tab::NotMerged,
        "",
    );
    assert!(sub(&model, "gone").contains("upstream gone"));
    assert!(sub(&model, "none").contains("no upstream"));
    assert!(sub(&model, "even").contains("even with origin"));
    assert!(sub(&model, "behind").contains("3 behind"));
    assert!(!sub(&model, "r").contains("upstream"));
}

#[test]
fn merged_into_and_not_merged_words() {
    let mut shown = merged(branch("shown"));
    shown.merged_into = Some("origin/main".into());
    let model = panel(
        &input(vec![shown, merged(branch("int-merged")), branch("open")]),
        Tab::Merged,
        "",
    );
    assert!(sub(&model, "shown").contains("merged into main"));
    assert!(sub(&model, "int-merged").contains("merged into int"));
    assert!(!sub(&model, "shown").contains("not merged"));
    let open = panel(&input(vec![branch("open")]), Tab::NotMerged, "");
    assert!(sub(&open, "open").contains("not merged"));
    let base = panel(&input(vec![base_branch("main")]), Tab::Working, "");
    assert!(!sub(&base, "main").contains("merged"));
}

#[test]
fn files_and_ahead_forms() {
    let mut behind = branch("behind");
    behind.behind = Some(1);
    let mut pending = branch("pending");
    pending.lines = None;
    pending.pending = true;
    let mut no_base = branch("no-base");
    no_base.base = None;
    no_base.ahead = None;
    no_base.lines = None;
    let model = panel(&input(vec![behind, pending, no_base]), Tab::NotMerged, "");
    assert!(sub(&model, "behind").ends_with("3 files \u{b7} 2 ahead, 1 behind"));
    assert!(sub(&model, "pending").ends_with("2 ahead"));
    assert!(!sub(&model, "pending").contains("files"));
    assert!(!sub(&model, "no-base").contains("ahead"));
    let mut one = branch("one");
    one.lines = Some(Lines {
        added: 1,
        removed: 0,
        files: 1,
    });
    let model = panel(&input(vec![one]), Tab::NotMerged, "");
    assert!(sub(&model, "one").contains("1 file \u{b7}"));
}

#[test]
fn unrelated_row_says_no_common_history() {
    let mut orphan = branch("orphan");
    orphan.unrelated = true;
    orphan.base = None;
    orphan.ahead = None;
    orphan.lines = None;
    let wt = checked_out(
        Branch {
            unrelated: true,
            base: None,
            ahead: None,
            lines: None,
            ..branch("lane")
        },
        "/r/lane",
        work(9, 9, 9, 0),
    );
    let model = panel(&input(vec![orphan, wt]), Tab::NotMerged, "");
    assert!(sub(&model, "orphan").ends_with("not merged \u{b7} no common history"));
    assert_eq!(row(&model, "orphan").right, Right::Empty);
    assert!(sub(&model, "lane").ends_with("no common history"));
    assert!(!sub(&model, "lane").contains("files"));
    assert_eq!(row(&model, "lane").right, Right::Empty);
}

#[test]
fn failed_after_good_keeps_rows() {
    let mut inp = input(vec![branch("a")]);
    inp.failed = Some("daemon is gone".into());
    let model = panel(&inp, Tab::NotMerged, "");
    assert_eq!(model.state, PanelState::Ready);
    assert_eq!(names(&model), vec!["a"]);
    assert_eq!(model.note, None);
}

#[test]
fn states_loading_failed_empty_no_match() {
    let loading = panel(&PanelInput::default(), Tab::Working, "");
    assert_eq!(loading.state, PanelState::Loading);
    assert_eq!(
        loading.note.as_deref(),
        Some("Reading the branches\u{2026}")
    );
    assert!(loading.groups.is_empty());
    let failed = panel(
        &PanelInput {
            failed: Some("not a repository".into()),
            ..PanelInput::default()
        },
        Tab::Working,
        "",
    );
    assert_eq!(failed.state, PanelState::Failed("not a repository".into()));
    assert_eq!(failed.note.as_deref(), Some("Can't read the branches."));
    let empty = panel(&input(vec![branch("a")]), Tab::Merged, "");
    assert_eq!(empty.state, PanelState::Ready);
    assert_eq!(empty.note.as_deref(), Some("No branches here."));
    let none = panel(&input(vec![branch("a")]), Tab::NotMerged, "zzz");
    assert_eq!(none.note.as_deref(), Some("No branches match."));
    assert!(none.groups.is_empty());
    assert_eq!(
        panel(&input(vec![branch("a")]), Tab::NotMerged, "").note,
        None
    );
}

#[test]
fn no_other_worktrees_note() {
    let main = checked_out(base_branch("int"), MAIN, None);
    let one = panel(&input(vec![main.clone(), branch("x")]), Tab::Working, "");
    assert_eq!(one.note.as_deref(), Some("No other worktrees."));
    assert_eq!(names(&one), vec!["int"]);
    let lane = checked_out(branch("lane"), "/r/lane", work(1, 1, 1, 0));
    let two = panel(&input(vec![main.clone(), lane]), Tab::Working, "");
    assert_eq!(two.note, None);
    let other_tab = panel(&input(vec![main.clone(), branch("x")]), Tab::NotMerged, "");
    assert_eq!(other_tab.note, None);
    let filtered = panel(&input(vec![main]), Tab::Working, "int");
    assert_eq!(filtered.note, None);
}

#[test]
fn truncated_note() {
    let mut inp = input(vec![branch("a")]);
    let mut l = list(vec![branch("a")]);
    l.truncated = true;
    inp.list = Some(Arc::new(l));
    let model = panel(&inp, Tab::NotMerged, "");
    assert!(model.truncated);
    assert_eq!(model.note.as_deref(), Some("Not all branches are shown."));
    assert!(!panel(&input(vec![branch("a")]), Tab::NotMerged, "").truncated);
}

#[test]
fn tab_counts_absent_while_loading() {
    let loading = panel(&PanelInput::default(), Tab::Working, "");
    assert_eq!(
        loading.tabs,
        [
            (Tab::Working, None),
            (Tab::NotMerged, None),
            (Tab::Merged, None)
        ]
    );
    let failed = panel(
        &PanelInput {
            failed: Some("x".into()),
            ..PanelInput::default()
        },
        Tab::Working,
        "",
    );
    assert!(failed.tabs.iter().all(|(_, n)| n.is_none()));
    let ready = panel(
        &input(vec![branch("a"), merged(branch("b")), base_branch("main")]),
        Tab::Working,
        "a",
    );
    assert_eq!(
        ready.tabs,
        [
            (Tab::Working, Some(1)),
            (Tab::NotMerged, Some(1)),
            (Tab::Merged, Some(1))
        ]
    );
    assert_eq!(tab_label(Tab::NotMerged, Some(6)), "Not Merged (6)");
    assert_eq!(tab_label(Tab::Working, None), "Working");
    assert_eq!(tab_label(Tab::Merged, Some(0)), "Merged (0)");
}

#[test]
fn repo_name_and_equal_inputs_give_equal_models() {
    let a = panel(&input(vec![branch("a")]), Tab::NotMerged, "");
    let b = panel(&input(vec![branch("a")]), Tab::NotMerged, "");
    assert_eq!(a, b);
    assert_eq!(a.repo, "notchling");
}
