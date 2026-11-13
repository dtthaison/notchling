use std::collections::{BTreeMap, BTreeSet};

use super::fakes_jobs::{ok, AnswerRunner};
use super::morning::{
    build, clean_comment, fetch_issues, fetch_issues_checked, is_repo_slug, is_worker_comment,
    item_id, latest_worker_comment, merged, GhComment, GhIssue, MorningKind,
};
use super::poller::LastReport;
use super::runner::CommandResult;
use super::status::{parse_status_date, NeedsYouEntry, ReadyEntry, StuckEntry, WorkerStatus};

const REPO: &str = "acme/app";
const T0: f64 = 1_800_000_000.0;

fn ready(n: i64, at: Option<f64>) -> ReadyEntry {
    ReadyEntry {
        repo: REPO.into(),
        issue: n,
        branch: format!("sd/{n}"),
        commit: "abc".into(),
        worktree: format!("/wt/{n}"),
        evidence_dir: format!("/wt/{n}/evidence"),
        ready_at: at.map(|o| T0 + o),
    }
}

fn needs(n: i64) -> NeedsYouEntry {
    NeedsYouEntry {
        repo: REPO.into(),
        issue: n,
        reason: "q".into(),
        ..NeedsYouEntry::default()
    }
}

fn reports(status: WorkerStatus) -> BTreeMap<String, LastReport> {
    BTreeMap::from([("m1".to_string(), LastReport::ok(status, T0))])
}

fn issues(list: Vec<GhIssue>) -> BTreeMap<String, Vec<GhIssue>> {
    BTreeMap::from([(REPO.to_string(), list)])
}

#[test]
fn build_orders_ready_needs_stuck() {
    let status = WorkerStatus {
        stuck: vec![StuckEntry {
            repo: REPO.into(),
            issue: 4,
            reason: "r".into(),
            ..StuckEntry::default()
        }],
        needs_you: vec![needs(9), needs(3)],
        ready: vec![ready(5, Some(200.0)), ready(8, Some(100.0)), ready(6, None)],
        ..WorkerStatus::default()
    };
    let known = issues(vec![GhIssue::new(8, "Eight"), GhIssue::new(5, "Five")]);

    let items = build(&reports(status), &known);

    let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "m1/acme/app#8",
            "m1/acme/app#5",
            "m1/acme/app#6",
            "m1/acme/app#3",
            "m1/acme/app#9",
            "m1/acme/app#4"
        ]
    );
    let kinds: Vec<MorningKind> = items.iter().map(|i| i.kind).collect();
    assert_eq!(
        kinds,
        [
            MorningKind::Ready,
            MorningKind::Ready,
            MorningKind::Ready,
            MorningKind::NeedsYou,
            MorningKind::NeedsYou,
            MorningKind::Stuck
        ]
    );
    assert_eq!(items[0].title, "Eight");
    assert_eq!(
        items[2].title, "#6",
        "no matching issue: the number stands in for the title"
    );
}

#[test]
fn build_skips_machines_without_status() {
    let r = BTreeMap::from([("m1".to_string(), LastReport::failed("timed out", T0))]);
    assert!(build(&r, &BTreeMap::new()).is_empty());
}

#[test]
fn build_lists_machines_in_name_order_within_a_group() {
    let status = WorkerStatus {
        needs_you: vec![needs(3)],
        ..WorkerStatus::default()
    };
    let r = BTreeMap::from([
        ("m2".to_string(), LastReport::ok(status.clone(), T0)),
        ("m1".to_string(), LastReport::ok(status, T0)),
    ]);
    let ids: Vec<String> = build(&r, &BTreeMap::new())
        .into_iter()
        .map(|i| i.id)
        .collect();
    assert_eq!(ids, ["m1/acme/app#3", "m2/acme/app#3"]);
}

#[test]
fn gh_issue_decodes_gh_output() {
    let json = r#"
    [{"number":12,"title":"Add login","labels":[{"name":"sd:ready","color":"fff"},{"name":"bug"}],
      "comments":[{"author":{"login":"sondoan"},"body":"please","createdAt":"2026-11-05T22:10:31Z"},
                  {"author":{"login":"bot"},"body":"m1: done\n<!-- sd:worker -->","createdAt":"2026-11-05T23:00:00Z"}]},
     {"number":13,"title":"Empty","labels":[],"comments":[]}]"#;
    let list = GhIssue::decode_list(json);
    assert_eq!(list.iter().map(|i| i.number).collect::<Vec<_>>(), [12, 13]);
    assert_eq!(list[0].title, "Add login");
    assert_eq!(list[0].labels, ["sd:ready", "bug"]);
    assert_eq!(
        list[0]
            .comments
            .iter()
            .map(|c| c.author.as_str())
            .collect::<Vec<_>>(),
        ["sondoan", "bot"]
    );
    assert_eq!(list[0].comments[1].body, "m1: done\n<!-- sd:worker -->");
    assert_eq!(
        list[0].comments[0].created_at,
        parse_status_date("2026-11-05T22:10:31Z")
    );
    assert!(GhIssue::decode_list("not json").is_empty());
}

