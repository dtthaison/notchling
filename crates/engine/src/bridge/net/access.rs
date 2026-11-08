//! `TunnelAccess`: the Cloudflare tunnel and Access settings (`bridge/access.json`, 0600), the gate
//! that tells a tunnel client from a local one at the WebSocket upgrade, and the pairing QR's text.
//!
//! Tunnel traffic reaches the computer from loopback (cloudflared), so Cloudflare's headers are the
//! only way to tell it apart from a local client. The client secret lives only in that file and is
//! never logged or returned by a status.

use super::jwt::{AccessJwtVerifier, Clock};
use super::limiter::{AuthRateLimiter, Verdict};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The file's name in the bridge folder.
pub const FILE_NAME: &str = "access.json";
pub const DEFAULT_HOSTNAME: &str = "console.trietkun.shop";
pub const DEFAULT_TEAM_DOMAIN: &str = "bold-dew-100f.cloudflareaccess.com";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AccessConfig {
    pub enabled: bool,
    pub hostname: String,
    pub team_domain: String,
    pub audience: String,
    pub client_id: String,
    pub client_secret: String,
}

impl Default for AccessConfig {
    fn default() -> Self {
        AccessConfig {
            enabled: false,
            hostname: DEFAULT_HOSTNAME.into(),
            team_domain: DEFAULT_TEAM_DOMAIN.into(),
            audience: String::new(),
            client_id: String::new(),
            client_secret: String::new(),
        }
    }
}

impl AccessConfig {
    /// Everything the computer needs to verify tunnel clients and the phone needs to reach it.
    pub fn is_complete(&self) -> bool {
        [
            &self.hostname,
            &self.team_domain,
            &self.audience,
            &self.client_id,
            &self.client_secret,
        ]
        .iter()
        .all(|f| !f.trim().is_empty())
    }

    /// Whether the pairing QR carries a tunnel block.
    pub fn offers_tunnel(&self) -> bool {
        self.enabled && self.is_complete()
    }

    fn tunnel_payload(&self) -> Option<Value> {
        self.offers_tunnel().then(|| {
            json!({
                "url": format!("wss://{}", self.hostname.trim()),
                "accessClientId": self.client_id,
                "accessClientSecret": self.client_secret,
            })
        })
    }

    /// Reads `file` without writing anything: absent or unreadable is the defaults.
    pub fn read(file: &Path) -> AccessConfig {
        std::fs::read(file)
            .ok()
            .and_then(|d| serde_json::from_slice(&d).ok())
            .unwrap_or_default()
    }
}

/// `access.json`, cached.
pub struct AccessConfigStore {
    file: PathBuf,
    cached: Mutex<AccessConfig>,
}

impl AccessConfigStore {
    pub fn open(file: PathBuf) -> AccessConfigStore {
        let cached = Mutex::new(AccessConfig::read(&file));
        AccessConfigStore { file, cached }
    }

    pub fn config(&self) -> AccessConfig {
        self.lock().clone()
    }

    /// Applies `change` and writes the file when it changed (sorted keys, pretty, as Swift).
    pub fn update(&self, change: impl FnOnce(&mut AccessConfig)) -> std::io::Result<()> {
        let mut cached = self.lock();
        let mut next = cached.clone();
        change(&mut next);
        if next == *cached {
            return Ok(());
        }
        let value =
            super::files::sorted(serde_json::to_value(&next).map_err(std::io::Error::other)?);
        let text = serde_json::to_string_pretty(&value).map_err(std::io::Error::other)?;
        super::files::write_private(&self.file, text.as_bytes())?;
        *cached = next;
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, AccessConfig> {
        self.cached.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Headers as (name, value) pairs, names compared ignoring case.
pub type Headers = [(String, String)];

pub fn header<'a>(name: &str, headers: &'a Headers) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.trim())
}

/// A request that carries Cloudflare's marks came through the tunnel.
pub fn is_tunnel_origin(headers: &Headers) -> bool {
    header("Cf-Connecting-Ip", headers).is_some() || header("Cf-Ray", headers).is_some()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// No Cloudflare headers: LAN or loopback.
    Local,
    /// Came through the tunnel with a valid Access token; the real client's address.
    Tunnel(String),
    Reject(String),
}

/// A refusal, with the HTTP status to answer it with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub status: u16,
    pub reason: String,
}

/// Used when a tunnel request has no `Cf-Connecting-Ip`.
pub const UNKNOWN_CLIENT: &str = "cf-unknown";

pub struct TunnelGate {
    config: Box<dyn Fn() -> AccessConfig + Send + Sync>,
    verifier: AccessJwtVerifier,
    limiter: Arc<AuthRateLimiter>,
    now: Clock,
}

impl TunnelGate {
    pub fn new(
        config: Box<dyn Fn() -> AccessConfig + Send + Sync>,
        verifier: AccessJwtVerifier,
        limiter: Arc<AuthRateLimiter>,
        now: Clock,
    ) -> TunnelGate {
        TunnelGate {
            config,
            verifier,
            limiter,
            now,
        }
    }

    pub fn evaluate(&self, headers: &Headers) -> Decision {
        if !is_tunnel_origin(headers) {
            return Decision::Local;
        }
        match self.evaluate_tunnel(headers) {
            Ok(client) => Decision::Tunnel(client),
            Err(refusal) => Decision::Reject(refusal.reason),
        }
    }

    /// The checks for a request that carries Cloudflare headers.
    pub fn evaluate_tunnel(&self, headers: &Headers) -> Result<String, Refusal> {
        let refuse = |status: u16, reason: String| Err(Refusal { status, reason });
        let settings = (self.config)();
        if !settings.enabled {
            return refuse(403, "internet access is off".into());
        }
        if !settings.is_complete() {
            return refuse(403, "internet access is not fully configured".into());
        }
        let client = header("Cf-Connecting-Ip", headers)
            .filter(|c| !c.is_empty())
            .unwrap_or(UNKNOWN_CLIENT)
            .to_string();
        if let Verdict::Blocked(_) = self.limiter.check(&client, (self.now)()) {
            return refuse(429, format!("{client} is locked out"));
        }
        let Some(token) = header("Cf-Access-Jwt-Assertion", headers).filter(|t| !t.is_empty())
        else {
            return refuse(401, "missing Cf-Access-Jwt-Assertion".into());
        };
        match self
            .verifier
            .verify(token, &settings.team_domain, &settings.audience)
        {
            Ok(_) => Ok(client),
            Err(why) => refuse(401, format!("access token refused: {why}")),
        }
    }
}

/// The text inside the pairing QR. `v` stays 1 and every v1 field is kept, so an app that predates
/// the tunnel still parses it and ignores `tunnel`.
pub fn pairing_payload(
    name: &str,
    port: u16,
    fingerprint: &str,
    hosts: &[String],
    code: &str,
    access: &AccessConfig,
) -> String {
    let mut object = Map::new();
    object.insert("v".into(), json!(1));
    object.insert("name".into(), json!(name));
    object.insert("port".into(), json!(port));
    object.insert("fp".into(), json!(fingerprint));
    object.insert("hosts".into(), json!(hosts));
    object.insert("code".into(), json!(code));
    if let Some(tunnel) = access.tunnel_payload() {
        object.insert("tunnel".into(), tunnel);
    }
    super::files::sorted(Value::Object(object)).to_string()
}

#[cfg(test)]
#[path = "access_tests.rs"]
pub(crate) mod tests;
