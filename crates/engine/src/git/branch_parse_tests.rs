use super::branch_parse::{
    assemble, finish, parse_branches, parse_name_status, BranchStat, MAX_BRANCHES, MAX_BRANCH_FILES,
};
use super::guard::clean_ref;
use std::collections::HashSet;

fn z(records: &[&str]) -> String {
    let mut text = records.join("\0");
    text.push('\0');
    text
}

#[test]
fn name_status_reads_one_path_for_most_and_two_for_renames() {
    let got = parse_name_status(&z(&[
        "A", "new.rs", "M", "a.rs", "D", "gone.rs", "T", "t", "R087", "old.rs", "moved.rs",
    ]));
    let flat: Vec<(&str, &str, Option<&str>)> = got
        .iter()
        .map(|(s, p, f)| (s.as_str(), p.as_str(), f.as_deref()))
        .collect();
    assert_eq!(
        flat,
        [
            ("A", "new.rs", None),
            ("M", "a.rs", None),
            ("D", "gone.rs", None),
            ("M", "t", None),
            ("R", "moved.rs", Some("old.rs")),
        ]
    );
    assert!(parse_name_status("").is_empty());
    assert!(parse_name_status(&z(&["R100", "only-one"])).is_empty());
}

#[test]
fn counts_join_by_path_and_untracked_files_come_after_with_their_lines() {
    let names = z(&["M", "a.rs", "R090", "old.rs", "b.rs", "A", "bin.png"]);
    let counts = z(&["3\t1\ta.rs", "2\t2\t", "old.rs", "b.rs", "-\t-\tbin.png"]);
    let uncommitted: HashSet<String> = ["a.rs".to_owned()].into_iter().collect();
    let (files, cut) = assemble(
        &names,
        &counts,
        &uncommitted,
        vec![("z.txt".into(), Some(4)), ("blob".into(), None)],
    );
    assert!(!cut);
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["a.rs", "b.rs", "bin.png", "blob", "z.txt"]);
    assert_eq!(
        (files[0].added, files[0].removed, files[0].uncommitted),
        (Some(3), Some(1), true)
    );
    assert_eq!(
        (files[1].status.as_str(), files[1].from.as_deref()),
        ("R", Some("old.rs"))
    );
    assert_eq!((files[1].added, files[1].removed), (Some(2), Some(2)));
    assert_eq!((files[2].added, files[2].removed), (None, None));
    assert_eq!(
        (files[3].status.as_str(), files[3].added, files[3].removed),
        ("?", None, None)
    );
    assert_eq!(
        (files[4].added, files[4].removed, files[4].uncommitted),
        (Some(4), Some(0), true)
    );
}

#[test]
fn totals_count_every_file_and_the_list_is_cut_at_the_cap() {
    let names: String = (0..MAX_BRANCH_FILES + 5)
        .flat_map(|n| ["M".to_owned(), format!("f{n:05}")])
        .map(|r| format!("{r}\0"))
        .collect();
    let counts: String = (0..MAX_BRANCH_FILES + 5)
        .map(|n| format!("2\t1\tf{n:05}\0"))
        .collect();
    let (files, cut) = assemble(&names, &counts, &HashSet::new(), vec![]);
    assert!(cut);
    let mut stat = BranchStat::default();
    finish(&mut stat, files, cut);
    assert_eq!(stat.files.len(), MAX_BRANCH_FILES);
    assert_eq!(stat.file_count, (MAX_BRANCH_FILES + 5) as u64);
    assert_eq!(stat.added, 2 * (MAX_BRANCH_FILES + 5) as u64);
    assert_eq!(stat.removed, (MAX_BRANCH_FILES + 5) as u64);
    assert!(stat.truncated);
}

#[test]
fn branches_list_the_current_one_and_stop_at_the_cap() {
    let (current, local, cut) = parse_branches("  main\n* feature/x\n  other\n\n");
    assert_eq!(current.as_deref(), Some("feature/x"));
    assert_eq!(local, ["main", "feature/x", "other"]);
    assert!(!cut);
    let many: String = (0..MAX_BRANCHES + 3).map(|n| format!("  b{n}\n")).collect();
    let (_, local, cut) = parse_branches(&many);
    assert_eq!(local.len(), MAX_BRANCHES);
    assert!(cut);
}

#[test]
fn only_plain_branch_names_pass_for_a_base() {
    for good in [
        "main",
        "origin/main",
        "feature/a-b_c.d",
        "release+1",
        "HEAD",
        "v1.2@x",
    ] {
        assert_eq!(clean_ref(good), Ok(good), "{good}");
    }
    for bad in [
        "",
        "-x",
        "--output=/tmp/x",
        "/abs",
        "a/",
        "a.",
        "a..b",
        "a b",
        "a\nb",
        "a\0b",
        "a~1",
        "a^",
        "a:b",
        "a?b",
        "a*",
        "a[0]",
        "@{u}",
        "a\\b",
        "$(x)",
        "a;b",
    ] {
        assert!(clean_ref(bad).is_err(), "{bad:?}");
    }
    assert!(clean_ref(&"a".repeat(201)).is_err());
}
