use super::*;
use notchling_engine::cluster::logins::argv_read_only;
use notchling_engine::inbox::{ActionId, Item};
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

#[derive(Default)]
struct Book {
    state: BTreeMap<String, State>,
    raised: Vec<Item>,
}

#[derive(Default)]
struct Fake(Mutex<Book>);

impl Notes for Fake {
    fn raise(&self, item: Item) {
        let mut b = lock(&self.0);
        b.state.insert(item.id.clone(), State::Open);
        b.raised.push(item);
    }

    fn withdraw(&self, id: &str) {
        let mut b = lock(&self.0);
        if b.state.get(id) == Some(&State::Open) {
            b.state.insert(id.to_string(), State::Withdrawn);
        }
    }

    fn state(&self, id: &str) -> Option<State> {
        lock(&self.0).state.get(id).copied()
    }

    fn open_ids(&self) -> Vec<String> {
        lock(&self.0)
            .state
            .iter()
            .filter(|(_, s)| **s == State::Open)
            .map(|(id, _)| id.clone())
            .collect()
    }
}

impl Fake {
    fn answer(&self, id: &str, state: State) {
        lock(&self.0).state.insert(id.to_string(), state);
    }

    fn raised(&self) -> Vec<Item> {
        lock(&self.0).raised.clone()
    }
}

struct Installed {
    codex: Mutex<Option<String>>,
    agy: bool,
    scanned: std::sync::atomic::AtomicBool,
}

impl Installs for Installed {
    fn codex(&self) -> Option<String> {
        lock(&self.codex).clone()
    }

    fn scanned(&self) -> bool {
        self.scanned.load(Ordering::SeqCst)
    }

    fn agy(&self) -> bool {
        self.agy
    }

    fn home(&self) -> PathBuf {
        PathBuf::from("/home/w")
    }
}

struct Status {
    calls: Mutex<Vec<Vec<String>>>,
    next: Mutex<Vec<Option<(i32, String)>>>,
    count: AtomicUsize,
}

impl Status {
    fn new(answers: &[Option<(i32, &str)>]) -> Arc<Status> {
        let mut next: Vec<_> = answers
            .iter()
            .map(|a| a.map(|(c, t)| (c, t.to_string())))
            .collect();
        next.reverse();
        Arc::new(Status {
            calls: Mutex::new(Vec::new()),
            next: Mutex::new(next),
            count: AtomicUsize::new(0),
        })
    }
}

impl StatusRunner for Status {
    fn run(&self, argv: &[String], _timeout: Duration) -> Option<(i32, String)> {
        assert!(argv_read_only(argv), "{argv:?}");
        lock(&self.calls).push(argv.to_vec());
        self.count.fetch_add(1, Ordering::SeqCst);
        let mut next = lock(&self.next);
        if next.len() > 1 {
            next.pop().flatten()
        } else {
            next.last().cloned().flatten()
        }
    }
}

const ON: Option<(i32, &str)> = Some((0, "Logged in using ChatGPT"));
const OFF: Option<(i32, &str)> = Some((1, "Not logged in"));

struct Rig {
    installed: Arc<Installed>,
    probe: Arc<LoginProbe>,
    notes: Arc<Fake>,
    status: Arc<Status>,
    now: Arc<AtomicI64>,
}

fn rig(answers: &[Option<(i32, &str)>], agy: bool, files: Files) -> Rig {
    rig_on("linux", answers, agy, files)
}

fn rig_on(os: &str, answers: &[Option<(i32, &str)>], agy: bool, files: Files) -> Rig {
    let notes = Arc::new(Fake::default());
    let status = Status::new(answers);
    let now = Arc::new(AtomicI64::new(1_000));
    let clock = now.clone();
    let installed = Arc::new(Installed {
        codex: Mutex::new(Some("/usr/local/bin/codex".to_string())),
        agy,
        scanned: std::sync::atomic::AtomicBool::new(true),
    });
    let probe = LoginProbe::new(
        (installed.clone(), status.clone(), files),
        (
            notes.clone(),
            Arc::new(move || clock.load(Ordering::SeqCst)),
        ),
        ("w 1".to_string(), os.to_string()),
    );
    Rig {
        installed,
        probe,
        notes,
        status,
        now,
    }
}

fn no_files() -> Files {
    Arc::new(|_| false)
}

#[test]
fn probe_flip_raises_login_needed_once() {
    let r = rig(&[OFF], false, no_files());
    r.probe.refresh();
    r.probe.refresh();
    r.probe.refresh();
    let raised = r.notes.raised();
    assert_eq!(raised.len(), 1, "{raised:?}");
    let item = &raised[0];
    assert_eq!(item.kind, Kind::LoginNeeded);
    assert_eq!(item.title, "Codex isn't signed in on w 1");
    assert!(item.detail.contains("codex login --device-auth"));
    assert_eq!(item.actions.len(), 1);
    assert_eq!(item.actions[0].id, ActionId::Done);
    assert_eq!(item.id, "login-signed-out:w_1:codex");
}

