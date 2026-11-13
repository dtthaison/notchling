use super::*;
use crate::delegate::test_support::scratch;

const CLAUDE: &str = r#"{
  "numStartups": 3,
  "projects": {
    "/Users/me/code/app": {"hasTrustDialogAccepted": true, "allowedTools": []},
    "/Users/me/code/other": {"hasTrustDialogAccepted": false},
    "/Users/me/code/asked": {"allowedTools": []}
  }
}"#;

#[test]
fn claude_trusts_a_folder_it_accepted_and_everything_under_it() {
    let yes = |p: &str| claude_trusts(CLAUDE, Path::new(p));
    assert!(yes("/Users/me/code/app"));
    assert!(yes("/Users/me/code/app/.claude/worktrees/feature"));
    assert!(yes("/Users/me/code/app/crates/engine"));
    assert!(
        !yes("/Users/me/code"),
        "a parent isn't trusted by its child"
    );
    assert!(!yes("/Users/me/code/other"), "declined");
    assert!(!yes("/Users/me/code/asked"), "no answer");
    assert!(
        !yes("/Users/me/code/app2"),
        "a name that only starts the same"
    );
    assert!(!yes("/tmp/x"));
}

#[test]
fn a_file_that_is_not_claude_state_trusts_nothing() {
    for text in [
        "",
        "not json",
        "[]",
        r#"{"projects": []}"#,
        r#"{"projects": {"/a": 3}}"#,
    ] {
        assert!(!claude_trusts(text, Path::new("/a")), "{text:?}");
    }
}

#[test]
fn merging_adds_one_entry_and_keeps_everything_else() {
    let before = r#"{"theme": "dark", "nested": {"a": [1, 2]}, "trustedWorkspaces": ["/old/one", "/old/two"]}"#;
    let (text, added) = merged(before, &["/new/folder".to_string()]).expect("merges");
    assert_eq!(added, ["/new/folder"]);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["theme"], "dark");
    assert_eq!(v["nested"], json!({"a": [1, 2]}));
    assert_eq!(
        v["trustedWorkspaces"],
        json!(["/old/one", "/old/two", "/new/folder"])
    );
    assert!(text.ends_with('\n'));
}

#[test]
fn an_entry_already_there_is_not_added_again_even_with_a_trailing_slash() {
    let before = r#"{"trustedWorkspaces": ["/a/b/"]}"#;
    let (_, added) = merged(
        before,
        &["/a/b".to_string(), "/c".to_string(), "/c".to_string()],
    )
    .unwrap();
    assert_eq!(added, ["/c"]);
    let (_, none) = merged(before, &["/a/b".to_string()]).unwrap();
    assert!(none.is_empty());
}

#[test]
fn a_missing_list_is_made_and_an_empty_file_is_an_empty_object() {
    let (text, added) = merged(r#"{"other": 1}"#, &["/a".to_string()]).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        (v["other"].clone(), v["trustedWorkspaces"].clone()),
        (json!(1), json!(["/a"]))
    );
    assert_eq!(added, ["/a"]);
    assert!(merged("  \n", &["/a".to_string()]).is_some());
}

#[test]
fn a_settings_file_that_is_not_what_agy_writes_is_left_alone() {
    for text in [
        "{ broken",
        "[1,2]",
        "42",
        r#"{"trustedWorkspaces": "/a"}"#,
        r#"{"trustedWorkspaces": {"a": 1}}"#,
    ] {
        assert_eq!(merged(text, &["/x".to_string()]), None, "{text:?}");
    }
}

struct World {
    root: PathBuf,
    files: Files,
    work: PathBuf,
}

fn world(claude: Option<&str>, settings: Option<&str>) -> World {
    let root = scratch("trust");
    let work = root.join("code/app/.claude/worktrees/feature");
    std::fs::create_dir_all(&work).unwrap();
    let canonical = std::fs::canonicalize(root.join("code/app")).unwrap();
    let claude_json = root.join("claude.json");
    if let Some(template) = claude {
        std::fs::write(
            &claude_json,
            template.replace("{app}", &canonical.to_string_lossy()),
        )
        .unwrap();
    }
    let settings_path = root.join("gemini/settings.json");
    std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    if let Some(text) = settings {
        std::fs::write(&settings_path, text).unwrap();
    }
    World {
        root,
        files: Files {
            claude_json,
            settings: settings_path,
        },
        work,
    }
}

const TRUSTED: &str = r#"{"projects": {"{app}": {"hasTrustDialogAccepted": true}}}"#;

fn listed(w: &World) -> Vec<String> {
    let v: Value =
        serde_json::from_str(&std::fs::read_to_string(&w.files.settings).unwrap()).unwrap();
    v["trustedWorkspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_worktree_under_a_folder_claude_trusts_is_added_to_agys_list() {
    let w = world(
        Some(TRUSTED),
        Some(r#"{"trustedWorkspaces": ["/keep/me"], "x": true}"#),
    );
    let result = trust(&w.files, &w.work);
    let real = std::fs::canonicalize(&w.work)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    match result {
        Trusted::Added(added) => assert!(added.contains(&real), "{added:?}"),
        other => panic!("{other:?}"),
    }
    let list = listed(&w);
    assert_eq!(list[0], "/keep/me");
    assert!(list.contains(&real));
    let v: Value =
        serde_json::from_str(&std::fs::read_to_string(&w.files.settings).unwrap()).unwrap();
    assert_eq!(v["x"], true);
    assert_eq!(trust(&w.files, &w.work), Trusted::Already);
    assert_eq!(listed(&w), list);
    let _ = std::fs::remove_dir_all(&w.root);
}

#[test]
fn a_folder_claude_does_not_trust_is_left_to_agy() {
    let w = world(
        Some(r#"{"projects": {"/somewhere/else": {"hasTrustDialogAccepted": true}}}"#),
        Some(r#"{"trustedWorkspaces": []}"#),
    );
    assert_eq!(trust(&w.files, &w.work), Trusted::NotTrustedByClaude);
    assert!(listed(&w).is_empty());
    let none = world(None, Some(r#"{"trustedWorkspaces": []}"#));
    assert_eq!(trust(&none.files, &none.work), Trusted::NotTrustedByClaude);
    let _ = std::fs::remove_dir_all(&w.root);
    let _ = std::fs::remove_dir_all(&none.root);
}

#[test]
fn an_unreadable_or_invalid_settings_file_is_not_touched() {
    let missing = world(Some(TRUSTED), None);
    assert_eq!(trust(&missing.files, &missing.work), Trusted::Skipped);
    assert!(!missing.files.settings.exists(), "no file is made");

    let broken = world(Some(TRUSTED), Some("{ not json"));
    assert_eq!(trust(&broken.files, &broken.work), Trusted::Skipped);
    assert_eq!(
        std::fs::read_to_string(&broken.files.settings).unwrap(),
        "{ not json"
    );
    let _ = std::fs::remove_dir_all(&missing.root);
    let _ = std::fs::remove_dir_all(&broken.root);
}

#[cfg(unix)]
#[test]
fn the_settings_file_keeps_its_mode() {
    use std::os::unix::fs::PermissionsExt;
    let w = world(Some(TRUSTED), Some(r#"{"trustedWorkspaces": []}"#));
    std::fs::set_permissions(&w.files.settings, std::fs::Permissions::from_mode(0o640)).unwrap();
    assert!(matches!(trust(&w.files, &w.work), Trusted::Added(_)));
    let mode = std::fs::metadata(&w.files.settings)
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o640);
    let _ = std::fs::remove_dir_all(&w.root);
}
