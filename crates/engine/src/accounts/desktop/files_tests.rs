use super::*;

struct Root(PathBuf);

impl Root {
    fn new(tag: &str) -> Root {
        let name = format!("nl-desktop-files-{tag}-{}", std::process::id());
        let root = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Claude")).unwrap();
        Root(root)
    }

    fn claude(&self) -> PathBuf {
        self.0.join("Claude")
    }

    fn store(&self) -> PathBuf {
        self.0.join("ours")
    }

    fn sign_out(&self) -> Result<Option<Kept>, Refused> {
        let (claude, store) = (self.claude(), self.store());
        sign_out(&Dirs {
            claude: &claude,
            store: &store,
        })
    }

    fn restore(&self, saved: &Path, account: &str) -> Result<Option<Kept>, Refused> {
        let (claude, store) = (self.claude(), self.store());
        let dirs = Dirs {
            claude: &claude,
            store: &store,
        };
        restore(&dirs, saved, Some(account))
    }

    fn number(&self, n: u64) -> PathBuf {
        let name = format!("{n:012}.cookies");
        self.store().join(UNNAMED_DIR).join(name)
    }

    fn config(&self) -> Value {
        serde_json::from_slice(&std::fs::read(config(&self.claude())).unwrap()).unwrap()
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn keep_unnamed(store: &Path, data: &[u8]) -> Option<PathBuf> {
    let mut fresh = Temps::default();
    let kept = super::keep_unnamed(store, data, &mut fresh);
    fresh.0.clear();
    kept
}

fn account(dir: &Path) -> std::io::Result<Option<String>> {
    found(dir).map(|found| found.account())
}

fn plan(dir: &Path, account: Option<&str>) -> Result<Option<Config>, Refused> {
    let found = found(dir).map_err(|_| Refused::Failed)?;
    found.plan(dir, account)
}

#[cfg(unix)]
fn mode_of(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, Permissions::from_mode(mode)).unwrap();
}

#[test]
fn the_account_is_what_config_json_says() {
    let root = Root::new("active");
    let dir = root.claude();
    assert_eq!(account(&dir).unwrap(), None);
    std::fs::write(config(&dir), r#"{"lastKnownAccountUuid":"u-1"}"#).unwrap();
    assert_eq!(account(&dir).unwrap().as_deref(), Some("u-1"));
    std::fs::write(config(&dir), r#"{"theme":"dark"}"#).unwrap();
    assert_eq!(account(&dir).unwrap(), None);
    for bad in ["not json", "", r#"{"lastKnownAccountUuid":"#, "[]", "null"] {
        std::fs::write(config(&dir), bad).unwrap();
        assert!(account(&dir).is_err(), "{bad:?}");
    }
}

#[test]
fn a_save_is_an_exact_private_copy_made_once() {
    let root = Root::new("save");
    let mine = saved(&root.store(), "u-1");
    assert!(save(&mine, b"cookie-store-1"));
    assert_eq!(std::fs::read(&mine).unwrap(), b"cookie-store-1");
    #[cfg(unix)]
    assert_eq!(mode_of(&mine), 0o600);
    assert!(save(&mine, b"cookie-store-2"));
    assert_eq!(std::fs::read(&mine).unwrap(), b"cookie-store-2");
    assert!(is_copy(&mine) && !is_copy(&root.store()));
}

#[test]
fn a_restore_keeps_the_login_it_replaces_backs_up_once_and_marks_the_account() {
    let root = Root::new("restore");
    let dir = root.claude();
    std::fs::write(cookies(&dir), b"live-store").unwrap();
    std::fs::write(
        config(&dir),
        r#"{"theme":"dark","lastKnownAccountUuid":"u-old","bootSideSessions":{"accountUuid":"u-old"}}"#,
    )
    .unwrap();
    let from = root.0.join("saved.cookies");
    std::fs::write(&from, b"saved-store").unwrap();
    let kept = root.restore(&from, "u-2").unwrap().unwrap();
    assert_eq!(kept.account.as_deref(), Some("u-old"));
    assert_eq!(kept.copy, saved(&root.store(), "u-old"));
    assert_eq!(std::fs::read(&kept.copy).unwrap(), b"live-store");
    assert_eq!(std::fs::read(cookies(&dir)).unwrap(), b"saved-store");
    assert_eq!(std::fs::read(backup(&dir)).unwrap(), b"live-store");
    let config = root.config();
    assert_eq!(config["lastKnownAccountUuid"], "u-2");
    assert_eq!(
        config["bootSideSessions"],
        json!({"accountUuid": "u-2", "supported": true})
    );
    assert_eq!(config["theme"], "dark", "other keys stay");
    root.restore(&from, "u-2").unwrap();
    assert_eq!(std::fs::read(backup(&dir)).unwrap(), b"live-store");
    let left: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn a_restore_from_a_missing_or_empty_session_changes_nothing() {
    let root = Root::new("restore-bad");
    let dir = root.claude();
    std::fs::write(cookies(&dir), b"live-store").unwrap();
    assert_eq!(root.restore(&dir.join("nope"), "u"), Err(Refused::Failed));
    std::fs::write(dir.join("empty"), b"").unwrap();
    assert_eq!(root.restore(&dir.join("empty"), "u"), Err(Refused::Failed));
    assert_eq!(std::fs::read(cookies(&dir)).unwrap(), b"live-store");
    assert!(!backup(&dir).exists() && !config(&dir).exists());
}

#[test]
fn sign_out_removes_the_session_and_the_markers() {
    let root = Root::new("signout");
    let dir = root.claude();
    std::fs::write(cookies(&dir), b"live-store").unwrap();
    std::fs::write(dir.join("Cookies-wal"), b"").unwrap();
    std::fs::write(
        config(&dir),
        r#"{"keep":1,"lastKnownAccountUuid":"u","bootSideSessions":{}}"#,
    )
    .unwrap();
    let kept = root.sign_out().unwrap().unwrap();
    assert_eq!(kept.copy, saved(&root.store(), "u"));
    assert!(!cookies(&dir).exists() && !dir.join("Cookies-wal").exists());
    assert_eq!(std::fs::read(backup(&dir)).unwrap(), b"live-store");
    assert_eq!(root.config(), json!({"keep": 1}));
    assert_eq!(root.sign_out(), Ok(None), "nothing left to keep");
}

#[test]
fn a_session_is_settled_only_with_no_write_ahead_content() {
    let root = Root::new("settled");
    let dir = root.claude();
    assert!(settled(&dir));
    for name in WRITE_AHEAD {
        std::fs::write(dir.join(name), b"").unwrap();
    }
    std::fs::write(dir.join(WAL_INDEX), b"index").unwrap();
    assert!(settled(&dir));
    for name in WRITE_AHEAD {
        std::fs::write(dir.join(name), b"x").unwrap();
        assert!(!settled(&dir), "{name}");
        assert_eq!(root.sign_out(), Err(Refused::Unsettled), "{name}");
        std::fs::remove_file(dir.join(name)).unwrap();
        std::fs::create_dir(dir.join(name)).unwrap();
        assert!(!settled(&dir), "{name} is no file");
        std::fs::remove_dir(dir.join(name)).unwrap();
    }
    assert!(settled(&dir));
}

#[cfg(unix)]
#[test]
fn config_json_keeps_its_mode() {
    let root = Root::new("mode");
    let path = config(&root.claude());
    std::fs::write(&path, r#"{"a":1,"lastKnownAccountUuid":"u"}"#).unwrap();
    set_mode(&path, 0o640);
    root.sign_out().unwrap();
    assert_eq!(root.config(), json!({"a": 1}));
    assert_eq!(mode_of(&path), 0o640);
}

#[test]
fn a_config_json_that_already_says_it_is_not_written_again() {
    let root = Root::new("config-same");
    let dir = root.claude();
    assert!(plan(&dir, None).unwrap().is_none(), "none to write");
    assert!(plan(&dir, Some("u")).unwrap().is_some());
    std::fs::write(config(&dir), "{ \"a\": 1 }").unwrap();
    assert!(plan(&dir, None).unwrap().is_none());
    root.sign_out().unwrap();
    assert_eq!(std::fs::read(config(&dir)).unwrap(), b"{ \"a\": 1 }");
    for bad in ["not json", "[]", "null"] {
        std::fs::write(config(&dir), bad).unwrap();
        assert!(plan(&dir, None).is_err(), "{bad}");
    }
}

#[test]
fn a_change_whose_last_step_fails_puts_config_json_back_as_it_was() {
    let root = Root::new("undo");
    let dir = root.claude();
    let found = r#"{ "keep": 1, "lastKnownAccountUuid": "u" }"#;
    std::fs::write(config(&dir), found).unwrap();
    #[cfg(unix)]
    set_mode(&config(&dir), 0o640);
    let mut temps = Temps::default();
    let plan = plan(&dir, None).unwrap().unwrap();
    let swap = plan.stage(&dir, &mut temps).unwrap();
    let failed = commit(Some(&swap), || {
        assert_eq!(root.config(), json!({"keep": 1}), "config.json goes first");
        Err(std::io::ErrorKind::Other.into())
    });
    assert_eq!(failed, Err(Refused::Failed));
    assert_eq!(std::fs::read_to_string(config(&dir)).unwrap(), found);
    #[cfg(unix)]
    assert_eq!(mode_of(&config(&dir)), 0o640);
    drop(temps);
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        1,
        "only config.json"
    );
}

#[test]
fn a_config_json_made_for_a_change_that_fails_is_removed_again() {
    let root = Root::new("undo-new");
    let dir = root.claude();
    let mut temps = Temps::default();
    let plan = plan(&dir, Some("u-2")).unwrap().unwrap();
    let swap = plan.stage(&dir, &mut temps).unwrap();
    let failed = commit(Some(&swap), || {
        assert_eq!(root.config()["lastKnownAccountUuid"], "u-2");
        Err(std::io::ErrorKind::Other.into())
    });
    assert_eq!(failed, Err(Refused::Failed));
    drop(temps);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
}

#[test]
fn a_read_counts_only_when_it_comes_out_the_same_twice_with_nothing_being_written() {
    let count = std::cell::Cell::new(0usize);
    let reads = &count;
    let read = |values: [u8; 2]| {
        reads.set(0);
        move || {
            reads.set(reads.get() + 1);
            Ok::<u8, std::io::Error>(values[reads.get() - 1])
        }
    };
    assert_eq!(steady(|| true, read([7, 7])).ok(), Some(7));
    assert!(steady(|| true, read([7, 8])).is_err(), "it changed");
    assert!(steady(|| false, read([7, 7])).is_err(), "a write under way");
    assert_eq!(reads.get(), 0, "never read while one is");
    let after = || reads.get() < 2;
    assert!(steady(after, read([7, 7])).is_err(), "one began meanwhile");
    let gone = || Err::<u8, std::io::Error>(std::io::ErrorKind::NotFound.into());
    assert!(steady(|| true, gone).is_err());
}

#[test]
fn a_login_is_read_only_when_it_is_settled() {
    let root = Root::new("session");
    let dir = root.claude();
    assert!(session(&dir).unwrap().is_none());
    std::fs::write(cookies(&dir), b"live-store").unwrap();
    std::fs::write(config(&dir), r#"{"lastKnownAccountUuid":"u-1"}"#).unwrap();
    let whole = session(&dir).unwrap().unwrap();
    assert_eq!(whole.account.as_deref(), Some("u-1"));
    assert_eq!(whole.cookies, b"live-store");
    for name in WRITE_AHEAD {
        std::fs::write(dir.join(name), b"x").unwrap();
        assert!(session(&dir).is_err(), "{name}");
        assert_eq!(live(&dir), None, "{name}");
        std::fs::write(dir.join(name), b"").unwrap();
        assert!(live(&dir).is_some(), "{name}");
    }
}

#[test]
fn an_unnamed_copy_kept_again_stays_until_a_change_moves_it_to_the_newest_number() {
    let root = Root::new("unnamed");
    let store = root.store();
    let number = |n: u64| root.number(n);
    assert_eq!(keep_unnamed(&store, b"a"), Some(number(1)));
    assert_eq!(keep_unnamed(&store, b"b"), Some(number(2)));
    assert_eq!(keep_unnamed(&store, b"b"), Some(number(2)), "the newest");
    assert_eq!(keep_unnamed(&store, b"a"), Some(number(1)), "where it is");
    let restored = number(1);
    assert_eq!(newest(&store, number(1), Some(&restored)), number(1));
    assert_eq!(newest(&store, number(2), None), number(2), "the newest");
    assert_eq!(newest(&store, number(1), None), number(3));
    assert!(!number(1).exists());
    assert_eq!(std::fs::read(number(3)).unwrap(), b"a");
    std::fs::create_dir(number(4)).unwrap();
    assert_eq!(newest(&store, number(3), None), number(5), "past any entry");
    assert!(!number(3).exists());

    let last = store
        .join(UNNAMED_DIR)
        .join(format!("{}.cookies", u64::MAX));
    std::fs::write(&last, b"z").unwrap();
    assert_eq!(newest(&store, number(5), None), number(5), "nowhere newer");
    assert_eq!(keep_unnamed(&store, b"z"), Some(last.clone()));
    assert_eq!(keep_unnamed(&store, b"new"), None);
    for n in 10..22 {
        std::fs::write(number(n), format!("copy-{n}")).unwrap();
    }
    prune_unnamed(&store, &[number(5).as_path(), number(10).as_path()]);
    assert!(number(5).exists() && number(10).exists(), "in use");
    assert!(!number(2).exists() && !number(11).exists() && !number(14).exists());
    assert!(number(15).exists() && last.exists(), "ten stay");
}

#[test]
fn only_plain_ids_are_file_names() {
    assert!(valid_uuid("3f2a-9c"));
    for bad in ["", "../x", "a/b", "a b", "a.cookies", &"x".repeat(65)] {
        assert!(!valid_uuid(bad), "{bad}");
    }
}

#[test]
fn a_login_whose_config_json_will_not_read_is_not_known_yet() {
    let root = Root::new("session-config");
    let dir = root.claude();
    std::fs::write(cookies(&dir), b"live-store").unwrap();
    assert_eq!(session(&dir).unwrap().unwrap().account, None, "none");
    std::fs::write(config(&dir), r#"{"theme":"dark"}"#).unwrap();
    assert_eq!(session(&dir).unwrap().unwrap().account, None, "no id");
    for bad in ["", r#"{"lastKnownAccountUuid":"#, "not json", "[]"] {
        std::fs::write(config(&dir), bad).unwrap();
        assert!(session(&dir).is_err(), "{bad:?}");
        assert_eq!(live(&dir), None, "{bad:?}");
    }
    std::fs::remove_file(config(&dir)).unwrap();
    std::fs::create_dir(config(&dir)).unwrap();
    assert!(session(&dir).is_err(), "a folder");
    std::fs::remove_dir(config(&dir)).unwrap();
    #[cfg(unix)]
    {
        let real = root.0.join("real.json");
        std::fs::write(&real, r#"{"lastKnownAccountUuid":"u-1"}"#).unwrap();
        std::os::unix::fs::symlink(&real, config(&dir)).unwrap();
        assert!(session(&dir).is_err(), "a link");
    }
}

#[test]
fn a_change_over_a_config_json_that_will_not_read_keeps_nothing() {
    for (n, bad) in ["not json", "", "[]"].into_iter().enumerate() {
        let root = Root::new(&format!("bad-config-{n}"));
        let dir = root.claude();
        std::fs::write(cookies(&dir), b"live-store").unwrap();
        std::fs::write(config(&dir), bad).unwrap();
        let from = root.0.join("saved.cookies");
        std::fs::write(&from, b"saved-store").unwrap();
        assert_eq!(root.sign_out(), Err(Refused::Failed), "{bad:?}");
        assert_eq!(root.restore(&from, "u-2"), Err(Refused::Failed), "{bad:?}");
        assert!(!root.store().exists(), "{bad:?}");
        assert_eq!(std::fs::read(cookies(&dir)).unwrap(), b"live-store");
    }
}

#[test]
fn a_write_that_runs_between_the_two_reads_never_counts() {
    let reads = std::cell::Cell::new(0usize);
    let read = || {
        reads.set(reads.get() + 1);
        Ok::<u8, std::io::Error>(7)
    };
    let between = || reads.get() != 1;
    assert!(steady(between, read).is_err());
    assert_eq!(reads.get(), 1, "the second read waits for the next try");
}

#[test]
fn a_restore_leaves_the_copy_it_restores_where_it_is() {
    let root = Root::new("saved-stays");
    let (claude, store) = (root.claude(), root.store());
    assert_eq!(keep_unnamed(&store, b"u1"), Some(root.number(1)));
    assert_eq!(keep_unnamed(&store, b"u2"), Some(root.number(2)));
    std::fs::write(cookies(&claude), b"u1").unwrap();
    let dirs = Dirs {
        claude: &claude,
        store: &store,
    };
    let blocked = temp(&claude, "Cookies", "notchling.tmp");
    std::fs::create_dir_all(blocked.join("inner")).unwrap();
    assert_eq!(restore(&dirs, &root.number(1), None), Err(Refused::Failed));
    assert_eq!(std::fs::read(root.number(1)).unwrap(), b"u1", "it failed");
    std::fs::remove_dir_all(&blocked).unwrap();
    let spelled = claude.join("..").join("ours").join(UNNAMED_DIR);
    for saved in [root.number(1), spelled.join("000000000001.cookies")] {
        let kept = restore(&dirs, &saved, None).unwrap().unwrap();
        assert_eq!(kept.copy, root.number(1), "kept where it was");
        assert_eq!(std::fs::read(cookies(&claude)).unwrap(), b"u1");
        assert_eq!(std::fs::read(root.number(1)).unwrap(), b"u1");
    }
    assert!(root.number(2).exists() && !root.number(3).exists());
}

#[test]
fn a_change_that_fails_moves_no_unnamed_copy_and_one_that_works_moves_the_one_it_kept() {
    let root = Root::new("moves-last");
    let (claude, store) = (root.claude(), root.store());
    assert_eq!(keep_unnamed(&store, b"u1"), Some(root.number(1)));
    assert_eq!(keep_unnamed(&store, b"u2"), Some(root.number(2)));
    std::fs::write(cookies(&claude), b"u1").unwrap();
    let from = root.0.join("saved.cookies");
    std::fs::write(&from, b"saved-store").unwrap();
    let blocked = temp(&claude, "Cookies", "notchling.tmp");
    std::fs::create_dir_all(blocked.join("inner")).unwrap();
    assert_eq!(root.restore(&from, "u-2"), Err(Refused::Failed));
    assert!(root.number(1).exists() && !root.number(3).exists());
    std::fs::remove_dir_all(&blocked).unwrap();
    let kept = root.restore(&from, "u-2").unwrap().unwrap();
    assert_eq!(kept.copy, root.number(3), "the newest once it worked");
    assert_eq!(std::fs::read(root.number(3)).unwrap(), b"u1");
    assert!(!root.number(1).exists() && root.number(2).exists());
}

#[test]
fn a_change_is_refused_when_the_login_is_no_longer_the_one_it_read() {
    let names = |id: &str| {
        format!(
            r#"{{"lastKnownAccountUuid":"{id}","bootSideSessions":{{"accountUuid":"{id}","supported":true}}}}"#
        )
    };
    for (n, (restores, wrote)) in [(true, "config"), (true, "cookies"), (false, "config")]
        .into_iter()
        .enumerate()
    {
        let root = Root::new(&format!("moved-on-{n}"));
        let (claude, store) = (root.claude(), root.store());
        let dirs = Dirs {
            claude: &claude,
            store: &store,
        };
        std::fs::write(config(&claude), names("u-a")).unwrap();
        std::fs::write(cookies(&claude), b"live-a").unwrap();
        let from = root.0.join("saved.cookies");
        std::fs::write(&from, b"saved-a").unwrap();
        let to = restores.then_some((from.as_path(), Some("u-a")));
        let ready = ready(&dirs, to).unwrap();
        std::fs::write(dirs.claude.join("Cookies-wal"), b"").unwrap();
        match wrote {
            "config" => std::fs::write(config(&claude), names("u-b")).unwrap(),
            _ => std::fs::write(cookies(&claude), b"live-b").unwrap(),
        }
        let (config_then, cookies_then) = (root.config(), std::fs::read(cookies(&claude)).unwrap());
        assert_eq!(ready.land(), Err(Refused::Unsettled), "{restores} {wrote}");
        assert_eq!(root.config(), config_then, "{restores} {wrote}");
        assert_eq!(std::fs::read(cookies(&claude)).unwrap(), cookies_then);
        assert!(claude.join("Cookies-wal").exists(), "nothing was removed");
        let left = std::fs::read_dir(&claude).unwrap().flatten();
        let left = left.map(|e| e.file_name().to_string_lossy().into_owned());
        assert_eq!(left.filter(|name| name.ends_with(".tmp")).count(), 0);
    }
}

#[test]
fn a_copy_is_the_file_it_is_however_its_path_is_spelled() {
    let root = Root::new("spelling");
    let (claude, store) = (root.claude(), root.store());
    let dirs = Dirs {
        claude: &claude,
        store: &store,
    };
    std::fs::write(config(&claude), r#"{"lastKnownAccountUuid":"u-1"}"#).unwrap();
    std::fs::write(cookies(&claude), b"new").unwrap();
    assert!(save(&saved(&store, "u-1"), b"old"));
    let spelled = claude.join("..").join("ours");
    restore(&dirs, &spelled.join("u-1.cookies"), Some("u-1")).unwrap();
    assert_eq!(std::fs::read(cookies(&claude)).unwrap(), b"new");
    assert_eq!(std::fs::read(saved(&store, "u-1")).unwrap(), b"new");

    std::fs::write(config(&claude), "{}").unwrap();
    std::fs::create_dir_all(store.join(UNNAMED_DIR)).unwrap();
    for n in 1..=11 {
        std::fs::write(root.number(n), format!("unnamed-{n}")).unwrap();
    }
    let first = spelled.join(UNNAMED_DIR).join("000000000001.cookies");
    let kept = restore(&dirs, &first, None).unwrap().unwrap();
    assert_eq!(kept.copy, root.number(12));
    assert_eq!(std::fs::read(cookies(&claude)).unwrap(), b"unnamed-1");
    assert!(root.number(1).exists(), "the copy restored is never cut");
    assert!(!root.number(2).exists() && !root.number(3).exists());
    assert!(root.number(4).exists());
}

#[test]
fn a_write_between_the_two_reads_counts_even_when_the_bytes_come_out_the_same() {
    let root = Root::new("stamp");
    let dir = root.claude();
    std::fs::write(cookies(&dir), b"live-store").unwrap();
    let file = OpenOptions::new().write(true).open(cookies(&dir)).unwrap();
    let checks = std::cell::Cell::new(0usize);
    let lands_between = || {
        checks.set(checks.get() + 1);
        if checks.get() == 2 {
            let then = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1);
            file.set_modified(then).unwrap();
        }
        true
    };
    assert!(steady(lands_between, || found(&dir)).is_err());
    assert_eq!(checks.get(), 3, "settled throughout, yet the reads differ");
    let whole = steady(|| true, || found(&dir)).ok().unwrap();
    let whole = whole.cookies.map(|(cookies, _)| cookies);
    assert_eq!(whole.unwrap(), b"live-store");
}
