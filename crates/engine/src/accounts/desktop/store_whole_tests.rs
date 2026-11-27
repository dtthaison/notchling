use super::files::{backup, cookies};
use super::store::BUNDLE_ID;
use super::store_add_tests::AddRig;
use super::store_keep_tests::{block_config_write, config, kept, live};
use crate::accounts::procs::FakeApps;
use crate::providers::{AccountProvider, AddProgress};
use serde_json::Value;
use std::path::PathBuf;

const WRITE_AHEAD: [&str; 2] = ["Cookies-wal", "Cookies-journal"];

fn unnamed_dir(rig: &AddRig) -> PathBuf {
    rig.root.join("ours/desktop/unnamed")
}

fn sign_in_unnamed(rig: &AddRig, bytes: &[u8]) {
    rig.sign_out();
    std::fs::write(cookies(&rig.claude()), bytes).unwrap();
}

#[test]
fn a_login_desktop_has_not_finished_writing_is_never_changed_or_saved() {
    for name in WRITE_AHEAD {
        let rig = AddRig::new(
            &format!("unsettled-{name}"),
            FakeApps::new().running(BUNDLE_ID),
        );
        rig.sign_in("uuid-a", b"cookies-a");
        rig.keep_copy("uuid-b", b"cookies-b");
        let pending = rig.claude().join(name);
        std::fs::write(&pending, b"pages not yet in Cookies").unwrap();
        let before = config(&rig);
        let refused = rig.store.switch_to("uuid-b");
        assert!(
            refused.is_err_and(|e| e.message().contains("left in place")),
            "{name}"
        );
        assert!(rig.store.begin_add().is_err(), "{name}");
        assert_eq!(
            std::fs::read(&pending).unwrap(),
            b"pages not yet in Cookies",
            "{name}"
        );
        assert_eq!(live(&rig), b"cookies-a", "{name}");
        assert_eq!(config(&rig), before, "{name}");
        assert!(!rig.ours("uuid-a").exists(), "half a login is no copy");
        assert!(!rig.store.state().adding);
        assert_eq!(
            rig.apps.events().last().map(String::as_str),
            Some(format!("open {BUNDLE_ID}").as_str())
        );
    }
}

#[test]
fn a_login_with_no_cookies_file_but_unwritten_changes_is_left_alone_too() {
    let rig = AddRig::new("unsettled-empty", FakeApps::new());
    rig.sign_in("uuid-a", b"");
    rig.keep_copy("uuid-b", b"cookies-b");
    let pending = rig.claude().join("Cookies-wal");
    std::fs::write(&pending, b"every page so far").unwrap();
    assert!(rig.store.switch_to("uuid-b").is_err());
    assert!(rig.store.begin_add().is_err());
    assert_eq!(std::fs::read(&pending).unwrap(), b"every page so far");
    assert_eq!(live(&rig), b"");
}

#[test]
fn a_change_that_cannot_be_made_ready_removes_no_write_ahead_file() {
    let rig = AddRig::new("staged-stuck", FakeApps::new());
    rig.sign_in("uuid-a", b"cookies-a");
    rig.keep_copy("uuid-b", b"cookies-b");
    for name in WRITE_AHEAD {
        std::fs::write(rig.claude().join(name), b"").unwrap();
    }
    std::fs::write(rig.claude().join("Cookies-shm"), b"index").unwrap();
    block_config_write(&rig);
    assert!(rig.store.begin_add().is_err());
    assert!(rig.store.switch_to("uuid-b").is_err());
    for name in WRITE_AHEAD {
        assert!(rig.claude().join(name).exists(), "{name}");
    }
    assert_eq!(
        std::fs::read(rig.claude().join("Cookies-shm")).unwrap(),
        b"index"
    );
    assert_eq!(live(&rig), b"cookies-a");
    assert!(config(&rig).contains("uuid-a"));
}

#[test]
fn a_change_removes_the_empty_write_ahead_files_and_the_old_index() {
    let rig = AddRig::new("settled", FakeApps::new());
    rig.sign_in("uuid-a", b"cookies-a");
    rig.keep_copy("uuid-b", b"cookies-b");
    for name in WRITE_AHEAD {
        std::fs::write(rig.claude().join(name), b"").unwrap();
    }
    std::fs::write(rig.claude().join("Cookies-shm"), b"index").unwrap();
    rig.store.switch_to("uuid-b").unwrap();
    assert_eq!(live(&rig), b"cookies-b");
    for name in WRITE_AHEAD.iter().chain(&["Cookies-shm"]) {
        assert!(!rig.claude().join(name).exists(), "{name}");
    }
}

