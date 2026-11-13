use super::*;
use serde_json::json;

fn reply() -> Value {
    json!({
        "repo": true,
        "branch": "main",
        "head": "abc1234",
        "detached": false,
        "initial": false,
        "upstream": "origin/main",
        "ahead": 2,
        "behind": 1,
        "truncated": false,
        "changes": [
            {"path": "src/app.rs", "status": "M", "from": null, "added": 12, "removed": 3},
            {"path": "notes.md", "status": "?", "from": null, "added": null, "removed": null},
            {"path": "bad"},
        ],
    })
}

#[test]
fn a_status_reply_becomes_the_branch_the_tracking_line_and_the_files() {
    let m = parse_status(&reply()).unwrap();
    assert_eq!(m.branch, "main");
    assert_eq!(
        m.tracking.as_deref(),
        Some("origin/main \u{b7} 2 ahead, 1 behind")
    );
    assert_eq!(m.files.len(), 2, "a row without a status is left out");
    assert_eq!(m.files[0].added, Some(12));
    assert_eq!(m.files[1].added, None);
    assert_eq!(m.title(), "Git \u{b7} 2");
}

#[test]
fn not_a_repository_and_odd_replies_give_no_section() {
    assert_eq!(parse_status(&json!({"repo": false})), None);
    assert_eq!(parse_status(&json!({})), None);
    assert_eq!(parse_status(&json!("x")), None);
}

#[test]
fn a_clean_tree_is_titled_plain_and_a_detached_head_says_where() {
    let clean = parse_status(&json!({"repo": true, "branch": "main", "changes": []})).unwrap();
    assert_eq!(clean.title(), "Git");
    assert_eq!(clean.tracking, None);
    let detached = parse_status(
        &json!({"repo": true, "branch": null, "detached": true, "head": "abc1234", "changes": []}),
    )
    .unwrap();
    assert_eq!(detached.branch, "Detached at abc1234");
}

#[test]
fn the_tracking_line_names_ahead_and_behind_only_when_there_are_any() {
    assert_eq!(tracking("origin/x", 0, 0), "origin/x \u{b7} up to date");
    assert_eq!(tracking("origin/x", 3, 0), "origin/x \u{b7} 3 ahead");
    assert_eq!(tracking("origin/x", 0, 4), "origin/x \u{b7} 4 behind");
}

#[test]
fn files_split_into_a_name_and_a_folder_and_ink_by_status() {
    let f = |path: &str, status: &str| GitFile {
        path: path.into(),
        status: status.into(),
        added: None,
        removed: None,
    };
    assert_eq!(f("a/b/c.rs", "M").name(), "c.rs");
    assert_eq!(f("a/b/c.rs", "M").folder(), "a/b");
    assert_eq!(f("c.rs", "M").folder(), "");
    assert_eq!(f("c.rs", "A").ink(), Ink::Done);
    assert_eq!(f("c.rs", "D").ink(), Ink::Expired);
    assert_eq!(f("c.rs", "?").ink(), Ink::TextFaint);
    assert_eq!(f("c.rs", "M").ink(), Ink::AccentText);
}

#[test]
fn only_forty_files_show_and_the_rest_are_counted() {
    let many = |n: usize, truncated| GitModel {
        branch: "main".into(),
        tracking: None,
        files: (0..n)
            .map(|i| GitFile {
                path: format!("f{i}"),
                status: "M".into(),
                added: None,
                removed: None,
            })
            .collect(),
        truncated,
    };
    assert_eq!(many(40, false).hidden_text(), None);
    assert_eq!(
        many(43, false).hidden_text().as_deref(),
        Some("3 more files not shown")
    );
    assert_eq!(
        many(500, true).hidden_text().as_deref(),
        Some("460+ more files not shown")
    );
}

#[test]
fn requests_carry_the_folder_and_the_path_and_nothing_else() {
    let s = status_request("/w/app");
    assert_eq!((s.op, s.args), ("git.status", json!({"cwd": "/w/app"})));
    let d = diff_request("/w/app", "src/a.rs");
    assert_eq!(
        (d.op, d.args),
        ("git.diff", json!({"cwd": "/w/app", "path": "src/a.rs"}))
    );
}

