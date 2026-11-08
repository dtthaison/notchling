//! The watchers' sources over the real stores, on fakes: an in-memory Keychain, scripted HTTP, a
//! fake provider and a fake launcher. Off, nothing is called (the fakes that stand for a login
//! panic); on, each account's usage is read with its own token and nothing is renewed.

use super::*;
use crate::accounts_ops_tests::{profile, slot, world, ID1, ID2};
use crate::acting_ops_tests::{sessions, setup_in, PROMPT};
use crate::switches::{ACCOUNTS, ACCOUNTS_OFF};
use notchling_engine::accounts::claude::{LIVE_SERVICE, SAVED_SERVICE};
use notchling_engine::accounts::fake::{MemoryKeychain, PanicHttp, PanicKeychain, ScriptedHttp};
use notchling_engine::accounts::http::{PROFILE_URL, TOKEN_URL};
use notchling_engine::control::{FakeLauncher, Launcher};
use notchling_engine::providers::usage::{Bar as PBar, Group as PGroup};
use notchling_engine::providers::{
    AccountError, AddProgress, AddStarted, Capabilities, ProviderAccount, ProviderId, ProviderState,
};
use notchling_engine::sessions::Roots;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

const USAGE: &str = r#"{"five_hour":{"utilization":42.0,"resets_at":"2027-01-15T10:00:00Z"},"seven_day":{"utilization":10.0}}"#;

#[test]
fn claude_off_reads_no_login_and_switches_nothing() {
    let w = world(
        "lim-off",
        false,
        Arc::new(PanicKeychain),
        Arc::new(PanicHttp),
    );
    let source = ClaudeLimits::new(w.flag.clone(), w.ops.store());
    source.sync_usage();
    assert_eq!(source.usage(ID1), None);
    assert_eq!(source.switch_to(ID2), Err(ACCOUNTS_OFF.to_string()));
    assert_eq!(
        source.switch_lane("two", ID2),
        Err(ACCOUNTS_OFF.to_string())
    );
    // Listing reads the daemon's own file only.
    assert!(source.accounts().is_empty());
    assert!(!w.d.0.join("data/daemon/accounts.json").exists());
}

#[test]
fn claude_on_reads_each_accounts_usage_with_its_own_token_and_renews_nothing() {
    let chain = Arc::new(MemoryKeychain::new());
    let http = Arc::new(ScriptedHttp::new());
    let w = world("lim-on", true, chain.clone(), http.clone());
    std::fs::write(
        w.d.0.join("home/.claude.json"),
        "{\"oauthAccount\": {\"accountUuid\": \"a1a1\", \"organizationUuid\": \"0101\", \"emailAddress\": \"one@example.com\"}}",
    )
    .unwrap();
    chain.put(
        LIVE_SERVICE,
        "me",
        r#"{"claudeAiOauth":{"accessToken":"test-access-1"}}"#,
    );
    chain.put(SAVED_SERVICE, ID2, &slot(ID2, "two@example.com", "2"));
    std::fs::create_dir_all(w.d.0.join("data/daemon")).unwrap();
    std::fs::write(
        w.d.0.join("data/daemon/accounts.json"),
        format!(r#"[{{"id":"{ID2}","email":"two@example.com","plan":"Pro"}}]"#),
    )
    .unwrap();
    http.answer(PROFILE_URL, 200, &profile(ID1, "one@example.com"));
    http.answer(USAGE_URL, 200, USAGE);
    http.answer(USAGE_URL, 200, USAGE);

    let source =
        ClaudeLimits::new(w.flag.clone(), w.ops.store()).with_clock(Arc::new(|| 1_800_000_000.0));
    source.sync_usage();
    assert_eq!(source.active_id().as_deref(), Some(ID1));
    for id in [ID1, ID2] {
        let usage = source.usage(id).expect("read");
        assert_eq!(usage.session.map(|w| w.percent), Some(42.0));
        assert_eq!(usage.week.map(|w| w.percent), Some(10.0));
    }
    let mut bearers: Vec<Option<String>> = http
        .seen()
        .into_iter()
        .filter(|(url, _)| url == USAGE_URL)
        .map(|(_, bearer)| bearer)
        .collect();
    bearers.sort();
    assert_eq!(
        bearers,
        vec![
            Some("test-access-1".to_string()),
            Some("test-access-2".to_string())
        ]
    );
    assert!(!http.seen().iter().any(|(url, _)| url == TOKEN_URL));

    // Read this round: the next sync asks again only once they're due.
    let before = http.seen().len();
    source.sync_usage();
    let again: Vec<_> = http.seen()[before..]
        .iter()
        .filter(|(url, _)| url == USAGE_URL)
        .cloned()
        .collect();
    assert!(again.is_empty(), "{again:?}");

    // Off again: nothing more is read.
    w.flag.set(false);
    let before = http.seen().len();
    source.sync_usage();
    assert_eq!(http.seen().len(), before);
}

/// A provider that counts what it was asked.
#[derive(Default)]
struct CountingProvider {
    syncs: AtomicUsize,
    switches: AtomicUsize,
}

impl AccountProvider for CountingProvider {
    fn id(&self) -> ProviderId {
        ProviderId::new(ProviderId::CODEX)
    }
    fn title(&self) -> &'static str {
        "Codex"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::USAGE
    }
    fn state(&self) -> ProviderState {
        ProviderState {
            accounts: vec![ProviderAccount::new("u1:w1", "a@example.com", None)],
            active_id: Some("u1:w1".into()),
            ..ProviderState::default()
        }
    }
    fn sync(&self, _force: bool) {
        self.syncs.fetch_add(1, Ordering::SeqCst);
    }
    fn usage_report(&self, _account: &str) -> Option<ProviderReport> {
        Some(ProviderReport {
            groups: vec![PGroup {
                name: Some("Codex".into()),
                bars: vec![PBar {
                    label: "5h".into(),
                    kind: Kind::Session,
                    percent: 91.0,
                    resets_at: Some(1_800_000_100.0),
                }],
            }],
            fetched: 1_800_000_000.0,
        })
    }
    fn switch_to(&self, _id: &str) -> Result<(), AccountError> {
        self.switches.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn begin_add(&self) -> Result<AddStarted, AccountError> {
        Err(AccountError::new("no"))
    }
    fn poll_add(&self) -> AddProgress {
        AddProgress::Idle
    }
    fn cancel_add(&self) {}
    fn remove(&self, _id: &str) {}
    fn switch_failure_title(&self) -> &'static str {
        "Couldn't Switch"
    }
}