#[test]
fn a_switch_with_no_config_json_writes_one_that_names_the_account() {
    let rig = AddRig::new("no-config", FakeApps::new());
    std::fs::write(cookies(&rig.claude()), b"cookies-old").unwrap();
    rig.keep_copy("uuid-b", b"cookies-b");
    rig.store.switch_to("uuid-b").unwrap();
    assert_eq!(live(&rig), b"cookies-b");
    let path = rig.claude().join("config.json");
    assert!(path.exists(), "the account is named nowhere");
    let written: Value = serde_json::from_str(&config(&rig)).unwrap();
    assert_eq!(written["lastKnownAccountUuid"], "uuid-b");
    assert_eq!(written["bootSideSessions"]["accountUuid"], "uuid-b");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    rig.store.sync(false);
    assert_eq!(rig.store.state().active_id.as_deref(), Some("uuid-b"));
    assert_eq!(kept(&rig), [b"cookies-old".to_vec()]);
}

#[test]
fn a_switch_to_the_account_in_use_keeps_its_newest_cookies() {
    let rig = AddRig::new("same-account", FakeApps::new());
    rig.sign_in("uuid-a", b"cookies-a-new");
    rig.keep_copy("uuid-a", b"cookies-a-old");
    rig.store.switch_to("uuid-a").unwrap();
    assert_eq!(live(&rig), b"cookies-a-new");
    assert_eq!(std::fs::read(rig.ours("uuid-a")).unwrap(), b"cookies-a-new");
}

#[test]
fn an_account_named_with_no_cookies_is_not_in_use_when_an_add_begins() {
    let rig = AddRig::new("stale-name", FakeApps::new());
    rig.sign_in("uuid-a", b"");
    rig.store.begin_add().unwrap();
    rig.sign_in("uuid-a", b"cookies-a");
    assert_eq!(rig.store.poll_add(), AddProgress::Done);
    assert_eq!(rig.store.added_account(), Some(Some("uuid-a".to_string())));

    let rig = AddRig::new("stale-name-cancel", FakeApps::new());
    rig.keep_copy("uuid-b", b"cookies-b");
    rig.sign_in("uuid-b", b"");
    std::fs::remove_file(cookies(&rig.claude())).unwrap();
    rig.store.begin_add().unwrap();
    rig.store.cancel_add();
    assert!(
        !cookies(&rig.claude()).exists(),
        "Desktop was signed out when the add began"
    );
    assert_eq!(rig.store.abandon_add(), None);
}

#[test]
fn a_cancel_puts_back_a_login_that_named_no_account() {
    let rig = AddRig::new("cancel-no-name", FakeApps::new().running(BUNDLE_ID));
    sign_in_unnamed(&rig, b"cookies-unnamed");
    rig.store.begin_add().unwrap();
    assert!(!cookies(&rig.claude()).exists());
    rig.store.cancel_add();
    assert!(cookies(&rig.claude()).exists(), "left signed out");
    assert_eq!(live(&rig), b"cookies-unnamed");
    assert!(!config(&rig).contains("lastKnownAccountUuid"));
    assert_eq!(rig.store.state().active_id, None);
    assert_eq!(rig.store.abandon_add(), None);

    let rig = AddRig::new("cancel-odd-name", FakeApps::new());
    rig.sign_in("../x", b"cookies-x");
    rig.store.begin_add().unwrap();
    rig.store.cancel_add();
    assert!(cookies(&rig.claude()).exists(), "left signed out");
    assert_eq!(live(&rig), b"cookies-x");
    let written: Value = serde_json::from_str(&config(&rig)).unwrap();
    assert_eq!(written["lastKnownAccountUuid"], "../x");
    assert_eq!(rig.store.state().active_id, None);
}

#[test]
fn a_cancel_puts_the_unnamed_login_back_though_the_kept_list_is_full() {
    let rig = AddRig::new("cancel-full", FakeApps::new());
    let add_over = |bytes: &[u8]| {
        sign_in_unnamed(&rig, bytes);
        rig.store.begin_add().unwrap();
    };
    for n in 0..10 {
        add_over(format!("unnamed-{n}").as_bytes());
        rig.store.abandon_add();
    }
    add_over(b"unnamed-0");
    std::fs::write(cookies(&rig.claude()), b"half-a-login").unwrap();
    rig.store.cancel_add();
    assert_eq!(live(&rig), b"unnamed-0");
    let kept = kept(&rig);
    assert_eq!(kept.len(), 10);
    assert_eq!(kept[9], b"half-a-login");
}

