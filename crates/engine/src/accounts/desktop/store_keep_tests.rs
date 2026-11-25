use super::files::{self, backup, cookies, Dirs};
use super::store::BUNDLE_ID;
use super::store_add_tests::AddRig;
use crate::accounts::procs::FakeApps;
use crate::providers::{AccountProvider, AddProgress};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

pub(super) fn kept(rig: &AddRig) -> Vec<Vec<u8>> {
    let dir = rig.root.join("ours/desktop/unnamed");
    let mut names: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    names.sort();
    names.iter().map(|p| std::fs::read(p).unwrap()).collect()
}

pub(super) fn config(rig: &AddRig) -> String {
    std::fs::read_to_string(rig.claude().join("config.json")).unwrap()
}

pub(super) fn live(rig: &AddRig) -> Vec<u8> {
    std::fs::read(cookies(&rig.claude())).unwrap()
}

pub(super) fn block_config_write(rig: &AddRig) {
    let temp = format!(".config.json.{}.tmp", std::process::id());
    std::fs::create_dir_all(rig.claude().join(temp).join("inner")).unwrap();
}

fn restore(rig: &AddRig, saved: &Path) -> bool {
    let (claude, store) = (rig.claude(), rig.root.join("ours/desktop"));
    let dirs = Dirs {
        claude: &claude,
        store: &store,
    };
    files::restore(&dirs, saved, Some("uuid-b")).is_ok()
}

fn reopened(rig: AddRig, apps: FakeApps) -> AddRig {
    let mut rig = rig;
    let root = std::mem::take(&mut rig.root);
    drop(rig);
    AddRig::over(root, apps)
}

#[cfg(unix)]
pub(super) fn make_pipe(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
}

#[cfg(unix)]
pub(super) fn unblock_pipe(path: &Path) {
    use std::os::unix::fs::OpenOptionsExt;
    let _ = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path);
}

#[test]
fn an_add_keeps_cookies_that_name_no_account_and_never_overwrites_a_kept_copy() {
    let rig = AddRig::new("keep-unnamed", FakeApps::new().running(BUNDLE_ID));
    std::fs::write(backup(&rig.claude()), b"found-first").unwrap();
    rig.sign_out();
    std::fs::write(cookies(&rig.claude()), b"cookies-unnamed").unwrap();
    rig.store.begin_add().unwrap();
    assert!(!cookies(&rig.claude()).exists());
    assert_eq!(kept(&rig), [b"cookies-unnamed".to_vec()]);
    assert_eq!(
        std::fs::read(backup(&rig.claude())).unwrap(),
        b"found-first"
    );
    rig.store.abandon_add();
    std::fs::write(cookies(&rig.claude()), b"cookies-other").unwrap();
    rig.store.begin_add().unwrap();
    assert_eq!(
        kept(&rig),
        [b"cookies-unnamed".to_vec(), b"cookies-other".to_vec()]
    );
    let again = reopened(rig, FakeApps::new());
    assert!(!again.store.is_available(), "a kept copy is no account");
}

#[test]
fn unnamed_cookies_are_kept_up_to_a_bound_and_the_same_cookies_once() {
    let rig = AddRig::new("keep-bound", FakeApps::new());
    let add_over = |bytes: &[u8]| {
        rig.sign_out();
        std::fs::write(cookies(&rig.claude()), bytes).unwrap();
        rig.store.begin_add().unwrap();
        rig.store.abandon_add();
    };
    for n in 0..14 {
        add_over(format!("unnamed-{n}").as_bytes());
    }
    add_over(b"unnamed-13");
    let kept = kept(&rig);
    assert_eq!(kept.len(), 10);
    assert_eq!(kept[0], b"unnamed-4");
    assert_eq!(kept[9], b"unnamed-13");
}

#[test]
fn a_switch_keeps_cookies_whose_id_is_no_file_name_and_goes_ahead() {
    let rig = AddRig::new("keep-odd-id", FakeApps::new());
    rig.sign_in("../x", b"cookies-x");
    rig.keep_copy("uuid-b", b"cookies-b");
    rig.store.switch_to("uuid-b").unwrap();
    assert_eq!(kept(&rig), [b"cookies-x".to_vec()]);
    assert_eq!(live(&rig), b"cookies-b");
}

#[test]
fn an_add_over_a_config_that_is_not_settings_is_refused_and_changes_nothing() {
    for (n, bad) in ["not json", "[]", "null"].into_iter().enumerate() {
        let rig = AddRig::new(&format!("bad-config-{n}"), FakeApps::new());
        rig.sign_in("uuid-a", b"cookies-a");
        std::fs::write(rig.claude().join("config.json"), bad).unwrap();
        assert!(rig.store.begin_add().is_err(), "{bad}");
        assert_eq!(live(&rig), b"cookies-a", "{bad}");
        assert_eq!(config(&rig), bad);
        assert!(!rig.store.state().adding);
        rig.keep_copy("uuid-b", b"cookies-b");
        assert!(rig.store.switch_to("uuid-b").is_err(), "{bad}");
        assert_eq!(live(&rig), b"cookies-a", "{bad}");
    }
}