#[test]
fn a_flip_back_withdraws_it_and_a_new_flip_raises_a_new_one() {
    let r = rig(&[OFF, ON, OFF], false, no_files());
    r.probe.refresh();
    assert_eq!(r.notes.open_ids().len(), 1);
    r.probe.refresh();
    assert!(r.notes.open_ids().is_empty());
    r.probe.refresh();
    let open = r.notes.open_ids();
    assert_eq!(open, vec!["login-signed-out:w_1:codex#2".to_string()]);
    assert_eq!(r.notes.raised().len(), 2);
}

#[test]
fn a_new_flip_raises_again_even_when_the_old_one_was_answered_done() {
    let r = rig(&[OFF, ON, OFF], false, no_files());
    r.probe.refresh();
    r.notes
        .answer("login-signed-out:w_1:codex", State::Answered);
    r.probe.refresh();
    r.probe.refresh();
    assert_eq!(r.notes.raised().len(), 2);
}

#[test]
fn a_still_signed_out_tool_after_a_restart_does_not_raise_again() {
    let r = rig(&[OFF], false, no_files());
    r.notes
        .answer("login-signed-out:w_1:codex", State::Answered);
    r.probe.refresh();
    assert!(r.notes.raised().is_empty());
    let r = rig(&[OFF], false, no_files());
    r.notes.answer("login-signed-out:w_1:codex", State::Open);
    r.probe.refresh();
    assert!(r.notes.raised().is_empty());
    let r = rig(&[OFF], false, no_files());
    r.notes
        .answer("login-signed-out:w_1:codex", State::Withdrawn);
    r.probe.refresh();
    assert_eq!(r.notes.raised().len(), 1);
}

#[test]
fn a_signed_in_or_unknown_login_raises_nothing() {
    let r = rig(
        &[ON, None, Some((127, "sh: codex: not found"))],
        false,
        no_files(),
    );
    r.probe.refresh();
    r.probe.refresh();
    r.probe.refresh();
    assert!(r.notes.raised().is_empty());
    assert_eq!(r.probe.logins().get(&Tool::Codex), None);
}

#[test]
fn antigravity_is_probed_by_file_and_flips_the_same_way() {
    let present = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = present.clone();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let files: Files = Arc::new(move |p| {
        lock(&log).push(p.to_path_buf());
        flag.load(Ordering::SeqCst)
    });
    let r = rig(&[ON], true, files);
    r.probe.refresh();
    let raised = r.notes.raised();
    assert_eq!(raised.len(), 1);
    assert_eq!(raised[0].title, "Antigravity isn't signed in on w 1");
    present.store(true, Ordering::SeqCst);
    r.probe.refresh();
    assert!(r.notes.open_ids().is_empty());
    assert_eq!(
        lock(&seen)
            .iter()
            .collect::<std::collections::BTreeSet<_>>(),
        [PathBuf::from(
            "/home/w/.gemini/jetski-standalone-oauth-token"
        )]
        .iter()
        .collect()
    );
}

fn keychain_calls(r: &Rig) -> Vec<Vec<String>> {
    lock(&r.status.calls)
        .iter()
        .filter(|a| a.first().is_some_and(|p| p.ends_with("security")))
        .cloned()
        .collect()
}

#[test]
fn mac_keychain_item_is_a_login_and_flips_like_a_file() {
    let r = rig_on("macos", &[ON, ON, OFF], true, no_files());
    r.probe.refresh();
    assert_eq!(r.probe.logins().get(&Tool::Agy), Some(&true));
    assert!(r.notes.raised().is_empty());
    assert_eq!(keychain_calls(&r).len(), 1);
}

#[test]
fn mac_without_file_or_item_has_no_antigravity_entry_and_raises_nothing() {
    let r = rig_on("macos", &[ON, Some((44, "")), None], true, no_files());
    r.probe.refresh();
    assert_eq!(r.probe.logins().get(&Tool::Agy), None);
    assert!(r.notes.raised().is_empty());
    assert_eq!(keychain_calls(&r).len(), 1);
}

#[test]
fn a_mac_login_file_asks_no_keychain_and_linux_never_does() {
    let present: Files = Arc::new(|_| true);
    let r = rig_on("macos", &[ON], true, present);
    r.probe.refresh();
    assert!(keychain_calls(&r).is_empty());
    let r = rig_on("linux", &[ON], true, no_files());
    r.probe.refresh();
    assert!(keychain_calls(&r).is_empty());
}

#[test]
fn the_keychain_check_is_one_exact_argv_that_never_reads_the_secret() {
    let r = rig_on("macos", &[ON, Some((0, ""))], true, no_files());
    r.probe.refresh();
    let calls = keychain_calls(&r);
    assert_eq!(calls, [notchling_engine::cluster::logins::keychain_argv()]);
    for argv in &calls {
        assert!(argv.iter().all(|w| w != "-w" && w != "-g"), "{argv:?}");
        assert_eq!(argv[1], "find-generic-password");
    }
}