#[test]
fn an_abandon_says_desktop_was_left_signed_out_of_a_login_that_named_no_account() {
    let rig = AddRig::new("abandon-no-name", FakeApps::new());
    sign_in_unnamed(&rig, b"cookies-unnamed");
    rig.store.begin_add().unwrap();
    let note = rig.store.abandon_add().unwrap_or_default();
    assert!(note.contains("left signed out"), "{note:?}");
    assert!(!note.contains("switch to an account"), "{note:?}");
    assert_eq!(rig.store.abandon_add(), None);
    rig.store.cancel_add();
    assert!(!cookies(&rig.claude()).exists(), "the add was over");
}

#[test]
fn a_cancel_that_cannot_put_the_login_back_says_desktop_was_left_signed_out() {
    let rig = AddRig::new("cancel-stuck", FakeApps::new());
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.begin_add().unwrap();
    std::fs::remove_file(rig.ours("uuid-a")).unwrap();
    rig.store.cancel_add();
    assert!(!cookies(&rig.claude()).exists());
    let note = rig.store.abandon_add().unwrap_or_default();
    assert!(note.contains("left signed out"), "{note:?}");
    assert_eq!(rig.store.abandon_add(), None);

    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.begin_add().unwrap();
    rig.store.cancel_add();
    assert_eq!(live(&rig), b"cookies-a");
    assert_eq!(rig.store.abandon_add(), None, "put back");
}

#[test]
fn unnamed_cookies_already_kept_are_kept_once_however_old_the_copy() {
    let rig = AddRig::new("keep-once", FakeApps::new());
    for bytes in [b"unnamed-a", b"unnamed-b", b"unnamed-a", b"unnamed-b"] {
        sign_in_unnamed(&rig, bytes);
        rig.store.begin_add().unwrap();
        rig.store.abandon_add();
    }
    assert_eq!(kept(&rig), [b"unnamed-a".to_vec(), b"unnamed-b".to_vec()]);
}

#[cfg(unix)]
#[test]
fn an_unnamed_folder_that_cannot_be_listed_refuses_the_change() {
    use std::os::unix::fs::PermissionsExt;
    let rig = AddRig::new("keep-unlisted", FakeApps::new());
    let dir = unnamed_dir(&rig);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("000000000005.cookies"), b"unnamed-5").unwrap();
    sign_in_unnamed(&rig, b"cookies-unnamed");
    let mode = |mode| std::fs::set_permissions(&dir, PermissionsExt::from_mode(mode)).unwrap();
    mode(0o300);
    let listable = std::fs::read_dir(&dir).is_ok();
    let began = rig.store.begin_add();
    mode(0o700);
    if listable {
        return;
    }
    assert!(began.is_err());
    assert_eq!(live(&rig), b"cookies-unnamed");
    assert_eq!(kept(&rig), [b"unnamed-5".to_vec()]);
}

#[test]
fn an_entry_that_is_no_file_never_blocks_the_next_unnamed_copy() {
    let rig = AddRig::new("keep-past", FakeApps::new());
    let dir = unnamed_dir(&rig);
    std::fs::create_dir_all(dir.join("000000000001.cookies")).unwrap();
    std::fs::write(dir.join("000000000002.cookies"), b"unnamed-2").unwrap();
    std::fs::create_dir_all(dir.join("000000000003.cookies")).unwrap();
    sign_in_unnamed(&rig, b"cookies-unnamed");
    assert!(rig.store.begin_add().is_ok(), "wedged");
    assert_eq!(
        std::fs::read(dir.join("000000000004.cookies")).unwrap(),
        b"cookies-unnamed"
    );
    assert!(dir.join("000000000001.cookies").is_dir());
    assert!(dir.join("000000000003.cookies").is_dir());
    assert_eq!(
        std::fs::read(dir.join("000000000002.cookies")).unwrap(),
        b"unnamed-2"
    );
}

