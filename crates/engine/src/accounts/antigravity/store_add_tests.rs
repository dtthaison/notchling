use super::login::{decode_keyring, encode_keyring, fake_login};
use super::store::{
    AntigravityDeps, AntigravityPaths, AntigravityStore, LIVE_ACCOUNT, LIVE_SERVICE, SERVICE,
};
use crate::accounts::generic_password::{FakeKeychain, Keychain, KeychainError};
use crate::accounts::procs::{FakeApps, FakeProcs};
use crate::providers::{AccountProvider, AddProgress, FakeHttp};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

type Hook = Box<dyn FnOnce() + Send>;

struct Probed {
    inner: Arc<FakeKeychain>,
    during: Mutex<Option<Hook>>,
    saves_fail: Mutex<bool>,
}

impl Keychain for Probed {
    fn read(&self, service: &str, account: &str) -> Option<String> {
        if service == LIVE_SERVICE {
            let hook = self.during.lock().unwrap().take();
            if let Some(hook) = hook {
                hook();
            }
        }
        self.inner.read(service, account)
    }

    fn write(&self, service: &str, account: &str, value: &str) -> Result<(), KeychainError> {
        if service == SERVICE && *self.saves_fail.lock().unwrap() {
            return Err(KeychainError::Run("locked".into()));
        }
        self.inner.write(service, account, value)
    }

    fn delete(&self, service: &str, account: &str) {
        self.inner.delete(service, account);
    }
}

struct AddRig {
    root: PathBuf,
    fake: Arc<FakeKeychain>,
    probed: Arc<Probed>,
    store: Arc<AntigravityStore>,
}

