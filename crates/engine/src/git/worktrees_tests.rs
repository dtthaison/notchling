use super::worktrees::{parse_worktrees, WorktreeEntry, MAX_WORKTREES};

const A: &str = "0823e49772e044ef8d683209cb2c931dec786b96";
const B: &str = "1111111111111111111111111111111111111111";

fn z(attrs: &[&str]) -> String {
    let mut text = String::new();
    for attr in attrs {
        text.push_str(attr);
        text.push('\0');
    }
    text
}

#[test]
fn main_linked_and_detached_entries() {
    let text = z(&[
        "worktree /r",
        &format!("HEAD {A}"),
        "branch refs/heads/main",
        "",
        "worktree /w1",
        &format!("HEAD {B}"),
        "branch refs/heads/feat",
        "",
        "worktree /w2",
        &format!("HEAD {A}"),
        "detached",
        "",
    ]);
    let (entries, truncated) = parse_worktrees(&text);
    assert!(!truncated);
    assert_eq!(
        entries,
        vec![
            WorktreeEntry {
                path: "/r".into(),
                head: Some(A.into()),
                branch: Some("main".into()),
                ..Default::default()
            },
            WorktreeEntry {
                path: "/w1".into(),
                head: Some(B.into()),
                branch: Some("feat".into()),
                ..Default::default()
            },
            WorktreeEntry {
                path: "/w2".into(),
                head: Some(A.into()),
                detached: true,
                ..Default::default()
            },
        ]
    );
}

#[test]
fn bare_main_is_marked() {
    let text = z(&[
        "worktree /r.git",
        "bare",
        "",
        "worktree /w",
        &format!("HEAD {A}"),
        "branch refs/heads/x",
        "",
    ]);
    let (entries, _) = parse_worktrees(&text);
    assert_eq!(entries.len(), 2);
    assert!(entries[0].bare);
    assert_eq!(entries[0].head, None);
    assert_eq!(entries[0].branch, None);
    assert!(!entries[1].bare);
}

#[test]
fn locked_and_prunable_with_and_without_reasons() {
    let text = z(&[
        "worktree /a",
        &format!("HEAD {A}"),
        "detached",
        "locked",
        "prunable",
        "",
        "worktree /b",
        &format!("HEAD {A}"),
        "detached",
        "locked on a usb stick",
        "prunable gitdir file points to non-existent location",
        "",
        "worktree /c",
        &format!("HEAD {A}"),
        "detached",
        "",
    ]);
    let (entries, _) = parse_worktrees(&text);
    assert!(entries[0].locked && entries[0].prunable);
    assert!(entries[1].locked && entries[1].prunable);
    assert!(!entries[2].locked && !entries[2].prunable);
}

#[test]
fn path_with_space_and_newline_is_kept() {
    let text = z(&[
        "worktree /tmp/my dir/a\nb",
        &format!("HEAD {A}"),
        "detached",
        "",
    ]);
    let (entries, _) = parse_worktrees(&text);
    assert_eq!(entries[0].path, "/tmp/my dir/a\nb");
}

#[test]
fn branch_without_refs_heads_prefix() {
    let text = z(&[
        "worktree /a",
        &format!("HEAD {A}"),
        "branch topic",
        "",
        "worktree /b",
        &format!("HEAD {A}"),
        "branch refs/heads/feat/x",
        "",
    ]);
    let (entries, _) = parse_worktrees(&text);
    assert_eq!(entries[0].branch.as_deref(), Some("topic"));
    assert_eq!(entries[1].branch.as_deref(), Some("feat/x"));
}

#[test]
fn fifty_one_entries_cut_at_fifty() {
    let mut attrs = Vec::new();
    for i in 0..51 {
        attrs.push(format!("worktree /w{i}"));
        attrs.push(format!("HEAD {A}"));
        attrs.push("detached".to_string());
        attrs.push(String::new());
    }
    let refs: Vec<&str> = attrs.iter().map(String::as_str).collect();
    let (entries, truncated) = parse_worktrees(&z(&refs));
    assert_eq!(entries.len(), MAX_WORKTREES);
    assert!(truncated);
    assert_eq!(entries[49].path, "/w49");

    let (entries, truncated) = parse_worktrees(&z(&refs[..200]));
    assert_eq!(entries.len(), 50);
    assert!(!truncated);
}

#[test]
fn garbage_attribute_is_ignored() {
    let text = z(&[
        "mystery before",
        "worktree /a",
        "nonsense",
        &format!("HEAD {A}"),
        "branch refs/heads/x",
        "future-attr with value",
        "",
        "",
        "HEAD stray",
    ]);
    let (entries, truncated) = parse_worktrees(&text);
    assert!(!truncated);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].branch.as_deref(), Some("x"));
    assert_eq!(parse_worktrees("").0.len(), 0);
}

#[test]
fn unborn_head_is_none() {
    let text = z(&[
        "worktree /a",
        "HEAD 0000000000000000000000000000000000000000",
        "branch refs/heads/main",
        "",
    ]);
    let (entries, _) = parse_worktrees(&text);
    assert_eq!(entries[0].head, None);
    assert_eq!(entries[0].branch.as_deref(), Some("main"));
}

#[test]
fn entry_without_trailing_separator_is_kept() {
    let text = format!("worktree /a\0HEAD {A}\0detached");
    let (entries, _) = parse_worktrees(&text);
    assert_eq!(entries.len(), 1);
    assert!(entries[0].detached);
}
