use super::crypto::base64_decode;
use ring::signature::{RsaPublicKeyComponents, RSA_PKCS1_2048_8192_SHA256};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub const MAXIMUM_AGE_MS: i64 = 60 * 60 * 1000;
pub const MINIMUM_REFETCH_MS: i64 = 30 * 1000;
pub const SKEW_SECS: f64 = 60.0;

pub type Fetch = Arc<dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync>;
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JwtError {
    Malformed,
    UnsupportedAlgorithm,
    UnknownKey,
    BadSignature,
    WrongAudience,
    WrongIssuer,
    Expired,
    NotYetValid,
    IssuedInFuture,
}

impl std::fmt::Display for JwtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AccessClaims {
    pub audiences: Vec<String>,
    pub issuer: String,
    pub expires_at: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RsaKey {
    pub n: Vec<u8>,
    pub e: Vec<u8>,
}

struct Snapshot {
    keys: HashMap<String, RsaKey>,
    fetched_at: i64,
}

#[derive(Default)]
struct Cache {
    snapshots: HashMap<String, Snapshot>,
    last_attempt: HashMap<String, i64>,
}

pub struct AccessKeySet {
    fetch: Fetch,
    now: Clock,
    cache: Mutex<Cache>,
    fetch_count: AtomicUsize,
}

impl AccessKeySet {
    pub fn new(fetch: Fetch, now: Clock) -> AccessKeySet {
        AccessKeySet {
            fetch,
            now,
            cache: Mutex::new(Cache::default()),
            fetch_count: AtomicUsize::new(0),
        }
    }

    pub fn certs_url(team: &str) -> String {
        format!("https://{team}/cdn-cgi/access/certs")
    }

    pub fn fetch_count(&self) -> usize {
        self.fetch_count.load(Ordering::SeqCst)
    }

    pub fn key(&self, kid: &str, team: &str) -> Option<RsaKey> {
        let current = (self.now)();
        let (hit, stale, attempted) = {
            let cache = self.lock();
            let snapshot = cache.snapshots.get(team);
            (
                snapshot.and_then(|s| s.keys.get(kid).cloned()),
                snapshot.is_none_or(|s| current - s.fetched_at >= MAXIMUM_AGE_MS),
                cache.last_attempt.get(team).copied(),
            )
        };
        if let (Some(hit), false) = (&hit, stale) {
            return Some(hit.clone());
        }
        let keep = if stale { None } else { hit };
        if attempted.is_some_and(|at| current - at < MINIMUM_REFETCH_MS) {
            return keep;
        }
        self.lock().last_attempt.insert(team.to_string(), current);
        self.fetch_count.fetch_add(1, Ordering::SeqCst);
        match (self.fetch)(&Self::certs_url(team)).and_then(|data| parse_jwks(&data)) {
            Ok(keys) => {
                let found = keys.get(kid).cloned();
                self.lock().snapshots.insert(
                    team.to_string(),
                    Snapshot {
                        keys,
                        fetched_at: current,
                    },
                );
                found
            }
            Err(_) => keep,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub fn parse_jwks(data: &[u8]) -> Result<HashMap<String, RsaKey>, String> {
    let value: Value = serde_json::from_slice(data).map_err(|e| e.to_string())?;
    let list = value
        .get("keys")
        .and_then(Value::as_array)
        .ok_or("no keys")?;
    let mut keys = HashMap::new();
    for entry in list {
        let text = |k: &str| entry.get(k).and_then(Value::as_str);
        if text("kty") != Some("RSA") {
            continue;
        }
        let (Some(kid), Some(n), Some(e)) = (
            text("kid"),
            text("n").and_then(base64_decode),
            text("e").and_then(base64_decode),
        ) else {
            continue;
        };
        keys.insert(kid.to_string(), RsaKey { n, e });
    }
    Ok(keys)
}

pub struct AccessJwtVerifier {
    keys: Arc<AccessKeySet>,
    now: Clock,
}

impl AccessJwtVerifier {
    pub fn new(keys: Arc<AccessKeySet>, now: Clock) -> AccessJwtVerifier {
        AccessJwtVerifier { keys, now }
    }

    pub fn verify(
        &self,
        token: &str,
        team: &str,
        audience: &str,
    ) -> Result<AccessClaims, JwtError> {
        let parts: Vec<&str> = token.split('.').collect();
        let [head, body, sig] = parts[..] else {
            return Err(JwtError::Malformed);
        };
        let json =
            |part: &str| base64_decode(part).and_then(|d| serde_json::from_slice::<Value>(&d).ok());
        let (Some(header), Some(payload), Some(signature)) =
            (json(head), json(body), base64_decode(sig))
        else {
            return Err(JwtError::Malformed);
        };
        if !header.is_object() || !payload.is_object() {
            return Err(JwtError::Malformed);
        }
        if header.get("alg").and_then(Value::as_str) != Some("RS256") {
            return Err(JwtError::UnsupportedAlgorithm);
        }
        let key = header
            .get("kid")
            .and_then(Value::as_str)
            .and_then(|kid| self.keys.key(kid, team))
            .ok_or(JwtError::UnknownKey)?;
        let signed = format!("{head}.{body}");
        RsaPublicKeyComponents {
            n: &key.n,
            e: &key.e,
        }
        .verify(&RSA_PKCS1_2048_8192_SHA256, signed.as_bytes(), &signature)
        .map_err(|_| JwtError::BadSignature)?;

        let audiences: Vec<String> = match payload.get("aud") {
            Some(Value::Array(list)) => list
                .iter()
                .filter_map(|a| a.as_str().map(str::to_string))
                .collect(),
            Some(Value::String(one)) => vec![one.clone()],
            _ => Vec::new(),
        };
        if audience.is_empty() || !audiences.iter().any(|a| a == audience) {
            return Err(JwtError::WrongAudience);
        }
        let issuer = payload
            .get("iss")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if issuer != format!("https://{team}") {
            return Err(JwtError::WrongIssuer);
        }
        let number = |k: &str| payload.get(k).and_then(Value::as_f64);
        let exp = number("exp").ok_or(JwtError::Malformed)?;
        let now = (self.now)() as f64 / 1000.0;
        if now > exp + SKEW_SECS {
            return Err(JwtError::Expired);
        }
        if number("nbf").is_some_and(|nbf| now < nbf - SKEW_SECS) {
            return Err(JwtError::NotYetValid);
        }
        if number("iat").is_some_and(|iat| now < iat - SKEW_SECS) {
            return Err(JwtError::IssuedInFuture);
        }
        Ok(AccessClaims {
            audiences,
            issuer,
            expires_at: exp,
        })
    }
}

#[cfg(test)]
#[path = "jwt_tests.rs"]
pub(crate) mod tests;