#[test]
fn a_provider_is_synced_and_switched_only_while_accounts_are_on() {
    let flag = SwitchFlag::in_memory(ACCOUNTS, ACCOUNTS_OFF, false);
    let provider = Arc::new(CountingProvider::default());
    let source = ProviderLimits::new(flag.clone(), provider.clone());
    source.sync_usage();
    assert_eq!(source.switch_to("u1:w1"), Err(ACCOUNTS_OFF.to_string()));
    assert_eq!(provider.syncs.load(Ordering::SeqCst), 0);
    assert_eq!(provider.switches.load(Ordering::SeqCst), 0);

    flag.set(true);
    source.sync_usage();
    source.switch_to("u1:w1").unwrap();
    assert_eq!(provider.syncs.load(Ordering::SeqCst), 1);
    assert_eq!(provider.switches.load(Ordering::SeqCst), 1);
    assert_eq!(source.active_id().as_deref(), Some("u1:w1"));
    assert_eq!(source.accounts()[0].email, "a@example.com");
    let report = source.usage_report("u1:w1").unwrap();
    assert_eq!(report.groups[0].name.as_deref(), Some("Codex"));
    assert_eq!(report.groups[0].bars[0].kind, BarKind::Session);
    assert_eq!(report.groups[0].bars[0].percent, 91.0);
    assert_eq!(report.fetched, 1_800_000_000.0);
}

fn fast() -> NudgeTiming {
    NudgeTiming {
        respawn_wait: Duration::from_millis(30),
        poll: Duration::from_millis(5),
        settle: Duration::ZERO,
    }
}

fn roots(tag: &str) -> Roots {
    let root = std::env::temp_dir().join(format!("nl-limsrc-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("home/.claude/jobs/abcd1234")).unwrap();
    Roots {
        main_dir: root.join("home/.claude"),
        lanes_file: root.join("data/lanes.json"),
    }
}

#[test]
fn a_nudge_needs_acting_on() {
    let launcher = Arc::new(FakeLauncher::new());
    let s = setup_in(
        false,
        PROMPT,
        sessions(),
        roots("off"),
        Some(launcher.clone() as Arc<dyn Launcher>),
    );
    let jobs = SessionJobs::new(Arc::new(s.ops)).with_timing(fast());
    assert!(!jobs.nudge("abcd1234", "go on"));
    std::thread::sleep(Duration::from_millis(50));
    assert!(launcher.calls().is_empty());
    assert!(s.attached.lock().unwrap().is_empty());
}

#[test]
fn a_nudge_respawns_the_job_then_types_the_reply_once() {
    let launcher = Arc::new(FakeLauncher::new());
    let s = setup_in(
        true,
        PROMPT,
        sessions(),
        roots("on"),
        Some(launcher.clone() as Arc<dyn Launcher>),
    );
    let jobs = SessionJobs::new(Arc::new(s.ops)).with_timing(fast());
    assert!(jobs.nudge("abcd1234", "go on"));
    assert!(!jobs.nudge("abcd1234", "go on"), "already being restarted");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !lock(&jobs.working).is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(lock(&jobs.working).is_empty());
    let argv: Vec<Vec<String>> = launcher.commands().into_iter().map(|c| c.argv).collect();
    assert_eq!(argv, vec![vec!["claude", "respawn", "abcd1234"]]);
    let attached = s.attached.lock().unwrap().clone();
    assert_eq!(attached.len(), 1);
    assert_eq!(attached[0].0, "abcd1234");
    assert!(s.writes.lock().unwrap().concat().contains("go on"));
    // Done: it may be nudged again.
    assert!(jobs.nudge("abcd1234", "go on"));
}
