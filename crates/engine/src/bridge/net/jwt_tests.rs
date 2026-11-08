use super::*;
use crate::bridge::net::crypto::base64url;
use ring::rand::SystemRandom;
use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::PrivatePkcs8KeyDer;
use serde_json::json;
use std::sync::atomic::AtomicI64;

pub(crate) const TEAM: &str = "team-test.cloudflareaccess.com";
pub(crate) const AUDIENCE: &str = "aud-tag-1234";
pub(crate) const START: i64 = 1_800_000_000_000;

/// A throwaway RSA key standing in for a Cloudflare Access signing key (made for these tests).
pub(crate) struct TestKey {
    pub kid: String,
    pair: RsaKeyPair,
}

impl TestKey {
    pub fn a(kid: &str) -> TestKey {
        TestKey::from_pem(kid, include_str!("testdata/access-key-a.pem"))
    }

    pub fn b(kid: &str) -> TestKey {
        TestKey::from_pem(kid, include_str!("testdata/access-key-b.pem"))
    }

    fn from_pem(kid: &str, pem: &str) -> TestKey {
        let der = PrivatePkcs8KeyDer::from_pem_slice(pem.as_bytes()).unwrap();
        TestKey {
            kid: kid.to_string(),
            pair: RsaKeyPair::from_pkcs8(der.secret_pkcs8_der()).unwrap(),
        }
    }

    /// n and e, read out of the public key's SEQUENCE { INTEGER, INTEGER }.
    fn integers(&self) -> (Vec<u8>, Vec<u8>) {
        let der = self.pair.public().as_ref();
        let mut at = 1;
        let length = |at: &mut usize| {
            let first = der[*at] as usize;
            *at += 1;
            if first < 0x80 {
                return first;
            }
            let mut value = 0;
            for _ in 0..(first & 0x7F) {
                value = value << 8 | der[*at] as usize;
                *at += 1;
            }
            value
        };
        length(&mut at);
        let mut out = Vec::new();
        for _ in 0..2 {
            at += 1; // INTEGER
            let len = length(&mut at);
            let raw = &der[at..at + len];
            at += len;
            out.push(
                raw.iter()
                    .skip_while(|b| **b == 0)
                    .copied()
                    .collect::<Vec<u8>>(),
            );
        }
        (out[0].clone(), out[1].clone())
    }

    pub fn jwk(&self) -> Value {
        let (n, e) = self.integers();
        json!({"kty": "RSA", "alg": "RS256", "use": "sig", "kid": self.kid,
               "n": base64url(&n), "e": base64url(&e)})
    }

    pub fn token(&self, claims: &Value) -> String {
        self.token_with(claims, &json!({}), None)
    }

    pub fn token_with(&self, claims: &Value, extra: &Value, sign_with: Option<&TestKey>) -> String {
        let mut header = json!({"alg": "RS256", "kid": self.kid, "typ": "JWT"});
        for (k, v) in extra.as_object().unwrap() {
            header[k] = v.clone();
        }
        let head = base64url(header.to_string().as_bytes());
        let body = base64url(claims.to_string().as_bytes());
        let signer = sign_with.unwrap_or(self);
        let mut signature = vec![0u8; signer.pair.public().modulus_len()];
        signer
            .pair
            .sign(
                &RSA_PKCS1_SHA256,
                &SystemRandom::new(),
                format!("{head}.{body}").as_bytes(),
                &mut signature,
            )
            .unwrap();
        format!("{head}.{body}.{}", base64url(&signature))
    }
}

pub(crate) fn jwks(keys: &[&TestKey]) -> Vec<u8> {
    json!({"keys": keys.iter().map(|k| k.jwk()).collect::<Vec<_>>(), "public_certs": []})
        .to_string()
        .into_bytes()
}

pub(crate) fn claims(
    now_ms: i64,
    aud: Option<Value>,
    iss: Option<&str>,
    exp: f64,
    nbf: Option<f64>,
    iat: f64,
) -> Value {
    let now = now_ms as f64 / 1000.0;
    let mut c = json!({
        "aud": aud.unwrap_or(json!([AUDIENCE])),
        "iss": iss.map(str::to_string).unwrap_or(format!("https://{TEAM}")),
        "exp": now + exp,
        "iat": now + iat,
    });
    if let Some(nbf) = nbf {
        c["nbf"] = json!(now + nbf);
    }
    c
}

pub(crate) fn good_claims(now_ms: i64) -> Value {
    claims(now_ms, None, None, 600.0, None, 0.0)
}

/// A clock the test moves.
#[derive(Clone)]
pub(crate) struct TestClock(Arc<AtomicI64>);

