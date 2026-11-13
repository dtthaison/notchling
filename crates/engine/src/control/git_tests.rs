use super::*;
use crate::control::launcher::{FakeLauncher, LaunchOutput};

#[test]
fn status() {
    let text = "## feat/data-connectors...origin/feat/data-connectors [ahead 2, behind 1]\n M src/app.ts\nA  src/new.ts\n D old.ts\nR  a/before.ts -> a/after.ts\n?? notes.md\nUU conflict.ts";
    let info = parse_status(text);
    assert_eq!(info.branch.as_deref(), Some("feat/data-connectors"));
    assert_eq!(
        info.upstream.as_deref(),
        Some("origin/feat/data-connectors")
    );
    assert_eq!((info.ahead, info.behind), (2, 1));
    let statuses: Vec<&str> = info.changes.iter().map(|c| c.status.as_str()).collect();
    assert_eq!(statuses, ["M", "A", "D", "R", "?", "U"]);
    let paths: Vec<&str> = info.changes.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "src/app.ts",
            "src/new.ts",
            "old.ts",
            "a/after.ts",
            "notes.md",
            "conflict.ts"
        ]
    );
}

#[test]
fn branch_without_upstream() {
    assert_eq!(parse_status("## main\n").branch.as_deref(), Some("main"));
    assert_eq!(parse_status("## main\n").upstream, None);
    assert_eq!(
        parse_status("## No commits yet on main\n")
            .branch
            .as_deref(),
        Some("main")
    );
    assert_eq!(
        parse_status("## HEAD (no branch)\n").branch.as_deref(),
        Some("detached HEAD")
    );
}

#[test]
fn numstat() {
    let mut info =
        parse_status("## main\n M src/app.ts\nR  a/before.ts -> a/after.ts\n M logo.png\n");
    info.apply_numstat("12\t3\tsrc/app.ts\n4\t0\ta/{before.ts => after.ts}\n-\t-\tlogo.png\n");
    assert_eq!(
        (info.changes[0].added, info.changes[0].removed),
        (Some(12), Some(3))
    );
    assert_eq!(
        (info.changes[1].added, info.changes[1].removed),
        (Some(4), Some(0))
    );
    assert_eq!(info.changes[2].added, None);
}

#[test]
fn branches() {
    let text = [
        "*\trefs/heads/feat\torigin/feat\t[ahead 1]\t5 minutes ago\t/r/wt",
        " \trefs/heads/main\torigin/main\t\t4 days ago\t/r",
        " \trefs/heads/old\t\t\t8 days ago\t",
        " \trefs/heads/dead\torigin/dead\t[gone]\t9 days ago\t/r/other",
        " \trefs/remotes/origin/HEAD\t\t\t4 days ago\t",
        " \trefs/remotes/origin/main\t\t\t4 days ago\t",
        " \trefs/remotes/origin/idea\t\t\t8 days ago\t",
    ]
    .join("\n");
    let b = parse_branches(&text, "/r");
    let names: Vec<&str> = b.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["feat", "main", "old", "dead", "origin/idea"]);
    assert!(b[0].is_current);
    assert_eq!(b[0].ahead, 1);
    assert_eq!(b[1].worktree, None);
    assert!(!b[2].has_upstream);
    assert!(b[3].gone);
    assert_eq!(b[3].worktree.as_deref(), Some("/r/other"));
    assert!(b[4].is_remote);
}

#[test]
fn a_rename_without_braces_and_a_quoted_path() {
    let mut info = parse_status("## main\nR  old.rs -> new.rs\n M \"with space.txt\"\n");
    assert_eq!(info.changes[0].path, "new.rs");
    assert_eq!(info.changes[1].path, "with space.txt");
    info.apply_numstat("1\t2\told.rs => new.rs\n");
    assert_eq!(info.changes[0].added, Some(1));
}

#[test]
fn status_letters() {
    let letters = |xy: &str| status_letter(xy);
    assert_eq!(letters("AA"), "U");
    assert_eq!(letters("DD"), "U");
    assert_eq!(letters(" D"), "D");
    assert_eq!(letters("MD"), "D");
    assert_eq!(letters("AM"), "A");
    assert_eq!(letters("C "), "R");
    assert_eq!(letters("MM"), "M");
}