#[test]
fn nothing_is_asked_without_a_folder_or_while_hidden_or_while_one_is_out() {
    let mut gate = Gate::default();
    assert!(!gate.ask(Why::Tick, None, true, 0));
    assert!(!gate.ask(Why::Tick, Some(""), true, 0));
    assert!(!gate.ask(Why::Tick, Some("/w"), false, 0));
    assert!(gate.ask(Why::Tick, Some("/w"), true, 0));
    assert!(gate.inflight());
    assert!(
        !gate.ask(Why::Opened, Some("/other"), true, 10_000),
        "one at a time"
    );
    gate.done();
    assert!(gate.ask(Why::Opened, Some("/other"), true, 10_001));
}

#[test]
fn the_tick_asks_at_most_every_five_seconds_for_the_same_folder() {
    let mut gate = Gate::default();
    assert!(gate.ask(Why::Tick, Some("/w"), true, 1_000));
    gate.done();
    for at in [1_500, 3_000, 5_999] {
        assert!(!gate.ask(Why::Tick, Some("/w"), true, at), "{at}");
    }
    assert!(gate.ask(Why::Tick, Some("/w"), true, 6_000));
    gate.done();
    assert!(!gate.ask(Why::Tick, Some("/w"), true, 6_100));
}

#[test]
fn opening_a_panel_or_a_turn_ending_asks_sooner_but_not_in_a_burst() {
    let mut gate = Gate::default();
    assert!(gate.ask(Why::Folder, Some("/w"), true, 1_000));
    gate.done();
    assert!(
        !gate.ask(Why::TurnEnded, Some("/w"), true, 1_500),
        "a burst"
    );
    assert!(gate.ask(Why::TurnEnded, Some("/w"), true, 2_100));
    gate.done();
    assert!(!gate.ask(Why::Opened, Some("/w"), true, 2_200));
    assert!(gate.ask(Why::Opened, Some("/w"), true, 3_200));
    gate.done();
    assert!(
        gate.ask(Why::Tick, Some("/new"), true, 3_300),
        "a new folder is at once"
    );
}

#[test]
fn a_diff_is_told_apart_line_by_line_only_after_its_first_hunk() {
    let text = concat!(
        "diff --git a/x b/x\n",
        "index 1..2 100644\n",
        "--- a/x\n",
        "+++ b/x\n",
        "@@ -1,3 +1,3 @@ fn main\n",
        " keep\n",
        "-old\n",
        "--- a comment that starts with dashes\n",
        "+new\n",
        "++ and plus\n",
        "\\ No newline at end of file\n",
        "diff --git a/y b/y\n",
        "--- a/y\n",
    );
    let kinds: Vec<(DiffKind, usize)> = runs(text)
        .iter()
        .map(|(k, t)| (*k, t.lines().count()))
        .collect();
    assert_eq!(
        kinds,
        [
            (DiffKind::Meta, 4),
            (DiffKind::Hunk, 1),
            (DiffKind::Context, 1),
            (DiffKind::Removed, 2),
            (DiffKind::Added, 2),
            (DiffKind::Meta, 3),
        ]
    );
}

#[test]
fn runs_hold_their_lines_without_a_trailing_newline_and_an_empty_diff_has_none() {
    let r = runs("@@ -1 +1 @@\n+a\n+b\n");
    assert_eq!(r[1], (DiffKind::Added, "+a\n+b".to_owned()));
    assert!(runs("").is_empty());
}

#[test]
fn a_diff_reply_keeps_its_cut_and_untracked_flags() {
    let d = parse_diff(
        &json!({"path": "a", "text": "@@ -1 +1 @@\n+x\n", "truncated": true, "untracked": true}),
    )
    .unwrap();
    assert!(d.truncated && d.untracked);
    assert_eq!(d.runs.len(), 2);
    assert_eq!(parse_diff(&json!({"path": "a"})), None);
}

#[test]
fn added_and_removed_lines_use_the_done_and_expired_inks() {
    assert_eq!(DiffKind::Added.ink(), Ink::Done);
    assert_eq!(DiffKind::Removed.ink(), Ink::Expired);
    assert_eq!(DiffKind::Meta.ink(), Ink::TextFaint);
}

#[test]
fn the_preview_has_every_status_and_a_diff_with_both_kinds() {
    let m = preview_model();
    let letters: Vec<&str> = m.files.iter().map(|f| f.status.as_str()).collect();
    assert_eq!(letters, ["M", "A", "D", "M", "?"]);
    assert!(m.files.iter().any(|f| f.path == PREVIEW_PATH));
    let kinds: Vec<DiffKind> = preview_diff().runs.iter().map(|r| r.0).collect();
    assert!(kinds.contains(&DiffKind::Added) && kinds.contains(&DiffKind::Removed));
}
