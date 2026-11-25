use std::sync::Arc;

use super::actions::MorningActions;
use super::fakes_jobs::{bad, ok, AnswerRunner, Counter, Gate};
use super::morning::{MorningEntry, MorningItem, MorningKind};
use super::runner::{Call, CommandResult, Runner};
use super::status::NeedsYouEntry;
use super::tests_actions::{actions, item, m1, passed, plan, ready, ready_item, silent, REPO};

fn diff_runner() -> Arc<AnswerRunner> {
    Arc::new(AnswerRunner::new(|argv| {
        if argv.contains(&"check-ref-format".to_string()) {
            ok("sd/7\n")
        } else if argv.contains(&"diff".to_string()) {
            ok("DIFF TEXT")
        } else {
            ok("")
        }
    }))
}

#[test]
fn diff_fetches_without_tags_into_machines_ref_then_diffs_against_main_by_full_names() {
    let runner = diff_runner();
    let text = actions(&runner).diff(&item(MorningKind::Ready, 7));
    assert_eq!(text, "DIFF TEXT");
    let calls = runner.calls();
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|c| c[0] == "/usr/bin/git"));
    let git: Vec<&[String]> = calls.iter().map(|c| &c[1..]).collect();
    assert_eq!(
        git[0],
        ["-C", "/work/app", "check-ref-format", "--branch", "sd/7"]
    );
    assert_eq!(git[1][..2], ["-C", "/work/app"]);
    assert!(
        git[1].contains(&"--no-tags".to_string()),
        "a fetch for a diff must not bring the machine's tags"
    );
    assert!(git[1].contains(&"ssh://agent@m1.tail/wt/7".to_string()));
    assert!(git[1].contains(&"+refs/heads/sd/7:refs/machines/m1/7".to_string()));
    assert_eq!(
        git[2],
        [
            "-C",
            "/work/app",
            "diff",
            "refs/heads/main...refs/machines/m1/7"
        ]
    );
}

#[test]
fn diff_refuses_a_branch_name_git_would_read_as_an_option() {
    let runner = silent();
    let mut entry = ready(7);
    entry.branch = "--upload-pack=touch /tmp/x".into();
    let text = actions(&runner).diff(&ready_item(entry));
    assert!(text.contains("not a branch name"), "{text}");
    assert!(runner.calls().is_empty(), "nothing ran");
}

#[test]
fn diff_refuses_a_branch_name_git_rejects() {
    let runner = Arc::new(AnswerRunner::new(|argv| {
        if argv.contains(&"check-ref-format".to_string()) {
            bad("fatal: bad")
        } else {
            ok("")
        }
    }));
    let text = actions(&runner).diff(&item(MorningKind::Ready, 7));
    assert!(text.contains("not a branch name"), "{text}");
    assert_eq!(runner.calls().len(), 1, "only the name check ran");
}

#[test]
fn diff_reports_a_failed_fetch_and_a_failed_diff() {
    let runner = Arc::new(AnswerRunner::new(|argv| {
        if argv.contains(&"check-ref-format".to_string()) {
            ok("sd/7\n")
        } else if argv.contains(&"fetch".to_string()) {
            CommandResult::new(128, "", "fatal: could not read from remote\n")
        } else {
            ok("")
        }
    }));
    let a = actions(&runner);
    assert_eq!(
        a.diff(&item(MorningKind::Ready, 7)),
        "fatal: could not read from remote"
    );
    assert!(
        !a.is_running(&item(MorningKind::Ready, 7).id),
        "the diff gave the item back"
    );
}

#[test]
fn diff_never_runs_while_an_action_has_the_item() {
    let gate = Arc::new(Gate::default());
    let entered = Arc::new(Counter::default());
    let (g, e) = (gate.clone(), entered.clone());
    let runner = silent();
    let a = actions(&runner).with_merge(Arc::new(move |_, _, _, _| {
        e.bump();
        g.wait();
        passed()
    }));
    let it = item(MorningKind::Ready, 7);
    std::thread::scope(|scope| {
        let merge = scope.spawn(|| a.merge(&it));
        entered.wait_for_first();
        let text = a.diff(&it);
        assert!(text.contains("action is running"), "{text}");
        assert!(
            runner.calls().is_empty(),
            "the diff fetched while a merge was running"
        );
        gate.open();
        merge.join().expect("the merge ends");
    });
}

