use super::file::*;
use super::testutil::*;
use std::fs;

fn settings(dir: &Scratch, text: &str) -> std::path::PathBuf {
    let path = dir.path("settings.json");
    fs::write(&path, text).unwrap();
    path
}

fn backups(dir: &Scratch) -> Vec<String> {
    dir.names()
        .into_iter()
        .filter(|n| n.contains(BACKUP_SUFFIX))
        .collect()
}

#[cfg(unix)]
fn mode(path: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn planning_reads_and_writes_nothing() {
    let dir = Scratch::new("plan");
    let path = settings(&dir, USER_HOOKS);
    let p = plan(&path, &install_all()).unwrap();
    assert!(p.hooks.changed);
    assert_eq!(p.state, State::Present);
    assert_eq!(fs::read_to_string(&path).unwrap(), USER_HOOKS);
    assert_eq!(dir.names(), vec!["settings.json"]);
}

#[test]
fn applying_backs_up_first_then_replaces_the_file() {
    let dir = Scratch::new("apply");
    let path = settings(&dir, USER_HOOKS);
    let done = apply(&path, &install_all(), 1_700_000_000_123).unwrap();
    assert!(done.wrote);
    let backup = done.backup.unwrap();
    assert_eq!(
        backup.file_name().unwrap().to_string_lossy(),
        "settings.json.notchling-backup-1700000000123"
    );
    assert_eq!(fs::read_to_string(&backup).unwrap(), USER_HOOKS);
    let new = fs::read_to_string(&path).unwrap();
    assert_eq!(new, done.plan.hooks.text);
    assert_ne!(new, USER_HOOKS);
    #[cfg(unix)]
    assert_eq!(mode(&backup), 0o600);
    assert_eq!(dir.names().len(), 2, "{:?}", dir.names());

    let again = apply(&path, &install_all(), 1_700_000_000_999).unwrap();
    assert!(!again.wrote && again.backup.is_none());
    assert_eq!(backups(&dir).len(), 1);

    let removed = apply(&path, &remove_all(), 1_700_000_001_000).unwrap();
    assert!(removed.wrote);
    assert_eq!(fs::read_to_string(&path).unwrap(), USER_HOOKS);
    assert_eq!(backups(&dir).len(), 2);
}

#[cfg(unix)]
#[test]
fn the_files_mode_is_kept() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new("mode");
    let path = settings(&dir, NO_HOOKS);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    apply(&path, &install_all(), 1).unwrap();
    assert_eq!(mode(&path), 0o640);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o664)).unwrap();
    apply(&path, &remove_all(), 2).unwrap();
    assert_eq!(mode(&path), 0o664);
}

#[test]
fn a_backup_never_replaces_an_older_one() {
    let dir = Scratch::new("same-stamp");
    let path = settings(&dir, NO_HOOKS);
    apply(&path, &install_all(), 5).unwrap();
    apply(&path, &remove_all(), 5).unwrap();
    let names = backups(&dir);
    assert_eq!(names.len(), 2, "{names:?}");
    let first = fs::read_to_string(dir.path("settings.json.notchling-backup-5")).unwrap();
    assert_eq!(first, NO_HOOKS);
}

#[test]
fn only_the_newest_backups_stay() {
    let dir = Scratch::new("prune");
    let path = settings(&dir, NO_HOOKS);
    for stamp in 1..=KEEP_BACKUPS as u64 + 5 {
        fs::write(
            dir.path(&format!("settings.json.notchling-backup-{stamp}")),
            "old",
        )
        .unwrap();
    }
    fs::write(
        dir.path("settings.json.notchling-backup-mine"),
        "not a stamp",
    )
    .unwrap();
    fs::write(dir.path("other.json.notchling-backup-1"), "another file's").unwrap();
    apply(&path, &install_all(), 1000).unwrap();
    let names = backups(&dir);
    let stamped: Vec<_> = names
        .iter()
        .filter(|n| n.starts_with("settings.json"))
        .collect();
    assert_eq!(stamped.len(), KEEP_BACKUPS + 1, "{names:?}");
    assert!(dir.path("settings.json.notchling-backup-1000").exists());
    assert!(!dir.path("settings.json.notchling-backup-1").exists());
    assert!(dir.path("settings.json.notchling-backup-mine").exists());
    assert!(dir.path("other.json.notchling-backup-1").exists());
}

