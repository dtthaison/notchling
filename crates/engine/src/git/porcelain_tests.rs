use super::porcelain::{letter, parse_numstat, parse_status, MAX_FILES};

fn z(records: &[&str]) -> String {
    let mut text = records.join("\0");
    text.push('\0');
    text
}

fn sample() -> String {
    z(&[
        "# branch.oid 0123456789abcdef0123456789abcdef01234567",
        "# branch.head main",
        "# branch.upstream origin/main",
        "# branch.ab +2 -1",
        "1 .M N... 100644 100644 100644 aaaa bbbb src/app.rs",
        "1 A. N... 000000 100644 100644 0000 cccc new file.rs",
        "2 R. N... 100644 100644 100644 aaaa bbbb R100 b/after.rs",
        "a/before.rs",
        "u UU N... 100644 100644 100644 100644 h1 h2 h3 conflict.rs",
        "? notes.md",
        "? caf\u{e9}/\u{65e5}\u{672c}.txt",
    ])
}

#[test]
fn the_branch_header_gives_name_head_upstream_and_counts() {
    let status = parse_status(&sample());
    assert_eq!(status.branch.as_deref(), Some("main"));
    assert_eq!(status.head.as_deref(), Some("0123456"));
    assert_eq!(status.upstream.as_deref(), Some("origin/main"));
    assert_eq!((status.ahead, status.behind), (2, 1));
    assert!(!status.detached && !status.initial && !status.truncated);
}

#[test]
fn every_kind_of_entry_becomes_a_change_with_a_letter() {
    let changes = parse_status(&sample()).changes;
    let rows: Vec<(&str, &str)> = changes
        .iter()
        .map(|c| (c.status.as_str(), c.path.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("M", "src/app.rs"),
            ("A", "new file.rs"),
            ("R", "b/after.rs"),
            ("U", "conflict.rs"),
            ("?", "notes.md"),
            ("?", "caf\u{e9}/\u{65e5}\u{672c}.txt"),
        ]
    );
    assert_eq!(changes[2].from.as_deref(), Some("a/before.rs"));
    assert_eq!(changes[0].from, None);
}

#[test]
fn no_upstream_detached_and_initial_heads() {
    let none = parse_status(&z(&["# branch.oid abc1234def", "# branch.head feature"]));
    assert_eq!(none.upstream, None);
    assert_eq!((none.ahead, none.behind), (0, 0));
    let detached = parse_status(&z(&["# branch.oid abc1234def", "# branch.head (detached)"]));
    assert!(detached.detached);
    assert_eq!(detached.branch, None);
    assert_eq!(detached.head.as_deref(), Some("abc1234"));
    let fresh = parse_status(&z(&["# branch.oid (initial)", "# branch.head main"]));
    assert!(fresh.initial);
    assert_eq!(fresh.branch.as_deref(), Some("main"));
    assert_eq!(fresh.head, None);
}

#[test]
fn a_clean_tree_and_junk_give_no_changes() {
    assert!(parse_status("").changes.is_empty());
    assert!(parse_status("1 .M short\0garbage\0! ignored.log\0")
        .changes
        .is_empty());
}

#[test]
fn more_than_the_cap_is_cut_and_says_so() {
    let mut records: Vec<String> = Vec::new();
    for n in 0..MAX_FILES + 20 {
        records.push(format!("? file{n}.txt"));
    }
    let refs: Vec<&str> = records.iter().map(String::as_str).collect();
    let status = parse_status(&z(&refs));
    assert_eq!(status.changes.len(), MAX_FILES);
    assert!(status.truncated);
    assert_eq!(status.changes[MAX_FILES - 1].path, "file499.txt");
}

#[test]
fn a_rename_past_the_cap_still_skips_its_old_name() {
    let mut records: Vec<String> = (0..MAX_FILES).map(|n| format!("? f{n}")).collect();
    records.push("2 R. N... 100644 100644 100644 aaaa bbbb R100 new".to_owned());
    records.push("old".to_owned());
    records.push("? after".to_owned());
    let refs: Vec<&str> = records.iter().map(String::as_str).collect();
    let status = parse_status(&z(&refs));
    assert!(status.truncated);
    assert_eq!(status.changes.len(), MAX_FILES);
}

#[test]
fn letters_follow_the_worst_of_the_two_columns() {
    assert_eq!(letter(".M"), "M");
    assert_eq!(letter("MM"), "M");
    assert_eq!(letter("A."), "A");
    assert_eq!(letter(".D"), "D");
    assert_eq!(letter("D."), "D");
    assert_eq!(letter("R."), "R");
    assert_eq!(letter("C."), "R");
    assert_eq!(letter("UU"), "U");
    assert_eq!(letter("AA"), "U");
    assert_eq!(letter("DD"), "U");
}

#[test]
fn numstat_counts_attach_by_path_including_renames_and_binaries() {
    let mut status = parse_status(&sample());
    let numstat = z(&[
        "12\t3\tsrc/app.rs",
        "5\t0\tnew file.rs",
        "-\t-\tcaf\u{e9}/\u{65e5}\u{672c}.txt",
    ]) + "4\t1\t\0a/before.rs\0b/after.rs\0";
    status.apply_numstat(&numstat);
    let get = |path: &str| {
        let c = status.changes.iter().find(|c| c.path == path).unwrap();
        (c.added, c.removed)
    };
    assert_eq!(get("src/app.rs"), (Some(12), Some(3)));
    assert_eq!(get("new file.rs"), (Some(5), Some(0)));
    assert_eq!(get("b/after.rs"), (Some(4), Some(1)));
    assert_eq!(
        get("caf\u{e9}/\u{65e5}\u{672c}.txt"),
        (None, None),
        "binary"
    );
    assert_eq!(get("notes.md"), (None, None), "untracked has none");
}

#[test]
fn numstat_ignores_what_it_cannot_read() {
    assert!(parse_numstat("").is_empty());
    assert!(parse_numstat("not numstat\0").is_empty());
    assert_eq!(parse_numstat("1\t2\ta.rs\0").len(), 1);
}
