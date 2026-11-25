use super::*;
use crate::bridge::net::jwt::tests::{
    good_claims, jwks, FetchLog, TestClock, TestKey, AUDIENCE, START, TEAM,
};
use crate::bridge::net::jwt::{AccessJwtVerifier, AccessKeySet};
use crate::sessions::testutil::TempDir;

pub(crate) fn configured(enabled: bool) -> AccessConfig {
    AccessConfig {
        enabled,
        team_domain: TEAM.into(),
        audience: AUDIENCE.into(),
        client_id: "id.access".into(),
        client_secret: "secret".into(),
        ..AccessConfig::default()
    }
}

pub(crate) fn gate_with(
    config: AccessConfig,
    key: &TestKey,
    clock: &TestClock,
    limiter: Arc<AuthRateLimiter>,
) -> TunnelGate {
    let log = FetchLog::new(vec![jwks(&[key])]);
    let set = Arc::new(AccessKeySet::new(log.fetch(), clock.clock()));
    TunnelGate::new(
        Box::new(move || config.clone()),
        AccessJwtVerifier::new(set, clock.clock()),
        limiter,
        clock.clock(),
    )
}

fn gate(config: AccessConfig, key: &TestKey, clock: &TestClock) -> TunnelGate {
    gate_with(config, key, clock, Arc::new(AuthRateLimiter::new()))
}

fn h(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(n, v)| (n.to_string(), v.to_string()))
        .collect()
}

#[test]
fn headers_without_cloudflare_marks_are_local_even_when_internet_access_is_off() {
    let key = TestKey::a("k1");
    let clock = TestClock::new(START);
    let gate = gate(configured(false), &key, &clock);
    assert_eq!(gate.evaluate(&[]), Decision::Local);
    let local = h(&[
        ("Host", "192.168.1.5"),
        ("Sec-WebSocket-Protocol", "agentconsole.v1"),
    ]);
    assert_eq!(gate.evaluate(&local), Decision::Local);
}

#[test]
fn the_tunnel_is_refused_while_the_toggle_is_off() {
    let key = TestKey::a("k1");
    let clock = TestClock::new(START);
    let gate = gate(configured(false), &key, &clock);
    let token = key.token(&good_claims(clock.now()));
    for headers in [
        h(&[
            ("Cf-Connecting-Ip", "203.0.113.9"),
            ("Cf-Access-Jwt-Assertion", &token),
        ]),
        h(&[
            ("cf-ray", "abc123-SGN"),
            ("Cf-Access-Jwt-Assertion", &token),
        ]),
        h(&[("CF-CONNECTING-IP", "203.0.113.9")]),
    ] {
        assert!(
            matches!(gate.evaluate(&headers), Decision::Reject(_)),
            "{headers:?}"
        );
    }
}

#[test]
fn the_tunnel_needs_a_valid_access_token() {
    let key = TestKey::a("k1");
    let clock = TestClock::new(START);
    let gate = gate(configured(true), &key, &clock);
    let ip = ("Cf-Connecting-Ip", "203.0.113.9");
    assert!(
        matches!(gate.evaluate(&h(&[ip])), Decision::Reject(_)),
        "no token"
    );
    let garbage = h(&[ip, ("Cf-Access-Jwt-Assertion", "garbage")]);
    assert!(matches!(gate.evaluate(&garbage), Decision::Reject(_)));
    let wrong = key.token(&crate::bridge::net::jwt::tests::claims(
        clock.now(),
        Some(serde_json::json!(["nope"])),
        None,
        600.0,
        None,
        0.0,
    ));
    let wrong = h(&[ip, ("Cf-Access-Jwt-Assertion", &wrong)]);
    assert!(
        matches!(gate.evaluate(&wrong), Decision::Reject(_)),
        "wrong aud"
    );

    let good = key.token(&good_claims(clock.now()));
    assert_eq!(
        gate.evaluate(&h(&[ip, ("cf-access-jwt-assertion", &good)])),
        Decision::Tunnel("203.0.113.9".into())
    );
    assert_eq!(
        gate.evaluate(&h(&[("Cf-Ray", "x"), ("Cf-Access-Jwt-Assertion", &good)])),
        Decision::Tunnel(UNKNOWN_CLIENT.into())
    );
}