impl TestClock {
    pub fn new(start: i64) -> TestClock {
        TestClock(Arc::new(AtomicI64::new(start)))
    }
    pub fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
    pub fn advance(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
    pub fn clock(&self) -> Clock {
        let me = self.clone();
        Arc::new(move || me.now())
    }
}

/// Serves the given bodies in turn (the last one again and again) and logs the URLs asked for.
pub(crate) struct FetchLog {
    responses: Mutex<Vec<Result<Vec<u8>, String>>>,
    pub urls: Mutex<Vec<String>>,
}

impl FetchLog {
    pub fn new(responses: Vec<Vec<u8>>) -> Arc<FetchLog> {
        Arc::new(FetchLog {
            responses: Mutex::new(responses.into_iter().map(Ok).collect()),
            urls: Mutex::new(Vec::new()),
        })
    }
    /// Every fetch from now on fails.
    pub fn go_down(&self) {
        *self.responses.lock().unwrap() = vec![Err("down".into())];
    }
    pub fn fetch(self: &Arc<Self>) -> Fetch {
        let me = self.clone();
        Arc::new(move |url: &str| {
            me.urls.lock().unwrap().push(url.to_string());
            let mut r = me.responses.lock().unwrap();
            if r.len() > 1 {
                r.remove(0)
            } else {
                r[0].clone()
            }
        })
    }
}

fn verifier(
    keys: &[&TestKey],
    clock: &TestClock,
) -> (AccessJwtVerifier, Arc<AccessKeySet>, Arc<FetchLog>) {
    let log = FetchLog::new(vec![jwks(keys)]);
    verifier_with(log, clock)
}

fn verifier_with(
    log: Arc<FetchLog>,
    clock: &TestClock,
) -> (AccessJwtVerifier, Arc<AccessKeySet>, Arc<FetchLog>) {
    let set = Arc::new(AccessKeySet::new(log.fetch(), clock.clock()));
    (AccessJwtVerifier::new(set.clone(), clock.clock()), set, log)
}

#[test]
fn a_good_token_verifies_and_the_certs_url_is_the_teams() {
    let key = TestKey::a("k1");
    let clock = TestClock::new(START);
    let (verifier, _, log) = verifier(&[&key], &clock);
    let parsed = verifier
        .verify(&key.token(&good_claims(clock.now())), TEAM, AUDIENCE)
        .unwrap();
    assert_eq!(parsed.audiences, vec![AUDIENCE]);
    assert_eq!(
        log.urls.lock().unwrap()[0],
        format!("https://{TEAM}/cdn-cgi/access/certs")
    );
}

#[test]
fn the_audience_may_be_a_string_or_a_list_and_must_match() {
    let key = TestKey::a("k1");
    let clock = TestClock::new(START);
    let (verifier, _, _) = verifier(&[&key], &clock);
    let with_aud = |aud: Value| key.token(&claims(clock.now(), Some(aud), None, 600.0, None, 0.0));
    assert!(verifier
        .verify(&with_aud(json!(["other", AUDIENCE])), TEAM, AUDIENCE)
        .is_ok());
    assert!(verifier
        .verify(&with_aud(json!(AUDIENCE)), TEAM, AUDIENCE)
        .is_ok());
    assert_eq!(
        verifier.verify(&with_aud(json!(["someone-else"])), TEAM, AUDIENCE),
        Err(JwtError::WrongAudience)
    );
    assert_eq!(
        verifier.verify(&key.token(&good_claims(clock.now())), TEAM, ""),
        Err(JwtError::WrongAudience),
        "an unset AUD never matches"
    );
}

#[test]
fn a_wrong_issuer_is_refused() {
    let key = TestKey::a("k1");
    let clock = TestClock::new(START);
    let (verifier, _, _) = verifier(&[&key], &clock);
    let token = key.token(&claims(
        clock.now(),
        None,
        Some("https://evil.cloudflareaccess.com"),
        600.0,
        None,
        0.0,
    ));
    assert_eq!(
        verifier.verify(&token, TEAM, AUDIENCE),
        Err(JwtError::WrongIssuer)
    );
}

#[test]
fn time_claims_allow_sixty_seconds_of_skew_and_an_expired_token_is_refused() {
    let key = TestKey::a("k1");
    let clock = TestClock::new(START);
    let (verifier, _, _) = verifier(&[&key], &clock);
    let check = |exp: f64, nbf: Option<f64>, iat: f64| {
        verifier
            .verify(
                &key.token(&claims(clock.now(), None, None, exp, nbf, iat)),
                TEAM,
                AUDIENCE,
            )
            .err()
    };
    assert_eq!(check(-120.0, None, 0.0), Some(JwtError::Expired));
    assert_eq!(
        check(-30.0, None, 0.0),
        None,
        "expired 30 s ago is inside the skew"
    );
    assert_eq!(check(600.0, Some(120.0), 0.0), Some(JwtError::NotYetValid));
    assert_eq!(check(600.0, Some(30.0), 0.0), None);
    assert_eq!(check(600.0, None, 120.0), Some(JwtError::IssuedInFuture));
    assert_eq!(check(600.0, None, 30.0), None);
}

#[test]
fn a_forged_token_is_refused() {
    let key = TestKey::a("k1");
    let impostor = TestKey::b("k1");
    let clock = TestClock::new(START);
    let (verifier, _, _) = verifier(&[&key], &clock);
    let forged = key.token_with(&good_claims(clock.now()), &json!({}), Some(&impostor));
    assert_eq!(
        verifier.verify(&forged, TEAM, AUDIENCE),
        Err(JwtError::BadSignature)
    );

    let good = key.token(&good_claims(clock.now()));
    let mut parts: Vec<String> = good.split('.').map(str::to_string).collect();
    parts[1] = base64url(
        claims(clock.now(), None, None, 99_999.0, None, 0.0)
            .to_string()
            .as_bytes(),
    );
    assert_eq!(
        verifier.verify(&parts.join("."), TEAM, AUDIENCE),
        Err(JwtError::BadSignature),
        "a changed payload breaks the signature"
    );
}

#[test]
fn other_algorithms_and_garbage_are_refused() {
    let key = TestKey::a("k1");
    let clock = TestClock::new(START);
    let (verifier, _, _) = verifier(&[&key], &clock);
    let claims = good_claims(clock.now());
    for alg in ["none", "HS256"] {
        let token = key.token_with(&claims, &json!({"alg": alg}), None);
        assert_eq!(
            verifier.verify(&token, TEAM, AUDIENCE),
            Err(JwtError::UnsupportedAlgorithm)
        );
    }
    assert_eq!(
        verifier.verify("not.a.jwt", TEAM, AUDIENCE),
        Err(JwtError::Malformed)
    );
    assert_eq!(
        verifier.verify("", TEAM, AUDIENCE),
        Err(JwtError::Malformed)
    );
}

#[test]
fn an_unknown_kid_refetches_the_keys_once() {
    let first = TestKey::a("old");
    let rotated = TestKey::b("new");
    let clock = TestClock::new(START);
    let log = FetchLog::new(vec![jwks(&[&first]), jwks(&[&first, &rotated])]);
    let (verifier, set, _) = verifier_with(log, &clock);

    assert!(verifier
        .verify(&first.token(&good_claims(clock.now())), TEAM, AUDIENCE)
        .is_ok());
    assert_eq!(set.fetch_count(), 1);
    assert!(verifier
        .verify(&first.token(&good_claims(clock.now())), TEAM, AUDIENCE)
        .is_ok());
    assert_eq!(set.fetch_count(), 1, "a known kid is served from the cache");

    clock.advance(MINIMUM_REFETCH_MS + 1000);
    assert!(verifier
        .verify(&rotated.token(&good_claims(clock.now())), TEAM, AUDIENCE)
        .is_ok());
    assert_eq!(
        set.fetch_count(),
        2,
        "the unknown kid made one refetch, and the new key was found"
    );
}

#[test]
fn random_kids_cannot_force_refetches_faster_than_the_minimum_interval() {
    let key = TestKey::a("k1");
    let stranger = TestKey::b("nobody");
    let clock = TestClock::new(START);
    let (verifier, set, _) = verifier(&[&key], &clock);
    for _ in 0..2 {
        let token = stranger.token(&good_claims(clock.now()));
        assert_eq!(
            verifier.verify(&token, TEAM, AUDIENCE),
            Err(JwtError::UnknownKey)
        );
    }
    assert_eq!(set.fetch_count(), 1);
    clock.advance(MINIMUM_REFETCH_MS + 1000);
    let _ = verifier.verify(&stranger.token(&good_claims(clock.now())), TEAM, AUDIENCE);
    assert_eq!(set.fetch_count(), 2);
}

#[test]
fn a_failed_fetch_keeps_serving_cached_keys() {
    let key = TestKey::a("k1");
    let clock = TestClock::new(START);
    let (verifier, set, log) = verifier(&[&key], &clock);
    assert!(verifier
        .verify(&key.token(&good_claims(clock.now())), TEAM, AUDIENCE)
        .is_ok());
    log.go_down();
    clock.advance(MINIMUM_REFETCH_MS + 1000);
    let other = TestKey::b("other");
    let token = other.token(&good_claims(clock.now()));
    assert_eq!(
        verifier.verify(&token, TEAM, AUDIENCE),
        Err(JwtError::UnknownKey)
    );
    assert_eq!(
        set.fetch_count(),
        2,
        "the unknown kid tried, and the fetch failed"
    );
    assert!(
        verifier
            .verify(&key.token(&good_claims(clock.now())), TEAM, AUDIENCE)
            .is_ok(),
        "the keys it had are still served"
    );
}