fn wait_for(r: &Rig, calls: usize) {
    for _ in 0..400 {
        if r.status.count.load(Ordering::SeqCst) >= calls && lock(&r.probe.inner).running.is_none()
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the probe didn't finish");
}

#[test]
fn the_cache_lasts_ten_minutes_and_the_first_call_does_not_wait_for_the_probe() {
    let r = rig(&[ON], false, no_files());
    assert!(r.probe.logins().is_empty());
    wait_for(&r, 1);
    assert_eq!(r.probe.logins().get(&Tool::Codex), Some(&true));
    r.now.fetch_add(599_000, Ordering::SeqCst);
    r.probe.logins();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(r.status.count.load(Ordering::SeqCst), 1);
    r.now.fetch_add(2_000, Ordering::SeqCst);
    r.probe.logins();
    wait_for(&r, 2);
}

#[test]
fn has_content_needs_a_non_empty_file_and_never_reads_it() {
    let dir = std::env::temp_dir().join(format!("nl-lp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let full = dir.join("full");
    let empty = dir.join("empty");
    std::fs::write(&full, "x").unwrap();
    std::fs::write(&empty, "").unwrap();
    assert!(has_content(&full));
    assert!(!has_content(&empty));
    assert!(!has_content(&dir));
    assert!(!has_content(&dir.join("missing")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nothing_is_stamped_while_the_tool_scan_has_not_finished() {
    let r = rig(&[ON], false, no_files());
    r.installed.scanned.store(false, Ordering::SeqCst);
    assert!(!r.probe.refresh());
    assert!(lock(&r.probe.inner).at.is_none());
    assert_eq!(r.status.count.load(Ordering::SeqCst), 0);
    assert!(r.probe.logins().is_empty());
    std::thread::sleep(Duration::from_millis(50));
    LoginProbe::wait_then_refresh(
        Arc::downgrade(&r.probe),
        Duration::from_millis(30),
        Duration::from_millis(5),
    );
    assert_eq!(r.status.count.load(Ordering::SeqCst), 0);
    assert!(lock(&r.probe.inner).running.is_none());
    r.installed.scanned.store(true, Ordering::SeqCst);
    r.probe.logins();
    wait_for(&r, 1);
    assert_eq!(r.probe.logins().get(&Tool::Codex), Some(&true));
}

#[test]
fn a_scan_that_finishes_while_waiting_lets_the_probe_run() {
    let r = rig(&[ON], false, no_files());
    r.installed.scanned.store(false, Ordering::SeqCst);
    let flag = r.installed.clone();
    let later = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        flag.scanned.store(true, Ordering::SeqCst);
    });
    LoginProbe::wait_then_refresh(
        Arc::downgrade(&r.probe),
        Duration::from_secs(5),
        Duration::from_millis(10),
    );
    later.join().unwrap();
    assert_eq!(r.status.count.load(Ordering::SeqCst), 1);
    assert_eq!(r.probe.logins().get(&Tool::Codex), Some(&true));
}

#[test]
fn a_waiting_probe_lets_go_of_its_owner_when_the_owner_is_dropped() {
    let r = rig(&[ON], false, no_files());
    r.installed.scanned.store(false, Ordering::SeqCst);
    let weak = Arc::downgrade(&r.probe);
    let started = Instant::now();
    let waiting = std::thread::spawn(move || {
        LoginProbe::wait_then_refresh(weak, Duration::from_secs(30), Duration::from_millis(5));
    });
    std::thread::sleep(Duration::from_millis(40));
    let Rig { probe, .. } = r;
    let held = Arc::downgrade(&probe);
    drop(probe);
    waiting.join().unwrap();
    assert!(held.upgrade().is_none());
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_changed_install_is_probed_again_before_ten_minutes() {
    let r = rig(&[ON, OFF], false, no_files());
    r.probe.logins();
    wait_for(&r, 1);
    r.probe.logins();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(r.status.count.load(Ordering::SeqCst), 1);
    *lock(&r.installed.codex) = Some("/opt/homebrew/bin/codex".to_string());
    r.probe.logins();
    wait_for(&r, 2);
    let calls = lock(&r.status.calls).clone();
    assert_eq!(calls[1][0], "/opt/homebrew/bin/codex");
}

#[test]
fn a_probe_stuck_for_a_minute_and_a_half_does_not_stop_probing_for_good() {
    let r = rig(&[ON], false, no_files());
    r.probe.logins();
    wait_for(&r, 1);
    let later = 1_000 + 600_001;
    r.now.store(later, Ordering::SeqCst);
    lock(&r.probe.inner).running = Some(later - 10_000);
    r.probe.logins();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(r.status.count.load(Ordering::SeqCst), 1, "busy under 90 s");
    lock(&r.probe.inner).running = Some(later - 90_000);
    r.probe.logins();
    wait_for(&r, 2);
}

#[test]
fn a_timed_out_probe_is_unknown_and_never_signed_out() {
    let r = rig(&[OFF, None], false, no_files());
    r.probe.refresh();
    assert_eq!(r.probe.logins().get(&Tool::Codex), Some(&false));
    r.probe.refresh();
    assert_eq!(lock(&r.probe.inner).result.get(&Tool::Codex), None);
    assert_eq!(r.notes.raised().len(), 1);
}
