use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget, NodeRef};
use notchling_kit::Status;
use notchling_kit::ToolMark;
use serde_json::json;

use super::*;
use crate::chat::branch::BranchModel;
use crate::chat::data::{Session, Tool};
use crate::chat::git::{GitFile, GitModel};
use crate::chat::inspector::{Mark, SubagentRow};

const NOW: i64 = 100_000_000;
const MIN: i64 = 60_000;

fn agent(id: &str, cwd: &str, group: AgentGroup, since: i64) -> Agent {
    Agent {
        id: id.into(),
        name: id.into(),
        project: "p".into(),
        cwd: cwd.into(),
        group,
        label: String::new(),
        detail: None,
        since_ms: since,
        target: AgentTarget::Job(id.into()),
        suggested_reply: None,
        transcript_path: None,
        host: String::new(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
        node: None,
        worktree_path: None,
    }
}

fn session(agent: Agent) -> Session {
    Session {
        agent,
        tool: Tool::Claude,
        transcript: None,
    }
}

fn sub(id: &str, cwd: Option<&str>, running: bool, started: i64) -> SubagentRow {
    SubagentRow {
        id: id.into(),
        description: String::new(),
        detail: String::new(),
        status: Status::Working,
        status_word: String::new(),
        running,
        mark: Mark::Spinner,
        tool: ToolMark::Claude,
        doing: None,
        cwd: cwd.map(str::to_owned),
        started_ms: started,
        depth: 1,
        children: 0,
        open: false,
        more_after: Vec::new(),
    }
}

fn status(branch: &str) -> GitModel {
    GitModel {
        branch: branch.into(),
        files: Vec::new(),
        truncated: false,
        upstream: None,
        ahead: 0,
        behind: 0,
        initial: false,
        changed_at: None,
    }
}

fn changed(n: usize, added: u64, removed: u64) -> Vec<GitFile> {
    (0..n)
        .map(|i| GitFile {
            path: format!("f{i}"),
            status: "M".into(),
            added: (i == 0).then_some(added),
            removed: (i == 0).then_some(removed),
        })
        .collect()
}

fn stat(base: &str, added: u64, removed: u64) -> BranchModel {
    BranchModel {
        branch: "x".into(),
        base: base.into(),
        merge_base: String::new(),
        files: Vec::new(),
        added,
        removed,
        file_count: 0,
        uncommitted_files: 0,
        truncated: false,
    }
}

fn entry(path: &str, branch: &str) -> Worktree {
    Worktree {
        path: path.into(),
        real: path.into(),
        branch: Some(branch.into()),
        head: Some("abc1234def".into()),
        base: Some("int".into()),
        ..Worktree::default()
    }
}

fn current(path: &str, branch: &str) -> Worktree {
    Worktree {
        current: true,
        ..entry(path, branch)
    }
}

fn work(added: u64, removed: u64, files: u64) -> Option<Work> {
    Some(Work {
        added,
        removed,
        files,
        uncommitted_files: 0,
        changed_at: None,
    })
}

fn listing(list: Vec<Worktree>) -> Worktrees {
    Worktrees {
        name: "notchling".into(),
        default: Some("origin/main".into()),
        integration: Some("int".into()),
        fetched_at: None,
        no_worktree: 39,
        truncated: false,
        list,
    }
}

fn none(n: usize) -> Vec<Option<Occupancy>> {
    vec![None; n]
}

fn running(since_ms: i64) -> Option<Occupancy> {
    Some(Occupancy::Running {
        since_ms,
        open: Open::Session("s".into()),
        more: 0,
    })
}

fn texts(parts: &[SubPart]) -> String {
    sub_text(parts)
}

fn current_text(status: &GitModel, wt: Option<&Worktrees>, stat: Option<&BranchModel>) -> String {
    let occ = none(wt.map_or(0, |w| w.list.len()));
    texts(&section(status, stat, wt, &occ, NOW).current.sub)
}

fn upstream_text(entry: Worktree) -> String {
    let wt = listing(vec![Worktree {
        current: true,
        ..entry
    }]);
    current_text(&status("main"), Some(&wt), None)
}

fn worktree_with_upstream(
    branch: &str,
    upstream: Option<&str>,
    ahead: u64,
    behind: u64,
    gone: bool,
) -> Worktree {
    Worktree {
        upstream: upstream.map(str::to_owned),
        upstream_ahead: ahead,
        upstream_behind: behind,
        upstream_gone: gone,
        ..entry("/r", branch)
    }
}

#[test]
fn parse_reply_fields_and_repo_false() {
    assert_eq!(parse_worktrees(&json!({"repo": false})), None);
    assert_eq!(parse_worktrees(&json!("x")), None);
    let reply = json!({
        "repo": true, "name": "notchling", "default": "origin/main", "integration": "int",
        "fetched_at": 1234, "no_worktree": 39, "truncated": true,
        "worktrees": [
            {"path": "/r", "real": "/real/r", "main": true, "current": false,
             "branch": "int", "head": "abc1234", "detached": false, "locked": false,
             "missing": false, "upstream": "origin/int", "upstream_ahead": 1,
             "upstream_behind": 2, "upstream_gone": false, "base": "origin/main",
             "ahead": 3, "behind": 4, "unrelated": false, "committed_at": 99,
             "work": {"added": 10, "removed": 2, "files": 3, "uncommitted_files": 1,
                      "changed_at": 77, "read_at": 88}},
            {"path": "/r/lane", "real": "/r/lane", "main": false, "current": true,
             "branch": null, "head": "ffff111", "detached": true, "locked": true,
             "missing": true, "upstream": null, "base": null, "unrelated": true,
             "committed_at": null, "work": null},
            {"real": "/nope"}
        ]
    });
    let w = parse_worktrees(&reply).unwrap();
    assert_eq!(w.name, "notchling");
    assert_eq!(w.default.as_deref(), Some("origin/main"));
    assert_eq!(w.integration.as_deref(), Some("int"));
    assert_eq!(
        (w.fetched_at, w.no_worktree, w.truncated),
        (Some(1234), 39, true)
    );
    assert_eq!(w.list.len(), 2);
    let main = &w.list[0];
    assert_eq!((main.path.as_str(), main.real.as_str()), ("/r", "/real/r"));
    assert!(main.main && !main.current && !main.detached);
    assert_eq!(main.branch.as_deref(), Some("int"));
    assert_eq!(main.upstream.as_deref(), Some("origin/int"));
    assert_eq!((main.upstream_ahead, main.upstream_behind), (1, 2));
    assert_eq!(main.base.as_deref(), Some("origin/main"));
    assert_eq!(
        (main.ahead, main.behind, main.committed_at),
        (3, 4, Some(99))
    );
    assert_eq!(
        main.work,
        Some(Work {
            added: 10,
            removed: 2,
            files: 3,
            uncommitted_files: 1,
            changed_at: Some(77),
        })
    );
    let lane = &w.list[1];
    assert!(lane.current && lane.detached && lane.locked && lane.missing && lane.unrelated);
    assert_eq!(
        (lane.branch.clone(), lane.base.clone(), lane.work.clone()),
        (None, None, None)
    );
    let empty = parse_worktrees(&json!({"repo": true})).unwrap();
    assert!(empty.list.is_empty());
    assert_eq!(parse_work(&json!(null)), None);
}

#[test]
fn the_request_carries_the_folder_and_fresh_paths_only_when_given() {
    let r = worktrees_request("/r", &[]);
    assert_eq!((r.op, r.args), ("git.worktrees", json!({"cwd": "/r"})));
    let fresh = worktrees_request("/r", &["/r/a".to_owned(), "/r/b".to_owned()]);
    assert_eq!(fresh.args, json!({"cwd": "/r", "fresh": ["/r/a", "/r/b"]}));
    assert_eq!(WORKTREES_OP, "git.worktrees");
    assert_eq!(SHOWN_WORKTREES, 6);
}

#[test]
fn current_right_side_is_the_stat_vs_base() {
    let s = status("agent-run-oss");
    let st = stat("agent-run-oss", 501, 91);
    let sec = section(&s, Some(&st), None, &[], NOW);
    assert_eq!(sec.current.branch, "agent-run-oss");
    assert_eq!(sec.current.right, Some((501, 91)));
    assert_eq!(section(&s, None, None, &[], NOW).current.right, None);
}

#[test]
fn current_sub_line_every_part_present() {
    let mut s = status("agent-run-oss");
    s.files = changed(5, 52, 9);
    s.changed_at = Some(NOW - 2 * MIN);
    let mut cur = current("/r", "agent-run-oss");
    cur.upstream = Some("origin/agent-run-oss".into());
    cur.committed_at = Some(NOW - 30 * MIN);
    let wt = listing(vec![cur]);
    let st = stat("agent-run-oss", 501, 91);
    let sec = section(&s, Some(&st), Some(&wt), &none(1), NOW);
    assert_eq!(
        sec.current.sub,
        vec![
            SubPart::Faint("5 uncommitted".into()),
            SubPart::Added(52),
            SubPart::Removed(9),
            SubPart::Faint("\u{b7}".into()),
            SubPart::Faint("last 2m ago".into()),
            SubPart::Faint("\u{b7}".into()),
            SubPart::Faint("even with origin".into()),
            SubPart::Faint("\u{b7}".into()),
            SubPart::Faint("vs agent-run-oss".into()),
        ]
    );
    assert_eq!(
        texts(&sec.current.sub),
        "5 uncommitted +52 \u{2212}9 \u{b7} last 2m ago \u{b7} even with origin \u{b7} vs agent-run-oss"
    );
}

#[test]
fn current_sub_line_parts_absent() {
    let s = status("main");
    assert_eq!(current_text(&s, None, None), "no upstream");
    let mut only_deletes = status("main");
    only_deletes.files = changed(2, 0, 7);
    only_deletes.upstream = Some("origin/main".into());
    assert_eq!(
        current_text(&only_deletes, None, None),
        "2 uncommitted \u{2212}7 \u{b7} even with origin"
    );
}

#[test]
fn last_time_is_the_newer_of_commit_and_changes() {
    let mut s = status("main");
    let mut cur = current("/r", "main");
    cur.upstream = Some("origin/main".into());
    cur.committed_at = Some(NOW - 10 * MIN);
    let wt = listing(vec![cur]);
    assert_eq!(
        current_text(&s, Some(&wt), None),
        "last 10m ago \u{b7} even with origin"
    );
    s.changed_at = Some(NOW - 3 * MIN);
    assert_eq!(
        current_text(&s, Some(&wt), None),
        "last 3m ago \u{b7} even with origin"
    );
    s.changed_at = Some(NOW - 5_000);
    assert_eq!(
        current_text(&s, Some(&wt), None),
        "last just now \u{b7} even with origin"
    );
}

#[test]
fn uncommitted_500_plus_when_truncated() {
    let mut s = status("main");
    s.files = changed(500, 1, 0);
    s.truncated = true;
    s.upstream = Some("origin/main".into());
    let sec = section(&s, None, None, &[], NOW);
    assert_eq!(
        sec.current.sub[0],
        SubPart::Faint("500+ uncommitted".into())
    );
}

#[test]
fn upstream_forms() {
    let w = |u, a, b, g| worktree_with_upstream("main", u, a, b, g);
    assert_eq!(
        upstream_text(w(Some("origin/main"), 0, 0, false)),
        "even with origin"
    );
    assert_eq!(
        upstream_text(w(Some("origin/main"), 2, 0, false)),
        "2 to push"
    );
    assert_eq!(
        upstream_text(w(Some("origin/main"), 0, 3, false)),
        "3 behind"
    );
    assert_eq!(
        upstream_text(w(Some("origin/main"), 2, 3, false)),
        "2 to push, 3 behind"
    );
    assert_eq!(upstream_text(w(None, 0, 0, false)), "no upstream");
    assert_eq!(
        upstream_text(w(Some("origin/main"), 0, 0, true)),
        "upstream gone"
    );
    let other = worktree_with_upstream("agent/x", Some("origin/agent-run-oss"), 0, 0, false);
    assert_eq!(upstream_text(other), "even with origin/agent-run-oss");
    let nested = worktree_with_upstream("agent/x", Some("origin/agent/x"), 0, 0, false);
    assert_eq!(upstream_text(nested), "even with origin");
    let mut detached = worktree_with_upstream("main", None, 0, 0, false);
    detached.detached = true;
    detached.branch = None;
    assert_eq!(upstream_text(detached), "");
}

#[test]
fn vs_base_short_name_and_absent_before_stat() {
    let s = status("main");
    let wt = listing(vec![current("/r", "main")]);
    let vs = |base: &str, wt: Option<&Worktrees>| {
        let st = stat(base, 1, 1);
        current_text(&s, wt, Some(&st))
    };
    assert_eq!(vs("origin/main", Some(&wt)), "no upstream \u{b7} vs main");
    assert_eq!(
        vs("origin/agent-run-oss", Some(&wt)),
        "no upstream \u{b7} vs origin/agent-run-oss"
    );
    assert_eq!(vs("origin/main", None), "no upstream \u{b7} vs origin/main");
    assert_eq!(current_text(&s, Some(&wt), None), "no upstream");
}

#[test]
fn other_rows_files_ahead_running_or_last() {
    let mut lane = entry("/r/lane-ask", "agent/ask");
    lane.work = work(1240, 38, 23);
    lane.ahead = 2;
    lane.committed_at = Some(NOW - 90 * MIN);
    let mut idle = entry("/r/lane-idle", "agent/idle");
    idle.work = work(3, 1, 1);
    idle.ahead = 2;
    idle.behind = 1;
    idle.committed_at = Some(NOW - 6 * MIN);
    let wt = listing(vec![current("/r", "int"), lane, idle]);
    let occ = vec![None, running(NOW - 41 * MIN), None];
    let sec = section(&status("int"), None, Some(&wt), &occ, NOW);
    assert_eq!(sec.others.len(), 2);
    let first = &sec.others[0];
    assert_eq!(first.path, "/r/lane-ask");
    assert_eq!(first.branch, "agent/ask");
    assert_eq!(first.right, Some((1240, 38)));
    assert!(first.working && !first.missing);
    assert_eq!(
        texts(&first.sub),
        "23 files \u{b7} 2 ahead \u{b7} running 41m"
    );
    assert!(matches!(first.sub.last(), Some(SubPart::Running(_))));
    let second = &sec.others[1];
    assert!(!second.working);
    assert_eq!(
        texts(&second.sub),
        "1 file \u{b7} 2 ahead, 1 behind \u{b7} last 6m ago"
    );
    let more = vec![
        None,
        Some(Occupancy::Running {
            since_ms: NOW - 41 * MIN,
            open: Open::Session("s".into()),
            more: 1,
        }),
        None,
    ];
    let again = section(&status("int"), None, Some(&wt), &more, NOW);
    assert_eq!(
        texts(&again.others[0].sub),
        "23 files \u{b7} 2 ahead \u{b7} running 41m +1"
    );
}

#[test]
fn unread_work_shows_ahead_only() {
    let mut lane = entry("/r/lane", "agent/x");
    lane.ahead = 2;
    lane.committed_at = Some(NOW - 3 * MIN);
    let wt = listing(vec![current("/r", "int"), lane]);
    let sec = section(&status("int"), None, Some(&wt), &none(2), NOW);
    let row = &sec.others[0];
    assert_eq!(row.right, None);
    assert_eq!(texts(&row.sub), "2 ahead \u{b7} last 3m ago");
}

#[test]
fn missing_folder_row() {
    let mut lane = entry("/r/gone", "agent/gone");
    lane.missing = true;
    lane.work = work(5, 5, 1);
    let wt = listing(vec![current("/r", "int"), lane]);
    let occ = vec![None, running(NOW - MIN)];
    let sec = section(&status("int"), None, Some(&wt), &occ, NOW);
    let row = &sec.others[0];
    assert!(row.missing && !row.working);
    assert_eq!(row.right, None);
    assert_eq!(texts(&row.sub), "folder missing");
}

#[test]
fn unrelated_row_says_no_common_history() {
    let mut lane = entry("/r/orphan", "orphan");
    lane.unrelated = true;
    lane.base = None;
    lane.committed_at = Some(NOW - 4 * MIN);
    let wt = listing(vec![current("/r", "int"), lane]);
    let sec = section(&status("int"), None, Some(&wt), &none(2), NOW);
    let row = &sec.others[0];
    assert_eq!(row.right, None);
    assert_eq!(texts(&row.sub), "no common history \u{b7} last 4m ago");
}

#[test]
fn upstream_from_status_before_the_worktrees_reply() {
    let mut s = status("main");
    s.upstream = Some("origin/main".into());
    s.ahead = 2;
    assert_eq!(current_text(&s, None, None), "2 to push");
    s.ahead = 0;
    assert_eq!(current_text(&s, None, None), "even with origin");
    s.behind = 3;
    assert_eq!(current_text(&s, None, None), "3 behind");
}

fn many(count: usize) -> Worktrees {
    let mut list = vec![current("/r", "int")];
    for i in 0..count {
        list.push(entry(&format!("/r/w{i}"), &format!("b{i}")));
    }
    listing(list)
}

#[test]
fn rows_running_first_then_path_capped_at_six() {
    let wt = many(8);
    let mut occ = none(9);
    occ[6] = running(NOW - 5 * MIN);
    let sec = section(&status("int"), None, Some(&wt), &occ, NOW);
    let paths: Vec<&str> = sec.others.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(
        paths,
        ["/r/w5", "/r/w0", "/r/w1", "/r/w2", "/r/w3", "/r/w4"]
    );
    assert_eq!(
        sec.more.as_deref(),
        Some("2 more worktrees \u{b7} main and 39 branches")
    );
    assert_eq!(sec.title, "Git \u{b7} 1 running");
}

#[test]
fn more_row_three_forms_and_absent() {
    let mut wt = many(2);
    let more =
        |wt: &Worktrees| section(&status("int"), None, Some(wt), &none(wt.list.len()), NOW).more;
    assert_eq!(more(&wt).as_deref(), Some("main and 39 branches"));
    wt.no_worktree = 1;
    assert_eq!(more(&wt).as_deref(), Some("main and 1 branch"));
    wt.list[1].branch = Some("main".into());
    wt.no_worktree = 39;
    assert_eq!(more(&wt).as_deref(), Some("39 other branches"));
    wt.no_worktree = 1;
    assert_eq!(more(&wt).as_deref(), Some("1 other branch"));
    let past = many(7);
    assert_eq!(
        more(&past).as_deref(),
        Some("1 more worktree \u{b7} main and 39 branches")
    );
    wt.no_worktree = 0;
    assert_eq!(more(&wt), None);
    let mut no_default = many(1);
    no_default.default = None;
    assert_eq!(more(&no_default).as_deref(), Some("39 other branches"));
}

#[test]
fn more_worktrees_alone_when_no_other_branch() {
    let more =
        |wt: &Worktrees| section(&status("int"), None, Some(wt), &none(wt.list.len()), NOW).more;
    let mut past = many(7);
    past.no_worktree = 0;
    assert_eq!(more(&past).as_deref(), Some("1 more worktree"));
    let mut wider = many(9);
    wider.no_worktree = 0;
    assert_eq!(more(&wider).as_deref(), Some("3 more worktrees"));
    let mut shown = many(6);
    shown.no_worktree = 0;
    assert_eq!(more(&shown), None);
}

#[test]
fn title_git_alone_running_and_total() {
    assert_eq!(title(0, 0), "Git");
    assert_eq!(title(0, 1), "Git");
    assert_eq!(title(1, 1), "Git");
    assert_eq!(title(0, 3), "Git \u{b7} 3");
    assert_eq!(title(2, 3), "Git \u{b7} 2 running");
    let wt = many(2);
    let sec = section(&status("int"), None, Some(&wt), &none(3), NOW);
    assert_eq!(sec.title, "Git \u{b7} 3");
    assert_eq!(section(&status("int"), None, None, &[], NOW).title, "Git");
}

#[test]
fn owner_longest_prefix_over_path_and_real() {
    let paths = [("/r", "/real/r"), ("/r/lane", "/real/r/lane")];
    assert_eq!(owner("/r/lane/crates/a", &paths), Some(1));
    assert_eq!(owner("/real/r/lane/x", &paths), Some(1));
    assert_eq!(owner("/r/other", &paths), Some(0));
    assert_eq!(owner("/real/r", &paths), Some(0));
    assert_eq!(owner("/x", &paths), None);
    assert_eq!(owner("", &paths), None);
    assert_eq!(owner("/r/lane", &paths), Some(1));
}

#[test]
fn component_prefix_not_string_prefix() {
    assert_eq!(owner("/r/lane-2/x", &[("/r/lane", "/r/lane")]), None);
    let both = [("/r/lane", "/r/lane"), ("/r/lane-2", "/r/lane-2")];
    assert_eq!(owner("/r/lane-2/x", &both), Some(1));
    assert_eq!(owner("/r/lane/x", &both), Some(0));
}

#[test]
fn session_folder_worktree_path_over_cwd() {
    let mut a = agent("s", "/r", AgentGroup::Working, 1);
    assert_eq!(session_folder(&a), "/r");
    a.worktree_path = Some("/r/lane".into());
    assert_eq!(session_folder(&a), "/r/lane");
}

#[test]
fn entered_worktree_is_this_session_not_an_other_row() {
    let mut a = agent("s", "/r", AgentGroup::Working, NOW - 10 * MIN);
    a.worktree_path = Some("/r/lane".into());
    let paths = [("/r", "/r"), ("/r/lane", "/r/lane")];
    let occ = occupancy(&paths, &[session(a)], &[]);
    assert_eq!(occ.len(), 2);
    assert_eq!(occ[0], None);
    assert!(matches!(occ[1], Some(Occupancy::Running { .. })));
    let main = Worktree {
        main: true,
        ..entry("/r", "int")
    };
    let wt = listing(vec![main, current("/r/lane", "agent/lane")]);
    let sec = section(&status("agent/lane"), None, Some(&wt), &occ, NOW);
    assert_eq!(sec.others.len(), 1);
    assert_eq!(sec.others[0].path, "/r");
    assert!(!sec.others[0].working);
    assert!(!texts(&sec.others[0].sub).contains("running"));
}

#[test]
fn remote_rows_not_matched() {
    let mut a = agent("s", "/r/lane", AgentGroup::Working, NOW - MIN);
    a.node = Some(NodeRef {
        id: "n".into(),
        name: "m2".into(),
        online: true,
    });
    assert_eq!(
        occupancy(&[("/r/lane", "/r/lane")], &[session(a)], &[]),
        vec![None]
    );
}

#[test]
fn subagent_cwd_matches_with_started_ms() {
    let paths = [("/r", "/r"), ("/r/lane", "/r/lane")];
    let rows = [
        sub("sa1", Some("/r/lane/crates"), true, 5_000),
        sub("done", Some("/r"), false, 1),
        sub("nocwd", None, true, 2),
    ];
    let occ = occupancy(&paths, &[], &rows);
    assert_eq!(occ[0], None);
    assert_eq!(
        occ[1],
        Some(Occupancy::Running {
            since_ms: 5_000,
            open: Open::Subagent("sa1".into()),
            more: 0
        })
    );
}

#[test]
fn two_matches_longest_named_plus_one() {
    let paths = [("/r/lane", "/r/lane")];
    let sessions = [session(agent("s", "/r/lane", AgentGroup::Working, 9_000))];
    let rows = [sub("sa", Some("/r/lane"), true, 4_000)];
    let occ = occupancy(&paths, &sessions, &rows);
    assert_eq!(
        occ[0],
        Some(Occupancy::Running {
            since_ms: 4_000,
            open: Open::Subagent("sa".into()),
            more: 1
        })
    );
}

#[test]
fn needs_input_word() {
    let paths = [("/r", "/r"), ("/r/lane", "/r/lane")];
    let sessions = [session(agent("s", "/r/lane", AgentGroup::NeedsInput, 1))];
    let occ = occupancy(&paths, &sessions, &[]);
    assert_eq!(
        occ[1],
        Some(Occupancy::NeedsInput {
            open: Open::Session("s".into()),
            more: 0
        })
    );
    let mut lane = entry("/r/lane", "agent/x");
    lane.ahead = 1;
    let wt = listing(vec![current("/r", "int"), lane]);
    let sec = section(&status("int"), None, Some(&wt), &occ, NOW);
    assert_eq!(texts(&sec.others[0].sub), "1 ahead \u{b7} needs input");
    assert!(!sec.others[0].working);
}

#[test]
fn idle_match_keeps_open_without_dot() {
    let paths = [("/r/lane", "/r/lane")];
    let sessions = [
        session(agent("old", "/r/lane", AgentGroup::Completed, 1_000)),
        session(agent("new", "/r/lane", AgentGroup::Completed, 2_000)),
    ];
    let occ = occupancy(&paths, &sessions, &[]);
    assert_eq!(
        occ[0],
        Some(Occupancy::Idle {
            open: Open::Session("new".into())
        })
    );
    let mut lane = entry("/r/lane", "agent/x");
    lane.committed_at = Some(NOW - 2 * MIN);
    let wt = listing(vec![current("/r", "int"), lane]);
    let sec = section(
        &status("int"),
        None,
        Some(&wt),
        &[None, occ[0].clone()],
        NOW,
    );
    assert!(!sec.others[0].working);
    assert!(texts(&sec.others[0].sub).ends_with("last 2m ago"));
}

#[test]
fn same_minute_same_section() {
    let mut lane = entry("/r/lane", "agent/x");
    lane.committed_at = Some(NOW - 30_000);
    lane.work = work(1, 1, 1);
    let wt = listing(vec![current("/r", "int"), lane]);
    let occ = vec![None, running(NOW - 5 * MIN)];
    let a = section(&status("int"), None, Some(&wt), &occ, NOW);
    let b = section(&status("int"), None, Some(&wt), &occ, NOW);
    assert_eq!(a, b);
    let later = section(&status("int"), None, Some(&wt), &occ, NOW + MIN);
    assert_ne!(a, later);
}

#[test]
fn unmatched_session_change_keeps_section_equal() {
    let paths = [("/r", "/r"), ("/r/lane", "/r/lane")];
    let one = [session(agent("s", "/r/lane", AgentGroup::Working, 1_000))];
    let two = [
        session(agent("s", "/r/lane", AgentGroup::Working, 1_000)),
        session(agent(
            "elsewhere",
            "/other/place",
            AgentGroup::Working,
            2_000,
        )),
    ];
    let a = occupancy(&paths, &one, &[]);
    let b = occupancy(&paths, &two, &[]);
    assert_eq!(a, b);
    let wt = listing(vec![current("/r", "int"), entry("/r/lane", "agent/x")]);
    assert_eq!(
        section(&status("int"), None, Some(&wt), &a, NOW),
        section(&status("int"), None, Some(&wt), &b, NOW)
    );
}

#[test]
fn no_commit_yet_shows_name_and_uncommitted_only() {
    let mut s = status("main");
    s.initial = true;
    s.files = changed(2, 3, 0);
    s.upstream = Some("origin/main".into());
    let wt = listing(vec![current("/r", "main")]);
    let st = stat("origin/main", 9, 9);
    let sec = section(&s, Some(&st), Some(&wt), &none(1), NOW);
    assert_eq!(sec.current.branch, "main");
    assert_eq!(sec.current.right, None);
    assert_eq!(texts(&sec.current.sub), "2 uncommitted +3");
    s.files.clear();
    let clean = section(&s, None, Some(&wt), &none(1), NOW);
    assert!(clean.current.sub.is_empty());
}

#[test]
fn shown_base_strips_origin_only_for_default() {
    assert_eq!(shown_base("origin/main", Some("origin/main")), "main");
    assert_eq!(
        shown_base("origin/agent-run-oss", Some("origin/main")),
        "origin/agent-run-oss"
    );
    assert_eq!(
        shown_base("agent-run-oss", Some("origin/main")),
        "agent-run-oss"
    );
    assert_eq!(shown_base("origin/main", None), "origin/main");
    assert_eq!(shown_base("main", Some("main")), "main");
}

#[test]
fn detached_other_row_names_its_head() {
    let mut lane = entry("/r/d", "x");
    lane.branch = None;
    lane.detached = true;
    lane.head = Some("abc1234def".into());
    let wt = listing(vec![current("/r", "int"), lane]);
    let sec = section(&status("int"), None, Some(&wt), &none(2), NOW);
    assert_eq!(sec.others[0].branch, "Detached at abc1234");
}