#[test]
fn gh_issue_with_odd_parts_keeps_what_it_can_read() {
    let list = GhIssue::decode_list(
        r#"[{"number":1,"labels":[5,{"name":"a"},{"name":3}],"comments":[]}]"#,
    );
    assert_eq!(list[0].labels, ["a"]);
    assert_eq!(list[0].title, "");
    let list = GhIssue::decode_list(r#"[{"number":1,"title":"t","comments":[7,{"body":"x"}]}]"#);
    assert_eq!(list.len(), 1);
    assert!(list[0].comments.is_empty());
    assert!(GhIssue::decode_list(r#"[{"number":1},{"title":"no number"}]"#).is_empty());
    assert!(GhIssue::decode_list(r#"{"number":1}"#).is_empty());
}

#[test]
fn worker_comment_selection_and_cleaning() {
    let issue = GhIssue {
        number: 1,
        title: "t".into(),
        comments: vec![
            GhComment::body("m1: first worker note\n<!-- sd:worker -->"),
            GhComment::body("notchling: use recommended"),
            GhComment::body(
                "Verified.\n\n<!-- sd:kind=ready -->\nAll green.\n<!-- sd:worker -->  \n",
            ),
            GhComment::body("a human reply after"),
        ],
        ..GhIssue::default()
    };
    assert_eq!(
        latest_worker_comment(&issue, "m1").as_deref(),
        Some("Verified.\n\n\nAll green.")
    );

    let prefixed = GhIssue {
        number: 2,
        title: "t".into(),
        comments: vec![
            GhComment::body("m1: which database?"),
            GhComment::body("thanks"),
        ],
        ..GhIssue::default()
    };
    assert_eq!(
        latest_worker_comment(&prefixed, "m1").as_deref(),
        Some("m1: which database?")
    );
    assert_eq!(
        latest_worker_comment(&prefixed, "m2"),
        None,
        "another machine's prefix does not count"
    );
    let marks_only = GhIssue {
        comments: vec![GhComment::body("<!-- sd:worker -->")],
        ..GhIssue::default()
    };
    assert_eq!(
        latest_worker_comment(&marks_only, "m1"),
        None,
        "only marks: nothing to show"
    );
}

#[test]
fn comment_helpers() {
    assert!(is_worker_comment("x <!-- sd:worker -->\n  ", "m1"));
    assert!(is_worker_comment("m1: x", "m1"));
    assert!(!is_worker_comment("m10: x", "m1"));
    assert_eq!(clean_comment("<!-- sd:a -->"), None);
    assert_eq!(
        clean_comment("  hi <!--sd:x\nmulti-->  ").as_deref(),
        Some("hi")
    );
    assert_eq!(item_id("m1", "o/r", 4), "m1/o/r#4");
}

#[test]
fn build_attaches_the_comment() {
    let status = WorkerStatus {
        needs_you: vec![needs(2)],
        ..WorkerStatus::default()
    };
    let known = issues(vec![GhIssue {
        number: 2,
        title: "Q".into(),
        comments: vec![GhComment::body("Which one?\n<!-- sd:worker -->")],
        ..GhIssue::default()
    }]);
    let items = build(&reports(status), &known);
    assert_eq!(items[0].comment.as_deref(), Some("Which one?"));
}

#[test]
fn fetch_issues_asks_every_label_for_every_repo() {
    let runner = AnswerRunner::new(|argv| {
        let label = &argv[argv.iter().position(|a| a == "--label").expect("a label") + 1];
        let n = match label.as_str() {
            "sd:ready" => 1,
            "sd:needs-you" => 2,
            _ => 3,
        };
        ok(&format!(
            "[{{\"number\":{n},\"title\":\"T{n}\",\"labels\":[],\"comments\":[]}}]"
        ))
    });
    let got = fetch_issues(&[REPO.to_string(), REPO.to_string()], &runner);
    assert_eq!(
        got[REPO].iter().map(|i| i.number).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    let calls = runner.gh_calls();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls[0],
        [
            "issue",
            "list",
            "-R",
            REPO,
            "--label",
            "sd:ready",
            "--json",
            "number,title,labels,comments",
            "--limit",
            "100"
        ]
    );
}

#[test]
fn fetch_names_the_repos_whose_gh_call_failed_and_keeps_their_old_issues() {
    let runner = AnswerRunner::new(|argv| {
        if argv.contains(&"sd:needs-you".to_string()) {
            CommandResult::new(1, "", "network")
        } else {
            ok(r#"[{"number":1,"title":"New","labels":[],"comments":[]}]"#)
        }
    });
    let (fetched, failed) = fetch_issues_checked(&[REPO.to_string()], &runner);
    assert_eq!(failed, BTreeSet::from([REPO.to_string()]));
    let old = issues(vec![GhIssue::new(1, "Old"), GhIssue::new(2, "Two")]);
    let titles = |m: &BTreeMap<String, Vec<GhIssue>>| -> Vec<String> {
        m[REPO].iter().map(|i| i.title.clone()).collect()
    };
    assert_eq!(titles(&merged(&old, &fetched, &failed)), ["Old", "Two"]);
    assert_eq!(titles(&merged(&old, &fetched, &BTreeSet::new())), ["New"]);
    assert_eq!(
        titles(&merged(&BTreeMap::new(), &fetched, &failed)),
        ["New"]
    );
}

#[test]
fn a_repo_name_that_is_not_owner_slash_name_never_reaches_gh() {
    for good in ["acme/app", "a-b/c_d.e", "A1/b2"] {
        assert!(is_repo_slug(good), "{good}");
    }
    let bad = [
        "",
        "app",
        "-x/app",
        "acme/-app",
        "acme/app extra",
        "acme/app;id",
        "a/b/c",
        "/app",
        "acme/",
        "a/b\nc",
    ];
    for repo in bad {
        assert!(!is_repo_slug(repo), "{repo:?}");
    }
    let runner = AnswerRunner::silent();
    let (fetched, failed) = fetch_issues_checked(&bad.map(String::from), &runner);
    assert!(
        runner.calls().is_empty(),
        "something reached gh: {:?}",
        runner.calls()
    );
    assert!(fetched.is_empty());
    assert_eq!(failed.len(), bad.len());
}
