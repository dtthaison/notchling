use super::files::{backup, cookies};
use super::store::{ClaudeName, DesktopDeps, DesktopPaths, DesktopStore, BUNDLE_ID};
use crate::accounts::procs::FakeApps;
use crate::providers::{AccountProvider, AddProgress};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub(super) struct AddRig {
    pub(super) root: PathBuf,
    pub(super) apps: Arc<FakeApps>,
    pub(super) store: Arc<DesktopStore>,
}

fn names() -> Vec<ClaudeName> {
    ["a", "b"]
        .iter()
        .map(|n| ClaudeName {
            id: format!("uuid-{n}:org"),
            email: format!("{n}@example.com"),
        })
        .collect()
}

impl AddRig {
    pub(super) fn new(tag: &str, apps: FakeApps) -> AddRig {
        let root =
            std::env::temp_dir().join(format!("nl-desktop-add-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Claude")).unwrap();
        AddRig::over(root, apps)
    }

    pub(super) fn over(root: PathBuf, apps: FakeApps) -> AddRig {
        let apps = Arc::new(apps);
        let store = Arc::new(DesktopStore::new(
            DesktopDeps {
                apps: apps.clone(),
                claude_accounts: Arc::new(names),
            },
            DesktopPaths {
                claude_dir: root.join("Claude"),
                store_dir: root.join("ours/desktop"),
                apps_dir: root.join("Applications"),
            },
        ));
        AddRig { root, apps, store }
    }

    pub(super) fn claude(&self) -> PathBuf {
        self.root.join("Claude")
    }

