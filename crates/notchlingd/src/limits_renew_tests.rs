use super::*;
use crate::accounts_ops_tests::{profile, slot, world, World, ID1, ID2};
use notchling_engine::accounts::claude::{LIVE_SERVICE, SAVED_SERVICE};
use notchling_engine::accounts::fake::{MemoryKeychain, ScriptedHttp};
use notchling_engine::accounts::http::{PROFILE_URL, TOKEN_URL};
use notchling_engine::lanes::service_for;

const USAGE: &str = r#"{"five_hour":{"utilization":42.0,"resets_at":"2027-01-15T10:00:00Z"},"seven_day":{"utilization":10.0}}"#;
const EXPIRED: &str = r#"{"claudeAiOauth":{"accessToken":"old","expiresAt":1000}}"#;
const FRESH: &str = r#"{"claudeAiOauth":{"accessToken":"fresh","expiresAt":4000000000000}}"#;

type OnRenew = Box<dyn Fn(Option<&str>) + Send + Sync>;

struct FakeRenewer {
    busy: Mutex<HashSet<Option<String>>>,
    calls: Mutex<Vec<Option<String>>>,
    result: Mutex<Result<(), String>>,
    on_renew: OnRenew,
}

impl FakeRenewer {
    fn new(on_renew: impl Fn(Option<&str>) + Send + Sync + 'static) -> Arc<FakeRenewer> {
        Arc::new(FakeRenewer {
            busy: Mutex::new(HashSet::new()),
            calls: Mutex::new(Vec::new()),
            result: Mutex::new(Ok(())),
            on_renew: Box::new(on_renew),
        })
    }

    fn calls(&self) -> Vec<Option<String>> {
        lock(&self.calls).clone()
    }
}

impl LoginRenewer for FakeRenewer {
    fn busy(&self, config_dir: Option<&str>) -> bool {
        lock(&self.busy).contains(&config_dir.map(String::from))
    }

    fn renew(&self, config_dir: Option<&str>) -> Result<(), String> {
        lock(&self.calls).push(config_dir.map(String::from));
        (self.on_renew)(config_dir);
        lock(&self.result).clone()
    }
}

const LANE: &str = "/lanes/two";

fn expired_world(tag: &str) -> (World, Arc<MemoryKeychain>, Arc<ScriptedHttp>) {
    let chain = Arc::new(MemoryKeychain::new());
    let http = Arc::new(ScriptedHttp::new());
    let w = world(tag, true, chain.clone(), http.clone());
    std::fs::write(
        w.d.0.join("home/.claude.json"),
        "{\"oauthAccount\": {\"accountUuid\": \"a1a1\", \"organizationUuid\": \"0101\", \"emailAddress\": \"one@example.com\"}}",
    )
    .unwrap();
    chain.put(LIVE_SERVICE, "me", EXPIRED);
    chain.put(SAVED_SERVICE, ID2, &slot(ID2, "two@example.com", "2"));
    chain.put(&service_for(LANE).unwrap(), "me", EXPIRED);
    std::fs::create_dir_all(w.d.0.join("data/daemon")).unwrap();
    std::fs::write(
        w.d.0.join("data/daemon/accounts.json"),
        format!(r#"[{{"id":"{ID2}","email":"two@example.com","plan":"Team"}}]"#),
    )
    .unwrap();
    std::fs::write(
        w.d.0.join("data/lanes.json"),
        format!(
            r#"[{{"name":"two","path":"{LANE}","accountID":"{ID2}","email":"two@example.com"}}]"#
        ),
    )
    .unwrap();
    http.answer(PROFILE_URL, 200, &profile(ID1, "one@example.com"));
    (w, chain, http)
}

fn source(w: &World, renewer: &Arc<FakeRenewer>) -> ClaudeLimits {
    ClaudeLimits::new(w.flag.clone(), w.ops.store())
        .with_clock(Arc::new(|| 1_800_000_000.0))
        .with_renewer(renewer.clone())
}

#[test]
fn refresh_asks_claude_code_to_renew_an_expired_login_once_and_reads_limits_again() {
    let (w, chain, http) = expired_world("lim-renew");
    let renewer = FakeRenewer::new(move |dir| {
        let service = match dir {
            Some(path) => service_for(path).unwrap(),
            None => LIVE_SERVICE.to_string(),
        };
        chain.put(&service, "me", FRESH);
    });
    let source = source(&w, &renewer);

    source.sync_usage();
    assert!(renewer.calls().is_empty(), "a timer read never renews");
    assert!(source.usage_error(ID1).is_some());
    assert!(source.usage_error(ID2).is_some());

    http.answer(USAGE_URL, 200, USAGE);
    http.answer(USAGE_URL, 200, USAGE);
    source.sync_usage_now();
    let mut calls = renewer.calls();
    calls.sort();
    assert_eq!(calls, vec![None, Some(LANE.to_string())]);
    assert!(source.usage(ID1).is_some() && source.usage(ID2).is_some());
    assert_eq!(source.usage_error(ID1), None);
    assert_eq!(source.usage_error(ID2), None);
    assert_eq!(source.renewal(ID1), None);
    assert_eq!(source.renewal(ID2), None);
    assert!(!http.seen().iter().any(|(url, _)| url == TOKEN_URL));
}

#[test]
fn a_place_with_a_running_session_is_skipped_and_a_renewal_that_fails_says_so() {
    let (w, _chain, _http) = expired_world("lim-renew-skip");
    let renewer = FakeRenewer::new(|_| {});
    lock(&renewer.busy).insert(None);
    *lock(&renewer.result) = Err("couldn't start it".into());
    let source = source(&w, &renewer);

    source.sync_usage_now();
    assert_eq!(renewer.calls(), vec![Some(LANE.to_string())]);
    assert_eq!(source.renewal(ID1), None);
    assert!(source.usage_error(ID1).is_some());
    assert_eq!(source.renewal(ID2), Some(RenewState::Failed));
    assert!(source.usage_error(ID2).is_some());

    lock(&renewer.busy).insert(Some(LANE.to_string()));
    source.sync_usage_now();
    assert_eq!(renewer.calls().len(), 1, "busy now, so not run again");
    assert_eq!(
        source.renewal(ID2),
        None,
        "a new click clears the old failure"
    );
}

#[test]
fn nothing_is_renewed_while_accounts_is_off() {
    let (w, _chain, _http) = expired_world("lim-renew-off");
    let renewer = FakeRenewer::new(|_| {});
    let source = source(&w, &renewer);
    w.flag.set(false);
    source.sync_usage_now();
    assert!(renewer.calls().is_empty());
}
