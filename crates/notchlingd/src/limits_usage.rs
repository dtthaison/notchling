use notchling_engine::limits::report::UsageReport;
use notchling_engine::limits::sources::ClaudeSource;
use notchling_local_api::ApiError;
use serde_json::{json, Map, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const LIMITS_USAGE: &str = "limits.usage";

const REPLY_WAIT: Duration = Duration::from_secs(4);

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