    pub(super) fn sign_in(&self, uuid: &str, bytes: &[u8]) {
        std::fs::write(cookies(&self.claude()), bytes).unwrap();
        std::fs::write(
            self.claude().join("config.json"),
            format!(r#"{{"lastKnownAccountUuid":"{uuid}","theme":"dark"}}"#),
        )
        .unwrap();
    }

    pub(super) fn sign_out(&self) {
        let _ = std::fs::remove_file(cookies(&self.claude()));
        std::fs::write(self.claude().join("config.json"), r#"{"theme":"dark"}"#).unwrap();
    }

    pub(super) fn ours(&self, uuid: &str) -> PathBuf {
        self.root.join(format!("ours/desktop/{uuid}.cookies"))
    }

    pub(super) fn keep_copy(&self, uuid: &str, bytes: &[u8]) {
        std::fs::create_dir_all(self.root.join("ours/desktop")).unwrap();
        std::fs::write(self.ours(uuid), bytes).unwrap();
    }

    pub(super) fn block_copy(&self, uuid: &str) {
        std::fs::create_dir_all(self.ours(uuid)).unwrap();
    }

    pub(super) fn flush_on_quit(&self, bytes: &'static [u8]) {
        let live = cookies(&self.claude());
        self.apps
            .when_quitting(move |_| std::fs::write(&live, bytes).unwrap());
    }

    pub(super) fn opens(&self) -> usize {
        self.apps
            .events()
            .iter()
            .filter(|e| e.starts_with("open"))
            .count()
    }

    pub(super) fn ids(&self) -> Vec<String> {
        self.store
            .state()
            .accounts
            .into_iter()
            .map(|a| a.id)
            .collect()
    }

    pub(super) fn added_b(&self) {
        self.sign_in("uuid-a", b"cookies-a");
        self.store.begin_add().unwrap();
        self.sign_in("uuid-b", b"cookies-b");
        assert_eq!(self.store.poll_add(), AddProgress::Done);
    }
}

impl Drop for AddRig {
    fn drop(&mut self) {
        if !self.root.as_os_str().is_empty() {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

#[test]
fn a_cancel_after_an_add_ended_done_leaves_desktop_signed_out() {
    let rig = AddRig::new("cancel-done", FakeApps::new().running(BUNDLE_ID));
    rig.added_b();
    rig.sign_out();
    let opens = rig.opens();
    rig.store.cancel_add();
    assert!(!cookies(&rig.claude()).exists(), "no add was under way");
    assert_eq!(rig.opens(), opens);
    assert!(!rig.store.state().busy);
}

#[test]
fn an_abandon_after_an_add_ended_done_names_nothing() {
    let rig = AddRig::new("abandon-done", FakeApps::new().running(BUNDLE_ID));
    rig.added_b();
    assert_eq!(rig.store.abandon_add(), None);
}

#[cfg(unix)]
#[test]
fn a_cancel_after_a_later_begin_failed_leaves_desktop_signed_out() {
    use std::os::unix::fs::PermissionsExt;
    let rig = AddRig::new("cancel-failed-begin", FakeApps::new().running(BUNDLE_ID));
    rig.added_b();
    let locked = |mode| std::fs::set_permissions(rig.claude(), PermissionsExt::from_mode(mode));
    locked(0o500).unwrap();
    let began = rig.store.begin_add();
    locked(0o700).unwrap();
    assert!(began.is_err(), "Desktop could not be signed out");
    assert!(!rig.store.state().adding);
    rig.sign_out();
    let opens = rig.opens();
    rig.store.cancel_add();
    assert!(
        !cookies(&rig.claude()).exists(),
        "the failed begin left no add"
    );
    assert_eq!(rig.opens(), opens);
}

#[test]
fn a_cancel_and_an_abandon_with_no_add_begun_do_nothing() {
    let rig = AddRig::new("never-begun", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.sync(false);
    rig.sign_out();
    rig.store.cancel_add();
    assert_eq!(rig.store.abandon_add(), None);
    assert!(!cookies(&rig.claude()).exists());
    assert!(rig.apps.events().iter().all(|e| !e.starts_with("quit")));
}

#[test]
fn an_abandon_ends_the_add_so_a_later_cancel_changes_nothing() {
    let rig = AddRig::new("abandon", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.begin_add().unwrap();
    assert!(rig.store.abandon_add().is_some());
    assert!(!rig.store.state().adding);
    assert_eq!(rig.store.abandon_add(), None);
    rig.store.cancel_add();
    assert!(!cookies(&rig.claude()).exists());
    assert_eq!(rig.store.poll_add(), AddProgress::Idle);
}

#[test]
fn a_cancel_puts_the_account_back_once_and_a_second_cancel_does_nothing() {
    let rig = AddRig::new("cancel-twice", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.begin_add().unwrap();
    rig.store.cancel_add();
    assert_eq!(std::fs::read(cookies(&rig.claude())).unwrap(), b"cookies-a");
    rig.sign_out();
    let opens = rig.opens();
    rig.store.cancel_add();
    assert!(!cookies(&rig.claude()).exists());
    assert_eq!(rig.opens(), opens);
}

#[test]
fn a_cancel_asked_while_the_add_is_beginning_ends_it_and_puts_the_old_account_back() {
    let rig = AddRig::new("cancel-beginning", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    let cancel: Arc<Mutex<Option<JoinHandle<()>>>> = Arc::new(Mutex::new(None));
    let (store, handle, once) = (rig.store.clone(), cancel.clone(), AtomicBool::new(false));
    rig.apps.when_quitting(move |_| {
        if once.swap(true, Ordering::SeqCst) {
            return;
        }
        let store = store.clone();
        let t = std::thread::spawn(move || store.cancel_add());
        let deadline = Instant::now() + Duration::from_millis(300);
        while !t.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        *handle.lock().unwrap() = Some(t);
    });
    rig.store.begin_add().unwrap();
    let thread = cancel.lock().unwrap().take().unwrap();
    thread.join().unwrap();
    assert!(!rig.store.state().adding, "the cancel ended the add");
    assert_eq!(std::fs::read(cookies(&rig.claude())).unwrap(), b"cookies-a");
    assert_eq!(rig.store.state().active_id.as_deref(), Some("uuid-a"));
    assert_eq!(rig.store.poll_add(), AddProgress::Idle);
}

#[test]
fn an_add_whose_copy_could_not_be_saved_fails_lists_nothing_and_ends() {
    let rig = AddRig::new("unsaved", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.begin_add().unwrap();
    rig.sign_in("uuid-b", b"cookies-b");
    std::fs::create_dir_all(rig.ours("uuid-b")).unwrap();
    assert!(matches!(rig.store.poll_add(), AddProgress::Failed(_)));
    let state = rig.store.state();
    assert!(!state.adding && state.active_id.is_none());
    assert_eq!(rig.ids(), vec!["uuid-a"]);
    assert_eq!(rig.store.poll_add(), AddProgress::Idle);
    rig.sign_out();
    let opens = rig.opens();
    rig.store.cancel_add();
    assert!(!cookies(&rig.claude()).exists(), "the failed add was over");
    assert_eq!(rig.opens(), opens);
}

#[test]
fn a_login_naming_no_usable_account_never_ends_the_add_done() {
    let rig = AddRig::new("bad-uuid", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.begin_add().unwrap();
    rig.sign_in("../escape", b"cookies-x");
    assert!(matches!(rig.store.poll_add(), AddProgress::Failed(_)));
    assert!(rig.store.state().active_id.is_none());
    assert_eq!(rig.ids(), vec!["uuid-a"]);
}

#[test]
fn an_add_waits_while_the_login_is_not_whole() {
    let rig = AddRig::new("whole", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.begin_add().unwrap();
    let waiting = AddProgress::Waiting { sign_in_url: None };
    std::fs::write(cookies(&rig.claude()), b"cookies-b").unwrap();
    assert_eq!(rig.store.poll_add(), waiting, "cookies with no account");
    rig.sign_in("uuid-b", b"");
    assert_eq!(
        rig.store.poll_add(),
        waiting,
        "an account with empty cookies"
    );
    assert!(rig.store.state().adding);
    rig.sign_in("uuid-b", b"cookies-b");
    assert_eq!(rig.store.poll_add(), AddProgress::Done);
    assert_eq!(std::fs::read(rig.ours("uuid-b")).unwrap(), b"cookies-b");
    assert_eq!(rig.ids(), vec!["uuid-a", "uuid-b"]);
    assert_eq!(std::fs::read(backup(&rig.claude())).unwrap(), b"cookies-a");
}

#[test]
fn the_live_login_is_one_read_of_the_account_and_its_whole_cookies() {
    let rig = AddRig::new("one-read", FakeApps::new());
    assert_eq!(super::files::live(&rig.claude()), None);
    std::fs::write(cookies(&rig.claude()), b"cookies-b").unwrap();
    assert_eq!(super::files::live(&rig.claude()), None, "no account named");
    rig.sign_in("uuid-b", b"");
    assert_eq!(super::files::live(&rig.claude()), None, "empty cookies");
    rig.sign_in("uuid-b", b"cookies-b");
    assert_eq!(
        super::files::live(&rig.claude()),
        Some(("uuid-b".to_string(), b"cookies-b".to_vec()))
    );
}

#[test]
fn an_add_saves_the_login_as_it_stands_once_desktop_has_quit() {
    let rig = AddRig::new("save-after-quit", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.flush_on_quit(b"cookies-a-flushed");
    rig.store.begin_add().unwrap();
    assert_eq!(
        std::fs::read(rig.ours("uuid-a")).unwrap(),
        b"cookies-a-flushed"
    );
}

#[test]
fn a_switch_saves_the_login_it_leaves_once_desktop_has_quit() {
    let rig = AddRig::new("switch-saves", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.keep_copy("uuid-b", b"cookies-b");
    rig.flush_on_quit(b"cookies-a-flushed");
    rig.store.switch_to("uuid-b").unwrap();
    assert_eq!(
        std::fs::read(rig.ours("uuid-a")).unwrap(),
        b"cookies-a-flushed"
    );
    assert_eq!(std::fs::read(cookies(&rig.claude())).unwrap(), b"cookies-b");
}

#[test]
fn an_add_is_refused_while_the_login_it_would_replace_cannot_be_saved() {
    let rig = AddRig::new("add-unsaved", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.block_copy("uuid-a");
    let err = rig.store.begin_add().unwrap_err();
    assert!(err.message().contains("left in place"), "{}", err.message());
    assert_eq!(std::fs::read(cookies(&rig.claude())).unwrap(), b"cookies-a");
    assert!(!backup(&rig.claude()).exists());
    let state = rig.store.state();
    assert!(!state.adding && !state.busy);
    assert_eq!(rig.store.abandon_add(), None);
    assert_eq!(
        rig.apps.events().last().map(String::as_str),
        Some(format!("open {BUNDLE_ID}").as_str())
    );
}

#[test]
fn a_switch_is_refused_while_the_login_it_leaves_cannot_be_saved() {
    let rig = AddRig::new("switch-unsaved", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.block_copy("uuid-a");
    rig.keep_copy("uuid-b", b"cookies-b");
    let err = rig.store.switch_to("uuid-b").unwrap_err();
    assert!(err.message().contains("left in place"), "{}", err.message());
    assert_eq!(std::fs::read(cookies(&rig.claude())).unwrap(), b"cookies-a");
    assert!(!backup(&rig.claude()).exists());
    assert!(!rig.store.state().busy);
}

#[test]
fn an_add_and_a_switch_go_ahead_when_desktop_has_no_session_to_keep() {
    let rig = AddRig::new("no-session", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"");
    rig.store.begin_add().unwrap();
    assert!(rig.store.state().adding);
    assert!(!rig.ours("uuid-a").exists());
    let other = AddRig::new("no-session-switch", FakeApps::new());
    other.sign_out();
    other.keep_copy("uuid-b", b"cookies-b");
    other.store.switch_to("uuid-b").unwrap();
    assert_eq!(
        std::fs::read(cookies(&other.claude())).unwrap(),
        b"cookies-b"
    );
}

#[test]
fn a_sync_names_an_account_only_for_a_whole_login_with_a_plain_id() {
    let rig = AddRig::new("sync-names", FakeApps::new());
    let named = |rig: &AddRig| rig.store.state().active_id;
    rig.sign_in("../escape", b"cookies-x");
    rig.store.sync(false);
    assert_eq!(named(&rig), None, "an id that is no file name");
    rig.sign_in("uuid-a", b"");
    rig.store.sync(false);
    assert_eq!(named(&rig), None, "no cookies");
    rig.sign_in("uuid-a", b"cookies-a");
    rig.block_copy("uuid-a");
    rig.store.sync(false);
    assert_eq!(named(&rig).as_deref(), Some("uuid-a"));
    assert!(!rig.store.state().accounts[0].switchable);
    std::fs::remove_dir_all(rig.ours("uuid-a")).unwrap();
    rig.store.sync(false);
    assert!(rig.store.state().accounts[0].switchable);
}

#[test]
fn a_cancel_puts_the_old_account_back_when_the_new_login_is_partial() {
    let rig = AddRig::new("cancel-partial", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.begin_add().unwrap();
    std::fs::write(
        rig.claude().join("config.json"),
        r#"{"lastKnownAccountUuid":"uuid-x"}"#,
    )
    .unwrap();
    rig.store.cancel_add();
    assert_eq!(std::fs::read(cookies(&rig.claude())).unwrap(), b"cookies-a");
    assert_eq!(rig.store.state().active_id.as_deref(), Some("uuid-a"));
}

#[test]
fn a_cancel_leaves_a_login_it_cannot_name_alone() {
    let rig = AddRig::new("cancel-unnamed", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    rig.store.begin_add().unwrap();
    rig.sign_in("../x", b"cookies-x");
    rig.store.cancel_add();
    assert_eq!(std::fs::read(cookies(&rig.claude())).unwrap(), b"cookies-x");
    assert!(!rig.store.state().adding);
}

#[test]
fn an_add_names_the_account_it_ended_on_and_a_later_begin_forgets_it() {
    let rig = AddRig::new("names-added", FakeApps::new().running(BUNDLE_ID));
    rig.sign_in("uuid-a", b"cookies-a");
    assert_eq!(rig.store.added_account(), Some(None));
    rig.store.begin_add().unwrap();
    assert_eq!(rig.store.added_account(), Some(None));
    rig.sign_in("uuid-b", b"cookies-b");
    assert_eq!(rig.store.poll_add(), AddProgress::Done);
    assert_eq!(rig.store.added_account(), Some(Some("uuid-b".to_string())));
    rig.store.begin_add().unwrap();
    assert_eq!(rig.store.added_account(), Some(None));
}

#[cfg(unix)]
#[test]
fn a_copy_that_is_a_link_is_never_taken_for_the_login() {
    let rig = AddRig::new("link", FakeApps::new());
    rig.keep_copy("uuid-b", b"cookies-b");
    let target = rig.root.join("elsewhere");
    std::fs::write(&target, b"cookies-a").unwrap();
    std::os::unix::fs::symlink(&target, rig.ours("uuid-a")).unwrap();
    assert!(!super::files::save(&rig.ours("uuid-a"), b"cookies-a"));
    assert_eq!(std::fs::read(&target).unwrap(), b"cookies-a");
}

#[test]
fn a_sign_out_that_cannot_remove_the_cookies_fails() {
    let rig = AddRig::new("signout-stuck", FakeApps::new());
    rig.sign_in("uuid-a", b"cookies-a");
    std::fs::remove_file(cookies(&rig.claude())).unwrap();
    std::fs::create_dir_all(cookies(&rig.claude()).join("inner")).unwrap();
    assert!(rig.store.begin_add().is_err());
    let config = std::fs::read_to_string(rig.claude().join("config.json")).unwrap();
    assert!(config.contains("uuid-a"));
}