impl AddRig {
    fn new(tag: &str) -> AddRig {
        let root = std::env::temp_dir().join(format!("nl-agy-add-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("gemini")).unwrap();
        let fake = Arc::new(FakeKeychain::new());
        let probed = Arc::new(Probed {
            inner: fake.clone(),
            during: Mutex::new(None),
            saves_fail: Mutex::new(false),
        });
        let store = Arc::new(AntigravityStore::new(
            AntigravityDeps {
                keychain: probed.clone(),
                http: Arc::new(FakeHttp::new()),
                procs: Arc::new(FakeProcs::new()),
                apps: Arc::new(FakeApps::new()),
                clock: Arc::new(|| 100.0),
            },
            AntigravityPaths {
                token_file: root.join("gemini/jetski-standalone-oauth-token"),
                apps_dir: root.join("Applications"),
                state_dir: root.join("state"),
                data_dir: root.clone(),
            },
        ));
        AddRig {
            root,
            fake,
            probed,
            store,
        }
    }

    fn set_live(&self, login: &str) {
        self.fake
            .write(LIVE_SERVICE, LIVE_ACCOUNT, &encode_keyring(login))
            .unwrap();
    }

    fn token_file(&self) -> PathBuf {
        self.root.join("gemini/jetski-standalone-oauth-token")
    }

    fn cancel_while(&self, during: impl FnOnce(Hook)) -> Arc<Mutex<Option<JoinHandle<()>>>> {
        let cancel: Arc<Mutex<Option<JoinHandle<()>>>> = Arc::new(Mutex::new(None));
        let (store, handle) = (self.store.clone(), cancel.clone());
        during(Box::new(move || {
            let t = std::thread::spawn(move || store.cancel_add());
            let deadline = Instant::now() + Duration::from_millis(300);
            while !t.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            *handle.lock().unwrap() = Some(t);
        }));
        cancel
    }

    fn live(&self) -> Option<String> {
        self.fake
            .get(LIVE_SERVICE, LIVE_ACCOUNT)
            .and_then(|v| decode_keyring(&v))
    }
}

impl Drop for AddRig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn a_cancel_during_the_login_probe_ends_the_add_cancelled_not_done() {
    let rig = AddRig::new("cancel-probe");
    let a = fake_login("1", "a@example.com", "ta");
    rig.set_live(&a);
    rig.store.begin_add().unwrap();
    assert_eq!(rig.live(), None);

    let cancel = rig.cancel_while(|hook| *rig.probed.during.lock().unwrap() = Some(hook));

    let progress = rig.store.poll_add();
    let t = cancel.lock().unwrap().take().unwrap();
    t.join().unwrap();

    assert_ne!(progress, AddProgress::Done);
    assert_eq!(rig.store.added_account(), Some(None));
    assert_eq!(
        rig.live().as_deref(),
        Some(a.as_str()),
        "the old login is back"
    );
    assert_eq!(rig.store.poll_add(), AddProgress::Idle);
}

#[test]
fn a_login_that_names_no_account_yet_keeps_the_add_waiting() {
    let rig = AddRig::new("half-login");
    rig.set_live(&fake_login("1", "a@example.com", "ta"));
    rig.store.begin_add().unwrap();
    std::fs::write(
        rig.root.join("gemini/jetski-standalone-oauth-token"),
        "{\"auth",
    )
    .unwrap();

    assert_eq!(
        rig.store.poll_add(),
        AddProgress::Waiting { sign_in_url: None }
    );
    assert!(rig.store.state().adding);
    assert_eq!(rig.store.added_account(), Some(None));

    rig.set_live(&fake_login("2", "b@example.com", "tb"));
    assert_eq!(rig.store.poll_add(), AddProgress::Done);
    assert_eq!(rig.store.added_account().flatten().as_deref(), Some("2"));
    assert!(!rig.store.state().adding);
}

#[test]
fn a_cancel_while_the_add_is_beginning_still_ends_it_and_puts_the_old_login_back() {
    let rig = AddRig::new("cancel-begin");
    let a = fake_login("1", "a@example.com", "ta");
    rig.set_live(&a);
    let cancel = rig.cancel_while(|hook| *rig.probed.during.lock().unwrap() = Some(hook));

    rig.store.begin_add().unwrap();
    let t = cancel.lock().unwrap().take().unwrap();
    t.join().unwrap();

    assert_eq!(rig.store.poll_add(), AddProgress::Idle);
    assert!(!rig.store.state().adding);
    assert_eq!(
        rig.live().as_deref(),
        Some(a.as_str()),
        "the old login is back"
    );
}

#[test]
fn an_add_whose_new_login_could_not_be_saved_fails_and_names_no_account() {
    let rig = AddRig::new("unsaved");
    rig.set_live(&fake_login("1", "a@example.com", "ta"));
    rig.store.begin_add().unwrap();
    *rig.probed.saves_fail.lock().unwrap() = true;
    rig.set_live(&fake_login("2", "b@example.com", "tb"));

    assert!(matches!(rig.store.poll_add(), AddProgress::Failed(_)));
    assert_eq!(rig.store.added_account(), Some(None));
    assert!(!rig.store.state().adding);
}

#[test]
fn an_add_waits_on_a_nameless_login_even_after_a_read_saw_a_whole_one() {
    let rig = AddRig::new("stale-active");
    rig.set_live(&fake_login("1", "a@example.com", "ta"));
    rig.store.begin_add().unwrap();
    rig.set_live(&fake_login("2", "b@example.com", "tb"));
    rig.store.sync(false);
    rig.fake.delete(LIVE_SERVICE, LIVE_ACCOUNT);
    std::fs::write(rig.token_file(), "{\"auth").unwrap();

    assert_eq!(
        rig.store.poll_add(),
        AddProgress::Waiting { sign_in_url: None }
    );
    assert_eq!(rig.store.added_account(), Some(None));
    assert!(rig.store.state().adding);
}

fn on_live_read(probed: &Arc<Probed>, nth: usize, hook: Hook) {
    if nth <= 1 {
        *probed.during.lock().unwrap() = Some(hook);
        return;
    }
    let next = probed.clone();
    *probed.during.lock().unwrap() = Some(Box::new(move || on_live_read(&next, nth - 1, hook)));
}

#[test]
fn a_login_gone_while_the_poll_checks_it_never_ends_the_add_unsaved() {
    let rig = AddRig::new("gone-unsaved");
    rig.set_live(&fake_login("1", "a@example.com", "ta"));
    rig.store.begin_add().unwrap();
    *rig.probed.saves_fail.lock().unwrap() = true;
    rig.set_live(&fake_login("2", "b@example.com", "tb"));
    let fake = rig.fake.clone();
    on_live_read(
        &rig.probed,
        3,
        Box::new(move || fake.delete(LIVE_SERVICE, LIVE_ACCOUNT)),
    );

    assert_ne!(rig.store.poll_add(), AddProgress::Done);
    assert_eq!(rig.store.added_account(), Some(None));
}

#[test]
fn a_poll_decides_from_one_read_of_the_login_whatever_changes_after_it() {
    let (a, b) = (
        fake_login("1", "a@example.com", "ta"),
        fake_login("2", "b@example.com", "tb"),
    );
    let rig = AddRig::new("one-read-refresh");
    rig.set_live(&a);
    rig.store.begin_add().unwrap();
    rig.set_live(&b);
    let (fake, refreshed) = (
        rig.fake.clone(),
        encode_keyring(&fake_login("2", "b@example.com", "tb2")),
    );
    on_live_read(
        &rig.probed,
        3,
        Box::new(move || fake.write(LIVE_SERVICE, LIVE_ACCOUNT, &refreshed).unwrap()),
    );
    assert_eq!(rig.store.poll_add(), AddProgress::Done);
    assert_eq!(rig.store.added_account().flatten().as_deref(), Some("2"));

    let rig = AddRig::new("one-read-half");
    rig.set_live(&a);
    rig.store.begin_add().unwrap();
    rig.set_live(&b);
    rig.store.sync(false);
    let (fake, token) = (rig.fake.clone(), rig.token_file());
    on_live_read(
        &rig.probed,
        2,
        Box::new(move || {
            fake.delete(LIVE_SERVICE, LIVE_ACCOUNT);
            std::fs::write(token, "{\"auth").unwrap();
        }),
    );
    assert_eq!(rig.store.poll_add(), AddProgress::Done);
    assert_eq!(rig.store.added_account().flatten().as_deref(), Some("2"));
}

#[test]
fn a_login_whose_copy_could_not_be_saved_is_never_listed_so_a_retry_adds_it() {
    let rig = AddRig::new("unsaved-book");
    rig.set_live(&fake_login("1", "a@example.com", "ta"));
    rig.store.begin_add().unwrap();
    *rig.probed.saves_fail.lock().unwrap() = true;
    let b = fake_login("2", "b@example.com", "tb");
    rig.set_live(&b);
    rig.store.sync(false);
    assert!(matches!(rig.store.poll_add(), AddProgress::Failed(_)));
    let ids = |rig: &AddRig| -> Vec<String> {
        rig.store
            .state()
            .accounts
            .into_iter()
            .map(|a| a.id)
            .collect()
    };
    assert_eq!(ids(&rig), ["1"]);

    *rig.probed.saves_fail.lock().unwrap() = false;
    rig.fake.delete(LIVE_SERVICE, LIVE_ACCOUNT);
    rig.store.begin_add().unwrap();
    rig.set_live(&b);
    assert_eq!(rig.store.poll_add(), AddProgress::Done);
    assert_eq!(rig.store.added_account().flatten().as_deref(), Some("2"));
    assert_eq!(ids(&rig), ["1", "2"]);
}

#[test]
fn a_cancel_after_an_add_ended_done_leaves_the_login_alone() {
    let rig = AddRig::new("cancel-after-done");
    rig.set_live(&fake_login("1", "a@example.com", "ta"));
    rig.store.begin_add().unwrap();
    rig.set_live(&fake_login("2", "b@example.com", "tb"));
    assert_eq!(rig.store.poll_add(), AddProgress::Done);
    rig.fake.delete(LIVE_SERVICE, LIVE_ACCOUNT);

    rig.store.cancel_add();

    assert_eq!(
        rig.live(),
        None,
        "no add was under way, so nothing was put back"
    );
    assert_eq!(rig.store.abandon_add(), None);
}

#[test]
fn removing_the_account_in_use_never_lists_it_again_without_its_copy() {
    let rig = AddRig::new("removed-live");
    rig.set_live(&fake_login("1", "a@example.com", "ta"));
    rig.store.sync(false);
    rig.store.remove("1");

    rig.store.sync(false);

    for account in rig.store.state().accounts {
        assert!(
            rig.fake.get(SERVICE, &account.id).is_some(),
            "{} is listed with no saved copy",
            account.id
        );
    }
}