#[test]
fn a_missing_file_is_made_when_its_folder_is_there() {
    let dir = Scratch::new("missing");
    let path = dir.path("settings.json");
    let p = plan(&path, &install_all()).unwrap();
    assert_eq!(p.state, State::Missing);
    assert!(!path.exists());
    let done = apply(&path, &install_all(), 7).unwrap();
    assert!(done.wrote && done.backup.is_none());
    assert_eq!(dir.names(), vec!["settings.json"]);
    assert!(value(&fs::read_to_string(&path).unwrap())["hooks"]["PermissionRequest"].is_array());
    #[cfg(unix)]
    assert_eq!(mode(&path), 0o600);

    let gone = dir.path("no-such-folder/settings.json");
    assert!(matches!(
        apply(&gone, &install_all(), 7),
        Err(FileError::NoFolder(_))
    ));
    assert!(!dir.path("no-such-folder").exists());
    let nope = dir.path("x/settings.json");
    assert!(matches!(
        plan(&nope, &remove_all()),
        Err(FileError::NoFolder(_))
    ));
    let none = dir.path("none.json");
    assert!(!apply(&none, &remove_all(), 1).unwrap().wrote);
    assert!(!none.exists());
}

#[test]
fn an_empty_file_is_backed_up_and_filled() {
    let dir = Scratch::new("empty");
    let path = settings(&dir, "");
    let p = plan(&path, &install_all()).unwrap();
    assert_eq!(p.state, State::Empty);
    let done = apply(&path, &install_all(), 3).unwrap();
    assert!(done.wrote);
    assert_eq!(fs::read_to_string(done.backup.unwrap()).unwrap(), "");
    assert!(value(&fs::read_to_string(&path).unwrap())["hooks"].is_object());
}

#[test]
fn a_broken_or_odd_file_is_refused_and_nothing_at_all_changes() {
    let cases: [(&str, Vec<u8>); 7] = [
        ("truncated", b"{\"model\": \"opus\", \"hooks\": {".to_vec()),
        ("array", b"[1, 2]".to_vec()),
        ("string", b"\"hello\"".to_vec()),
        ("comments only", b"// nothing here\n".to_vec()),
        ("not utf-8", vec![b'{', 0xff, 0xfe, b'}']),
        ("nul", b"{\"a\": \"\0\"}".to_vec()),
        ("two values", b"{} {}".to_vec()),
    ];
    for (name, bytes) in cases {
        let dir = Scratch::new("refuse");
        let path = dir.path("settings.json");
        fs::write(&path, &bytes).unwrap();
        for wanted in [install_all(), remove_all()] {
            let err = apply(&path, &wanted, 9).unwrap_err();
            assert!(matches!(err, FileError::Refused(_)), "{name}: {err}");
            assert_eq!(fs::read(&path).unwrap(), bytes, "{name}");
            assert_eq!(dir.names(), vec!["settings.json"], "{name}");
        }
        assert!(plan(&path, &install_all()).is_err(), "{name}");
    }
}

#[test]
fn a_file_that_is_too_big_or_not_a_file_is_refused() {
    let dir = Scratch::new("big");
    let path = dir.path("settings.json");
    let mut big = String::from("{\"a\": \"");
    big.push_str(&"x".repeat(MAX_BYTES as usize));
    big.push_str("\"}");
    fs::write(&path, &big).unwrap();
    assert!(matches!(
        apply(&path, &install_all(), 1),
        Err(FileError::Refused(_))
    ));
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(matches!(
        apply(&path, &install_all(), 1),
        Err(FileError::Refused(_))
    ));
}

