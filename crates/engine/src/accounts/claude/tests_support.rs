//! A world for the store's tests: temp folders, an in-memory Keychain, scripted HTTP, a fixed clock.
//! Every value is an obviously fake one.

use super::*;
use crate::accounts::fake::{MemoryKeychain, ScriptedHttp};
use crate::accounts::http::PROFILE_URL;
use crate::sessions::testutil::TempDir;
use serde_json::Value;
use std::sync::atomic::{AtomicI64, Ordering};

pub const NOW: i64 = 1_800_000_000_000;
pub const LATER: i64 = NOW + 3_600_000;

pub struct World {
    pub tmp: TempDir,
    pub chain: Arc<MemoryKeychain>,
    pub http: Arc<ScriptedHttp>,
    pub clock: Arc<AtomicI64>,
    pub store: ClaudeAccounts,
}

pub fn id(n: u8) -> String {
    format!("a{n}a{n}:0{n}0{n}")
}

pub fn email(n: u8) -> String {
    format!("user{n}@example.com")
}

/// `claudeAiOauth` for account `n`, version `v` of its tokens.
pub fn oauth(n: u8, v: u8, expires: i64) -> Value {
    json!({
        "accessToken": format!("test-access-{n}-{v}"),
        "refreshToken": format!("test-refresh-{n}-{v}"),
        "expiresAt": expires,
        "scopes": ["user:inference", "user:profile"],
    })
}

pub fn oauth_account(n: u8) -> Value {
    let (a, o) = id(n)
        .split_once(':')
        .map(|(a, o)| (a.to_string(), o.to_string()))
        .unwrap_or_default();
    json!({"accountUuid": a, "emailAddress": email(n), "organizationUuid": o,
           "organizationType": "claude_max", "organizationRateLimitTier": "default_claude_max_5x"})
}

pub fn profile_body(n: u8) -> String {
    let (a, o) = id(n)
        .split_once(':')
        .map(|(a, o)| (a.to_string(), o.to_string()))
        .unwrap_or_default();
    json!({"account": {"uuid": a, "email": email(n)},
           "organization": {"uuid": o, "name": "Org", "organization_type": "claude_max",
                            "rate_limit_tier": "default_claude_max_5x"}})
    .to_string()
}

impl World {
    pub fn new(tag: &str) -> World {
        World::over(tag, |chain| chain)
    }

    /// A world whose Keychain runner is `wrap` around the in-memory one (a gate, say).
    pub fn over(
        tag: &str,
        wrap: impl FnOnce(Arc<MemoryKeychain>) -> Arc<dyn crate::accounts::keychain::SecurityRunner>,
    ) -> World {
        let tmp = TempDir::new(tag);
        let chain = Arc::new(MemoryKeychain::new());
        let runner = wrap(chain.clone());
        let http = Arc::new(ScriptedHttp::new());
        let clock = Arc::new(AtomicI64::new(NOW));
        let now = clock.clone();
        let store = ClaudeAccounts::new(ClaudeEnv {
            keys: Keys {
                runner,
                user: "me".into(),
            },
            http: http.clone(),
            now_ms: Arc::new(move || now.load(Ordering::SeqCst)),
            config_file: tmp.path("home/.claude.json"),
            list_file: tmp.path("data/daemon/accounts.json"),
            lanes: LaneStore::new(&tmp.path("home"), &tmp.path("data")),
        });
        std::fs::create_dir_all(tmp.path("home/.claude")).ok();
        World {
            tmp,
            chain,
            http,
            clock,
            store,
        }
    }

    /// Claude Code signed in to account `n` (its item, with an MCP login beside it, and
    /// `~/.claude.json`).
    pub fn signed_in(&self, n: u8, v: u8) {
        let item = json!({"claudeAiOauth": oauth(n, v, LATER), "mcpOAuth": {"srv": {"accessToken": "test-mcp"}}});
        self.chain.put(LIVE_SERVICE, "me", &item.to_string());
        let config = format!(
            "{{\n  \"numStartups\": 4,\n  \"oauthAccount\": {},\n  \"projects\": {{\"/w\": {{\"cost\": 1.50}}}}\n}}\n",
            oauth_account(n)
        );
        self.tmp.write("home/.claude.json", &config);
    }

    /// A saved slot for account `n` with tokens version `v`.
    pub fn saved(&self, n: u8, v: u8, expires: i64) {
        let slot = json!({"claudeAiOauth": oauth(n, v, expires), "oauthAccount": oauth_account(n)});
        self.chain.put(SAVED_SERVICE, &id(n), &slot.to_string());
        let mut list = self.store.saved_list();
        list.retain(|a| a.id != id(n));
        list.push(ClaudeAccount {
            id: id(n),
            email: email(n),
            plan: "Max 5x".into(),
            needs_login: false,
        });
        std::fs::create_dir_all(self.tmp.path("data/daemon")).ok();
        std::fs::write(
            self.tmp.path("data/daemon/accounts.json"),
            serde_json::to_vec(&list).unwrap_or_default(),
        )
        .ok();
    }

    pub fn answers_profile(&self, n: u8) {
        self.http.answer(PROFILE_URL, 200, &profile_body(n));
    }

    pub fn live_item(&self) -> Value {
        serde_json::from_str(&self.chain.get(LIVE_SERVICE, "me").unwrap_or_default())
            .unwrap_or(Value::Null)
    }

    pub fn saved_item(&self, n: u8) -> Value {
        serde_json::from_str(&self.chain.get(SAVED_SERVICE, &id(n)).unwrap_or_default())
            .unwrap_or(Value::Null)
    }

    pub fn config_text(&self) -> String {
        std::fs::read_to_string(self.tmp.path("home/.claude.json")).unwrap_or_default()
    }

    /// Every request sent to the token endpoint, with the refresh token it carried.
    pub fn token_requests(&self) -> usize {
        self.http
            .seen()
            .iter()
            .filter(|(url, _)| url.contains("/oauth/token"))
            .count()
    }
}