#[test]
fn the_tunnel_is_refused_while_the_settings_are_incomplete() {
    let key = TestKey::a("k1");
    let clock = TestClock::new(START);
    let mut config = configured(true);
    config.audience = String::new();
    let gate = gate(config, &key, &clock);
    let token = key.token(&good_claims(clock.now()));
    let headers = h(&[
        ("Cf-Connecting-Ip", "203.0.113.9"),
        ("Cf-Access-Jwt-Assertion", &token),
    ]);
    assert!(matches!(gate.evaluate(&headers), Decision::Reject(_)));
}

#[test]
fn lockouts_are_keyed_by_the_client_address_not_loopback() {
    let key = TestKey::a("k1");
    let clock = TestClock::new(START);
    let limiter = Arc::new(AuthRateLimiter::new());
    let gate = gate_with(configured(true), &key, &clock, limiter.clone());
    let token = key.token(&good_claims(clock.now()));
    let headers = |ip: &str| {
        h(&[
            ("Cf-Connecting-Ip", ip),
            ("Cf-Access-Jwt-Assertion", &token),
        ])
    };
    limiter.record_failure("203.0.113.9", clock.now());
    assert!(matches!(
        gate.evaluate(&headers("203.0.113.9")),
        Decision::Reject(_)
    ));
    assert_eq!(
        gate.evaluate(&headers("198.51.100.4")),
        Decision::Tunnel("198.51.100.4".into()),
        "another client behind the same tunnel is unaffected"
    );
    clock.advance(2000);
    assert_eq!(
        gate.evaluate(&headers("203.0.113.9")),
        Decision::Tunnel("203.0.113.9".into()),
        "the 1 s backoff has passed"
    );
}

#[test]
fn settings_are_stored_privately_and_reloaded() {
    let dir = TempDir::new("access-store");
    let file = dir.path("bridge/access.json");
    let store = AccessConfigStore::open(file.clone());
    assert_eq!(store.config().hostname, "console.trietkun.shop");
    assert_eq!(
        store.config().team_domain,
        "bold-dew-100f.cloudflareaccess.com"
    );
    assert!(!store.config().enabled);
    assert!(!file.exists(), "opening writes nothing");

    store
        .update(|c| {
            c.enabled = true;
            c.audience = "aud".into();
            c.client_id = "id".into();
            c.client_secret = "s3cret".into();
        })
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let reloaded = AccessConfigStore::open(file).config();
    assert_eq!(reloaded, store.config());
    assert!(reloaded.offers_tunnel());
}

#[test]
fn reads_the_swift_apps_file() {
    let dir = TempDir::new("access-swift");
    let file = dir.write(
        "bridge/access.json",
        "{\n  \"audience\" : \"aud\",\n  \"clientId\" : \"id\",\n  \"clientSecret\" : \"s\",\n  \"enabled\" : true,\n  \"hostname\" : \"h.example\",\n  \"teamDomain\" : \"t.example\"\n}",
    );
    let config = AccessConfig::read(&file);
    assert!(config.offers_tunnel());
    assert_eq!(config.team_domain, "t.example");
}

#[test]
fn the_qr_keeps_the_v1_fields_and_adds_the_tunnel_only_when_on_and_complete() {
    let object = |access: &AccessConfig| -> Value {
        let hosts = vec!["192.168.1.5".to_string(), "mac.local".to_string()];
        let text = pairing_payload("Mac", 47821, &"a".repeat(64), &hosts, "ABCD2345", access);
        serde_json::from_str(&text).unwrap()
    };
    let mut access = configured(true);
    access.hostname = "console.trietkun.shop".into();
    let with_tunnel = object(&access);
    assert_eq!(with_tunnel["v"], 1);
    assert_eq!(with_tunnel["port"], 47821);
    assert_eq!(
        with_tunnel["hosts"],
        serde_json::json!(["192.168.1.5", "mac.local"])
    );
    assert_eq!(with_tunnel["code"], "ABCD2345");
    assert_eq!(with_tunnel["fp"], "a".repeat(64));
    assert_eq!(with_tunnel["tunnel"]["url"], "wss://console.trietkun.shop");
    assert_eq!(with_tunnel["tunnel"]["accessClientId"], "id.access");
    assert_eq!(with_tunnel["tunnel"]["accessClientSecret"], "secret");

    let mut off = access.clone();
    off.enabled = false;
    assert!(object(&off).get("tunnel").is_none(), "toggle off");
    let mut missing = access.clone();
    missing.client_secret = String::new();
    assert!(
        object(&missing).get("tunnel").is_none(),
        "a field is missing"
    );
    let keys: Vec<String> = object(&off).as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, ["code", "fp", "hosts", "name", "port", "v"]);
}