const STATUS: &str = "## worktree-agent-a1b6a...origin/phase7-m3 [ahead 3]\n M crates/engine/src/lib.rs\n?? crates/engine/src/control/mod.rs\n?? crates/engine/src/control/plan.rs\n";
const NUMSTAT: &str = "1\t0\tcrates/engine/src/lib.rs\n";
const LOG: &str =
    "ed35102 plan: Milestone 3, the daemon acts (built and switched off)\t2 hours ago\n";

#[test]
fn recorded_output() {
    let mut info = parse_status(STATUS);
    info.apply_numstat(NUMSTAT);
    info.apply_log(LOG);
    assert_eq!(info.branch.as_deref(), Some("worktree-agent-a1b6a"));
    assert_eq!(info.upstream.as_deref(), Some("origin/phase7-m3"));
    assert_eq!((info.ahead, info.behind), (3, 0));
    assert_eq!(info.changes.len(), 3);
    assert_eq!(info.changes[0].added, Some(1));
    assert_eq!(info.changes[1].status, "?");
    assert_eq!(info.changes[1].added, None);
    assert_eq!(
        info.last_commit.as_deref(),
        Some("ed35102 plan: Milestone 3, the daemon acts (built and switched off)")
    );
    assert_eq!(info.last_commit_age.as_deref(), Some("2 hours ago"));
}

#[test]
fn git_never_takes_the_index_lock_and_runs_in_the_folder() {
    let commands = plan_git_status("/work/app");
    assert_eq!(commands.len(), 4);
    for command in &commands {
        assert_eq!(command.program(), "git");
        assert_eq!(
            &command.argv[1..6],
            [
                "-c",
                "core.quotepath=off",
                "--no-optional-locks",
                "-C",
                "/work/app"
            ]
        );
    }
    assert_eq!(
        &commands[0].argv[6..],
        [
            "status",
            "--porcelain=v1",
            "--branch",
            "--untracked-files=all"
        ]
    );
    assert_eq!(&commands[1].argv[6..], ["diff", "--numstat", "HEAD"]);
    assert_eq!(
        &commands[2].argv[6..],
        ["log", "-1", "--format=%h %s%x09%cr"]
    );
    assert_eq!(commands[3].argv[6], "for-each-ref");
}

#[test]
fn read_runs_the_four_commands_and_skips_numstat_for_a_clean_tree() {
    let fake = FakeLauncher::new();
    fake.push_output(Ok(LaunchOutput::ok(STATUS)));
    fake.push_output(Ok(LaunchOutput::ok(NUMSTAT)));
    fake.push_output(Ok(LaunchOutput::ok(LOG)));
    fake.push_output(Ok(LaunchOutput::ok(
        "*\trefs/heads/main\torigin/main\t\t1 day ago\t/work/app\n",
    )));
    let info = read("/work/app/", &fake).expect("a working copy");
    assert_eq!(fake.commands().len(), 4);
    assert_eq!(info.changes[0].added, Some(1));
    assert_eq!(info.branches.len(), 1);
    assert_eq!(info.branches[0].worktree, None);

    let clean = FakeLauncher::new();
    clean.push_output(Ok(LaunchOutput::ok("## main\n")));
    let info = read("/work/app", &clean).expect("a working copy");
    assert!(info.changes.is_empty());
    assert_eq!(clean.commands().len(), 3, "no numstat without changes");
}

#[test]
fn not_a_working_copy_is_none() {
    let fake = FakeLauncher::new();
    fake.push_output(Ok(LaunchOutput::failed(128, "fatal: not a git repository")));
    assert_eq!(read("/tmp/x", &fake), None);
    assert_eq!(read("", &fake), None);
    let noisy = FakeLauncher::new();
    noisy.push_output(Ok(LaunchOutput {
        code: 0,
        output: "## main\n".into(),
        stderr: "warning: untracked cache is disabled\n".into(),
    }));
    assert_eq!(
        read("/w", &noisy).expect("copy").branch.as_deref(),
        Some("main")
    );
}
