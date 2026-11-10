//! `limits.usage`: every saved Claude Code account's limits, each read with that account's own
//! token (the watcher's [`ClaudeSource`], which reads the main login's and a lane's token and
//! never renews them; only a saved login live nowhere is renewed).
//!
//! | Op | Tier | Args | Reply |
//! | --- | --- | --- | --- |
//! | `limits.usage` | app | `{refresh?: bool}` | see below |
//!
//! ```json
//! {"accounts_on": true,
//!  "reports": {"<account id>": {"fetched": 1794150245.0, "groups": [{"name": null, "bars": [...]}]}},
//!  "errors": {"<account id>": "The saved login has expired. Add the account again to sign in."},
//!  "refreshing": true}
//! ```
//!
//! - `reports` has the last limits read for each saved account, in the shape of `providers.usage`'s;
//!   `errors` says why the last read of an account failed (an account may have both: the report
//!   is the last good one). An account in neither has not been read yet.
//! - `refresh: true` reads every account now, not just those due, on a thread of its own and
//!   waits for it a few seconds; `refreshing` is true while a read is still going (ask again with
//!   no `refresh`). A call made while one runs starts no second.
//! - With `accounts` off nothing is read and the lists are empty.

use notchling_engine::limits::report::UsageReport;
use notchling_engine::limits::sources::ClaudeSource;
use notchling_local_api::ApiError;
use serde_json::{json, Map, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const LIMITS_USAGE: &str = "limits.usage";

/// How long a reply waits for a refresh (the client gives a call up at 5 s).
const REPLY_WAIT: Duration = Duration::from_secs(4);

/// Clears the flag when the refresh thread ends, however it ends.
struct Done(Arc<AtomicBool>);

impl Drop for Done {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

pub struct ClaudeUsage {
    source: Mutex<Option<Arc<dyn ClaudeSource>>>,
    syncing: Arc<AtomicBool>,
    wait: Duration,
}

impl Default for ClaudeUsage {
    fn default() -> Self {
        ClaudeUsage {
            source: Mutex::new(None),
            syncing: Arc::new(AtomicBool::new(false)),
            wait: REPLY_WAIT,
        }
    }
}

impl ClaudeUsage {
    pub fn set_source(&self, source: Arc<dyn ClaudeSource>) {
        *self.source.lock().unwrap_or_else(|e| e.into_inner()) = Some(source);
    }

    /// How long a reply waits for a refresh (tests).
    pub fn with_wait(mut self, wait: Duration) -> ClaudeUsage {
        self.wait = wait;
        self
    }

    pub fn call(&self, accounts_on: bool, args: &Value) -> Result<Value, ApiError> {
        let source = self
            .source
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let Some(source) = source.filter(|_| accounts_on) else {
            return Ok(json!({"accounts_on": accounts_on, "reports": {}, "errors": {}}));
        };
        let refresh = args.get("refresh").and_then(Value::as_bool) == Some(true);
        if refresh && !self.syncing.swap(true, Ordering::SeqCst) {
            let (tx, rx) = channel();
            let (worker, done) = (source.clone(), Done(self.syncing.clone()));
            let spawned = std::thread::Builder::new()
                .name("limits-usage".into())
                .spawn(move || {
                    worker.sync_usage_now();
                    // Before the send, so a reply made right after sees it finished.
                    drop(done);
                    let _ = tx.send(());
                });
            if spawned.is_ok() {
                let _ = rx.recv_timeout(self.wait);
            } else {
                self.syncing.store(false, Ordering::SeqCst);
            }
        }
        let (mut reports, mut errors) = (Map::new(), Map::new());
        for account in source.accounts() {
            if let Some(usage) = source.usage(&account.id) {
                let report = UsageReport::from_account(&usage);
                if let Ok(value) = serde_json::to_value(report) {
                    reports.insert(account.id.clone(), value);
                }
            }
            if let Some(why) = source.usage_error(&account.id) {
                errors.insert(account.id, json!(why));
            }
        }
        let mut reply = json!({"accounts_on": true, "reports": reports, "errors": errors});
        if self.syncing.load(Ordering::SeqCst) {
            reply["refreshing"] = json!(true);
        }
        Ok(reply)
    }
}

#[cfg(test)]
#[path = "limits_usage_tests.rs"]
mod tests;
