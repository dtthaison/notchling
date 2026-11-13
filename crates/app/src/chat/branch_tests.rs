use super::*;
use serde_json::json;

fn reply() -> Value {
    json!({
        "repo": true,
        "includes_uncommitted": true,
        "branch": "feature",
        "head": "abc1234",
        "detached": false,
        "base": "origin/main",
        "merge_base": "8e73a97",
        "added": 98,
        "removed": 0,
        "file_count": 3,
        "uncommitted_files": 1,
        "truncated": false,
        "files": [
            {"path": "src/app.rs", "status": "M", "from": null, "added": 12, "removed": 3, "uncommitted": true},
            {"path": "docs/new.md", "status": "A", "from": null, "added": 86, "removed": 0, "uncommitted": false},
            {"path": "docs/guide.md", "status": "R", "from": "docs/intro.md", "added": null, "removed": null},
            {"path": "bad"},
        ],
    })
}

fn model(added: u64, removed: u64, files: u64) -> BranchModel {
    BranchModel {
        branch: "f".into(),
        base: "main".into(),
        merge_base: "abc".into(),
        files: vec![],
        added,
        removed,
        file_count: files,
        uncommitted_files: 0,
        truncated: false,
    }
}

#[test]
fn a_stat_reply_becomes_the_branch_the_base_the_totals_and_the_files() {
    let m = parse_stat(&reply()).unwrap();
    assert_eq!(
        (m.branch.as_str(), m.base.as_str()),
        ("feature", "origin/main")
    );
    assert_eq!(m.merge_base, "8e73a97");
    assert_eq!(
        m.summary(),
        Summary {
            added: 98,
            removed: 0,
            files: 3,
            base: "origin/main".into()
        }
    );
    assert_eq!(m.files.len(), 3, "a row without a status is left out");
    assert_eq!(m.files[0].added, Some(12));
    assert!(m.files[0].uncommitted && !m.files[1].uncommitted && !m.files[2].uncommitted);
    assert_eq!(m.files[2].from.as_deref(), Some("docs/intro.md"));
    assert_eq!(m.files[2].added, None);
    assert_eq!(m.counts_line(), "3 files \u{b7} 1 uncommitted");
    assert_eq!(m.find("docs/new.md").map(|f| f.added), Some(Some(86)));
    assert!(m.find("missing").is_none());
}

#[test]
fn no_repository_or_no_base_gives_no_model() {
    assert_eq!(parse_stat(&json!({"repo": false})), None);
    assert_eq!(parse_stat(&json!({})), None);
    assert_eq!(
        parse_stat(&json!({"repo": true, "base": null, "files": []})),
        None
    );
    assert_eq!(parse_stat(&json!({"repo": true, "base": ""})), None);
    assert_eq!(parse_stat(&json!("x")), None);
}

#[test]
fn a_detached_head_is_named_by_its_commit_and_counts_read_plainly() {
    let mut r = reply();
    r["branch"] = Value::Null;
    r["detached"] = json!(true);
    assert_eq!(parse_stat(&r).unwrap().branch, "Detached at abc1234");
    let one = BranchModel {
        file_count: 1,
        ..model(1, 0, 1)
    };
    assert_eq!(one.counts_line(), "1 file");
    assert_eq!(model(0, 0, 0).counts_line(), "0 files");
}

#[test]
fn a_diff_is_large_over_two_thousand_lines_or_a_hundred_files() {
    assert!(!model(1_000, 1_000, 100).is_large());
    assert!(model(1_000, 1_001, 3).is_large());
    assert!(model(5, 0, 101).is_large());
    assert!(!model(0, 0, 0).is_large());
    assert_eq!((LARGE_LINES, LARGE_FILES), (2_000, 100));
}