struct GatedRunner {
    gate: Gate,
    entered: Counter,
}

impl Runner for GatedRunner {
    fn run(&self, call: &Call) -> CommandResult {
        if call.argv.contains(&"fetch".to_string()) {
            self.entered.bump();
            self.gate.wait();
        }
        let branch = call.argv.contains(&"check-ref-format".to_string());
        CommandResult::ok(if branch { "sd/7\n" } else { "" })
    }
}

#[test]
fn a_merge_cannot_start_while_a_diff_holds_the_item() {
    let runner = Arc::new(GatedRunner {
        gate: Gate::default(),
        entered: Counter::default(),
    });
    let merge_calls = Arc::new(Counter::default());
    let c = merge_calls.clone();
    let a = MorningActions::new(runner.clone(), || vec![m1()], std::env::temp_dir())
        .with_plan(plan())
        .with_merge(Arc::new(move |_, _, _, _| {
            c.bump();
            passed()
        }));
    let it = item(MorningKind::Ready, 7);
    std::thread::scope(|scope| {
        let diff = scope.spawn(|| a.diff(&it));
        runner.entered.wait_for_first();
        assert!(a.is_running(&it.id));
        assert!(!a.merge(&it));
        assert_eq!(
            merge_calls.value(),
            0,
            "a merge started while the diff's fetch was running"
        );
        runner.gate.open();
        diff.join().expect("the diff ends");
    });
    assert!(!a.is_running(&it.id), "the diff gave the item back");
    assert!(a.merge(&it));
    assert_eq!(merge_calls.value(), 1);
}

#[test]
fn the_diff_fetch_runs_with_batch_mode_ssh() {
    let runner = diff_runner();
    actions(&runner).diff(&item(MorningKind::Ready, 7));
    let recs = runner.recs();
    let fetch = recs
        .iter()
        .find(|r| r.argv.contains(&"fetch".to_string()))
        .expect("a fetch");
    assert_eq!(
        fetch.env,
        vec![(
            "GIT_SSH_COMMAND".to_string(),
            "ssh -o BatchMode=yes -o ConnectTimeout=8".to_string()
        )]
    );
    assert!(!runner
        .calls()
        .iter()
        .any(|c| c.iter().any(|a| a.contains("core.sshCommand"))));
    assert_eq!(
        recs.iter().filter(|r| !r.env.is_empty()).count(),
        1,
        "only the fetch gets an environment"
    );
}

#[test]
fn diff_refuses_a_worktree_path_that_is_not_absolute() {
    for path in [
        ".evil.com/x",
        "@other/x",
        "relative/dir",
        "-oProxyCommand=x",
    ] {
        let runner = diff_runner();
        let mut entry = ready(7);
        entry.worktree = path.into();
        let text = actions(&runner).diff(&ready_item(entry));
        assert!(text.contains("not an absolute path"), "{path}: {text}");
        assert!(
            !runner
                .calls()
                .iter()
                .any(|c| c.contains(&"fetch".to_string())),
            "{path}"
        );
    }
}

#[test]
fn diff_refuses_a_worktree_path_with_odd_characters() {
    for path in [
        "/w/a b",
        "/w/a;touch /tmp/x",
        "/w/$(id)",
        "/w/../etc",
        "/w/it's",
        "/w/a\nb",
    ] {
        let runner = diff_runner();
        let mut entry = ready(7);
        entry.worktree = path.into();
        let text = actions(&runner).diff(&ready_item(entry));
        assert!(
            text.contains("characters Notchling does not pass to a command"),
            "{path:?}: {text}"
        );
        assert!(runner.calls().is_empty(), "{path:?} reached a command");
    }
}

#[test]
fn diff_of_a_needs_you_item_uses_its_branch_and_worktree_when_it_has_them() {
    let runner = diff_runner();
    let it = MorningItem {
        entry: MorningEntry::NeedsYou(NeedsYouEntry {
            repo: REPO.into(),
            issue: 7,
            branch: Some("sd/7".into()),
            worktree: Some("/wt/7".into()),
            ..NeedsYouEntry::default()
        }),
        ..item(MorningKind::NeedsYou, 7)
    };
    assert_eq!(actions(&runner).diff(&it), "DIFF TEXT");
    let triage = item(MorningKind::NeedsYou, 8);
    assert_eq!(
        actions(&diff_runner()).diff(&triage),
        "m1/acme/app#8 has no branch on the machine"
    );
}