#[cfg(unix)]
#[test]
fn a_read_only_file_is_refused_unchanged_and_without_a_backup() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new("readonly");
    let path = settings(&dir, USER_HOOKS);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
    let p = plan(&path, &install_all()).unwrap();
    assert!(p.read_only && p.hooks.changed);
    let err = apply(&path, &install_all(), 1).unwrap_err();
    assert!(
        matches!(&err, FileError::Refused(m) if m.contains("read-only")),
        "{err}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), USER_HOOKS);
    assert_eq!(dir.names(), vec!["settings.json"]);
    assert_eq!(mode(&path), 0o444);
    let done = apply(
        &path,
        &wanted(super::hooks::Want::Leave, super::hooks::Want::Leave),
        1,
    )
    .unwrap();
    assert!(!done.wrote);
}

#[cfg(unix)]
#[test]
fn a_symlinked_settings_file_is_written_through_and_stays_a_link() {
    use std::os::unix::fs::symlink;
    let dir = Scratch::new("link");
    fs::create_dir(dir.path("dotfiles")).unwrap();
    fs::create_dir(dir.path("claude")).unwrap();
    let real = dir.path("dotfiles/settings.json");
    fs::write(&real, USER_HOOKS).unwrap();
    let link = dir.path("claude/settings.json");
    symlink(&real, &link).unwrap();

    let done = apply(&link, &install_all(), 11).unwrap();
    assert!(done.wrote);
    assert!(fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_link(&link).unwrap(), real);
    assert_eq!(fs::read_to_string(&real).unwrap(), done.plan.hooks.text);
    let backup = done.backup.unwrap();
    assert_eq!(backup.parent().unwrap(), dir.path("claude"));
    assert_eq!(fs::read_to_string(&backup).unwrap(), USER_HOOKS);
    assert_eq!(fs::read_dir(dir.path("dotfiles")).unwrap().count(), 1);
    assert_eq!(fs::read_dir(dir.path("claude")).unwrap().count(), 2);

    let two = dir.path("claude/two.json");
    symlink("settings.json", &two).unwrap();
    assert!(apply(&two, &remove_all(), 12).unwrap().wrote);
    assert_eq!(fs::read_to_string(&real).unwrap(), USER_HOOKS);
}

#[cfg(unix)]
#[test]
fn a_link_to_nothing_or_to_a_loop_is_refused_and_creates_nothing() {
    use std::os::unix::fs::symlink;
    let dir = Scratch::new("dangling");
    let link = dir.path("settings.json");
    let target = dir.path("not-there.json");
    symlink(&target, &link).unwrap();
    let err = apply(&link, &install_all(), 1).unwrap_err();
    assert!(matches!(err, FileError::Refused(_)), "{err}");
    assert!(!target.exists());
    assert_eq!(dir.names(), vec!["settings.json"]);

    let a = dir.path("a.json");
    let b = dir.path("b.json");
    symlink(&b, &a).unwrap();
    symlink(&a, &b).unwrap();
    assert!(apply(&a, &install_all(), 1).is_err());
}

#[test]
fn a_file_changed_after_it_was_planned_from_is_not_overwritten() {
    let dir = Scratch::new("changed");
    let path = settings(&dir, USER_HOOKS);
    let loaded = load(&path).unwrap();
    let theirs = USER_HOOKS.replace("opus", "sonnet");
    fs::write(&path, &theirs).unwrap();
    let err = write_checked(&loaded, b"{}").unwrap_err();
    assert!(matches!(err, FileError::Changed), "{err}");
    assert_eq!(fs::read_to_string(&path).unwrap(), theirs);
    assert_eq!(dir.names(), vec!["settings.json"]);

    let gone = dir.path("later.json");
    let missing = load(&gone).unwrap();
    fs::write(&gone, "{\"theirs\": true}").unwrap();
    assert!(matches!(
        write_checked(&missing, b"{}"),
        Err(FileError::Changed)
    ));
    assert_eq!(fs::read_to_string(&gone).unwrap(), "{\"theirs\": true}");
}

#[test]
fn refusal_messages_say_nothing_of_the_files_content() {
    let secret = "ghp_SECRETTOKENVALUE";
    let dir = Scratch::new("quiet");
    let path = settings(&dir, &format!("{{\"env\": {{\"T\": \"{secret}\"}} oops}}"));
    let err = apply(&path, &install_all(), 1).unwrap_err().to_string();
    assert!(!err.contains(secret), "{err}");
    assert!(err.contains("not valid JSON"), "{err}");
}