#[test]
fn files_split_into_a_name_and_a_folder_and_new_ones_take_the_add_color() {
    let f = |path: &str, status: &str| ChangedFile {
        path: path.into(),
        status: status.into(),
        from: None,
        added: None,
        removed: None,
        uncommitted: false,
    };
    assert_eq!(
        (f("a/b/c.rs", "M").name(), f("a/b/c.rs", "M").folder()),
        ("c.rs", "a/b")
    );
    assert_eq!(f("c.rs", "M").folder(), "");
    assert_eq!(f("c.rs", "A").name_ink(), Ink::Done);
    assert_eq!(f("c.rs", "?").name_ink(), Ink::Done);
    assert_eq!(f("c.rs", "D").name_ink(), Ink::Expired);
    assert_eq!(f("c.rs", "M").name_ink(), Ink::Text);
    assert_eq!(f("c.rs", "R").name_ink(), Ink::Text);
    assert!(f("c.rs", "A").is_new() && f("c.rs", "?").is_new() && !f("c.rs", "M").is_new());
}

#[test]
fn the_search_matches_every_word_anywhere_in_the_path_ignoring_case() {
    assert!(matches("crates/App/src/chat.rs", ""));
    assert!(matches("crates/App/src/chat.rs", "  "));
    assert!(matches("crates/App/src/chat.rs", "app chat"));
    assert!(matches("crates/App/src/chat.rs", "SRC/CHAT"));
    assert!(!matches("crates/App/src/chat.rs", "app zzz"));
    let files: Vec<ChangedFile> = ["a/one.rs", "b/two.rs", "a/three.md"]
        .iter()
        .map(|p| ChangedFile {
            path: (*p).into(),
            status: "M".into(),
            from: None,
            added: None,
            removed: None,
            uncommitted: false,
        })
        .collect();
    assert_eq!(visible(&files, ""), [0, 1, 2]);
    assert_eq!(visible(&files, "a/"), [0, 2]);
    assert_eq!(visible(&files, ".rs"), [0, 1]);
    assert_eq!(visible(&files, "nothing"), Vec::<usize>::new());
}

#[test]
fn the_base_menu_leaves_out_the_current_branch_and_puts_the_remote_default_first() {
    let b = parse_branches(&json!({
        "repo": true,
        "current": "feature",
        "default": "origin/main",
        "remote_default": "origin/main",
        "local": ["feature", "main", "dev", "main"],
    }))
    .unwrap();
    assert_eq!(b.choices(), ["origin/main", "main", "dev"]);
    assert_eq!(b.default.as_deref(), Some("origin/main"));
    let plain = parse_branches(&json!({"repo": true, "local": ["a"]})).unwrap();
    assert_eq!(plain.choices(), ["a"]);
    assert_eq!(parse_branches(&json!({"repo": false})), None);
}

#[test]
fn requests_carry_what_the_ops_take_and_nothing_else() {
    let s = stat_request("/w/app", None);
    assert_eq!(
        (s.op, s.args),
        ("git.branch_diff_stat", json!({"cwd": "/w/app"}))
    );
    let s = stat_request("/w/app", Some("dev"));
    assert_eq!(s.args, json!({"cwd": "/w/app", "base": "dev"}));
    let f = file_request("/w/app", "main", "src/a.rs", None);
    assert_eq!(
        (f.op, f.args),
        (
            "git.branch_diff_file",
            json!({"cwd": "/w/app", "base": "main", "path": "src/a.rs"})
        )
    );
    let f = file_request("/w/app", "main", "b.rs", Some("a.rs"));
    assert_eq!(f.args["from"], "a.rs");
    let b = branches_request("/w/app");
    assert_eq!((b.op, b.args), ("git.branches", json!({"cwd": "/w/app"})));
}

#[test]
fn the_preview_model_adds_up() {
    let m = preview_model();
    assert_eq!(m.file_count as usize, m.files.len());
    let added: u64 = m.files.iter().filter_map(|f| f.added).sum();
    assert_eq!(m.added, added);
    assert!(m.find(PREVIEW_PATH).is_some());
    assert!(!m.is_large());
}