#[test]
fn an_add_whose_config_cannot_be_written_leaves_the_login_in_place() {
    let rig = AddRig::new("add-config-stuck", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    std::fs::write(rig.claude().join("Cookies-wal"), b"").unwrap();
    block_config_write(&rig);
    assert!(rig.store.begin_add().is_err());
    assert_eq!(live(&rig), b"cookies-a");
    assert!(config(&rig).contains("uuid-a"));
    assert!(!rig.store.state().adding);
    assert_eq!(
        rig.apps.events().last().map(String::as_str),
        Some(format!("open {BUNDLE_ID}").as_str())
    );
}

#[test]
fn a_switch_whose_config_cannot_be_written_leaves_the_old_login_whole() {
    let rig = AddRig::new("switch-config-stuck", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.keep_copy("uuid-b", b"cookies-b");
    block_config_write(&rig);
    assert!(rig.store.switch_to("uuid-b").is_err());
    assert_eq!(live(&rig), b"cookies-a");
    assert!(config(&rig).contains("uuid-a"));
    let strays: Vec<String> = std::fs::read_dir(rig.claude())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".Cookies"))
        .collect();
    assert!(strays.is_empty(), "{strays:?}");
}

#[test]
fn a_restore_or_sign_out_whose_cookies_cannot_change_leaves_config_as_it_was() {
    let rig = AddRig::new("cookies-stuck", FakeApps::new());
    rig.sign_in("uuid-a", b"cookies-a");
    let before = config(&rig);
    std::fs::remove_file(cookies(&rig.claude())).unwrap();
    std::fs::create_dir_all(cookies(&rig.claude()).join("inner")).unwrap();
    rig.keep_copy("uuid-b", b"cookies-b");
    assert!(!restore(&rig, &rig.ours("uuid-b")));
    assert_eq!(config(&rig), before);
    assert!(rig.store.begin_add().is_err());
    assert_eq!(config(&rig), before);
}

#[cfg(unix)]
#[test]
fn a_saved_copy_that_is_a_link_or_a_pipe_is_never_listed_or_switched_to() {
    for kind in ["link", "pipe"] {
        let rig = AddRig::new(&format!("copy-{kind}"), FakeApps::new());
        rig.sign_in("uuid-a", b"cookies-a");
        rig.keep_copy("uuid-a", b"cookies-a");
        let copy = rig.ours("uuid-b");
        match kind {
            "link" => std::os::unix::fs::symlink(cookies(&rig.claude()), &copy).unwrap(),
            _ => make_pipe(&copy),
        }
        let rig = reopened(rig, FakeApps::new().running(BUNDLE_ID));
        rig.store.sync(false);
        assert_eq!(rig.ids(), ["uuid-a"], "{kind}");
        let store = rig.store.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(store.switch_to("uuid-b").is_err());
        });
        let refused = rx.recv_timeout(Duration::from_secs(5));
        if refused.is_err() {
            unblock_pipe(&copy);
        }
        assert_eq!(refused, Ok(true), "{kind}");
        assert_eq!(live(&rig), b"cookies-a", "{kind}");
        assert!(
            rig.apps.events().iter().all(|e| !e.starts_with("quit")),
            "{kind}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_restore_from_a_pipe_fails_at_once() {
    let rig = AddRig::new("pipe-restore", FakeApps::new());
    rig.sign_in("uuid-a", b"cookies-a");
    let pipe = rig.root.join("pipe.cookies");
    make_pipe(&pipe);
    let (tx, rx) = mpsc::channel();
    let (claude, store, from) = (rig.claude(), rig.root.join("ours/desktop"), pipe.clone());
    std::thread::spawn(move || {
        let dirs = Dirs {
            claude: &claude,
            store: &store,
        };
        let _ = tx.send(files::restore(&dirs, &from, Some("uuid-b")).is_err());
    });
    let failed = rx.recv_timeout(Duration::from_secs(5));
    if failed.is_err() {
        unblock_pipe(&pipe);
    }
    assert_eq!(failed, Ok(true));
    assert_eq!(live(&rig), b"cookies-a");
}

#[test]
fn an_add_that_ends_on_an_account_known_before_it_names_none() {
    let rig = AddRig::new("added-known", FakeApps::new());
    rig.sign_in("uuid-z", b"cookies-z");
    rig.store.begin_add().unwrap();
    rig.sign_in("uuid-z", b"cookies-z-again");
    assert_eq!(rig.store.poll_add(), AddProgress::Done);
    assert_eq!(rig.store.added_account(), Some(None), "the one in use");

    rig.sign_in("uuid-y", b"cookies-y");
    rig.store.sync(false);
    rig.sign_in("uuid-z", b"cookies-z");
    rig.store.begin_add().unwrap();
    rig.sign_in("uuid-y", b"cookies-y-again");
    assert_eq!(rig.store.poll_add(), AddProgress::Done);
    assert_eq!(rig.store.added_account(), Some(None), "one saved before");

    rig.store.begin_add().unwrap();
    rig.sign_in("uuid-w", b"cookies-w");
    assert_eq!(rig.store.poll_add(), AddProgress::Done);
    assert_eq!(rig.store.added_account(), Some(Some("uuid-w".to_string())));
}