#[test]
fn an_unnamed_copy_numbered_at_the_limit_refuses_the_change() {
    let rig = AddRig::new("keep-limit", FakeApps::new());
    let dir = unnamed_dir(&rig);
    std::fs::create_dir_all(&dir).unwrap();
    let last = dir.join(format!("{}.cookies", u64::MAX));
    std::fs::write(&last, b"unnamed-last").unwrap();
    sign_in_unnamed(&rig, b"cookies-unnamed");
    let began = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rig.store.begin_add()));
    assert!(began.is_ok_and(|began| began.is_err()));
    assert_eq!(live(&rig), b"cookies-unnamed");
    assert_eq!(kept(&rig), [b"unnamed-last".to_vec()]);
}

#[cfg(unix)]
mod links {
    use super::*;
    use crate::accounts::desktop::store_keep_tests::{make_pipe, unblock_pipe};
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn a_config_json_that_is_a_link_or_a_pipe_is_never_read() {
        for kind in ["link", "pipe"] {
            let rig = AddRig::new(&format!("config-{kind}"), FakeApps::new());
            rig.sign_in("uuid-a", b"cookies-a");
            let path = rig.claude().join("config.json");
            let real = rig.root.join("real.json");
            std::fs::rename(&path, &real).unwrap();
            match kind {
                "link" => symlink(&real, &path).unwrap(),
                _ => make_pipe(&path),
            }
            let store = rig.store.clone();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                store.sync(false);
                let named = store.state().active_id;
                let _ = tx.send((named, store.begin_add().is_err()));
            });
            let seen = rx.recv_timeout(Duration::from_secs(5));
            if seen.is_err() {
                unblock_pipe(&path);
            }
            assert_eq!(seen, Ok((None, true)), "{kind}");
            assert_eq!(live(&rig), b"cookies-a", "{kind}");
            assert!(std::fs::read_to_string(&real).unwrap().contains("uuid-a"));
            assert!(!std::fs::symlink_metadata(&path).unwrap().is_file());
        }
    }

    #[test]
    fn a_stale_config_temp_that_is_a_link_is_replaced_and_never_written_through() {
        let rig = AddRig::new("temp-link", FakeApps::new());
        rig.sign_in("uuid-a", b"cookies-a");
        let elsewhere = rig.root.join("elsewhere");
        std::fs::write(&elsewhere, b"not ours").unwrap();
        let temp = format!(".config.json.{}.tmp", std::process::id());
        symlink(&elsewhere, rig.claude().join(temp)).unwrap();
        rig.store.begin_add().unwrap();
        assert_eq!(std::fs::read(&elsewhere).unwrap(), b"not ours");
        let path = rig.claude().join("config.json");
        assert!(std::fs::symlink_metadata(&path).unwrap().is_file());
        assert!(!config(&rig).contains("uuid-a"));
        assert!(!cookies(&rig.claude()).exists());
    }

    #[test]
    fn a_stale_config_temp_that_is_a_pipe_never_holds_a_change_up() {
        let rig = AddRig::new("temp-pipe", FakeApps::new());
        rig.sign_in("uuid-a", b"cookies-a");
        let temp = rig
            .claude()
            .join(format!(".config.json.{}.tmp", std::process::id()));
        make_pipe(&temp);
        let store = rig.store.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(store.begin_add().is_ok());
        });
        let began = rx.recv_timeout(Duration::from_secs(5));
        if began.is_err() {
            use std::os::unix::fs::OpenOptionsExt;
            let mut reader = std::fs::OpenOptions::new();
            let _ = reader.read(true).custom_flags(libc::O_NONBLOCK).open(&temp);
        }
        assert_eq!(began, Ok(true));
        assert!(!config(&rig).contains("uuid-a"));
    }

    #[test]
    fn the_first_backup_is_a_private_file_never_written_through_a_link() {
        let rig = AddRig::new("backup-link", FakeApps::new());
        rig.sign_in("uuid-a", b"cookies-a");
        let leak = rig.root.join("leak");
        symlink(&leak, backup(&rig.claude())).unwrap();
        rig.store.begin_add().unwrap();
        assert!(!leak.exists(), "the cookies went through a link");

        let rig = AddRig::new("backup-mode", FakeApps::new());
        rig.sign_in("uuid-a", b"cookies-a");
        let open = PermissionsExt::from_mode(0o644);
        std::fs::set_permissions(cookies(&rig.claude()), open).unwrap();
        rig.store.begin_add().unwrap();
        let copy = backup(&rig.claude());
        assert_eq!(std::fs::read(&copy).unwrap(), b"cookies-a");
        let mode = std::fs::metadata(&copy).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
