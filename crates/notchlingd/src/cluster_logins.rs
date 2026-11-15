use crate::cluster_node::LocalCalls;
use crate::cluster_redact::TailMasks;
use crate::grant_broker::{Clock, View};
use notchling_engine::cluster::grant_files::GrantValue;
use notchling_engine::cluster::login_files::{valid_token, LoginFile, LoginStore, CLAUDE};
use notchling_engine::cluster::wire::{Origin, OriginTier};
use notchling_local_api::{ApiError, ErrorCode};
use serde_json::{json, Value};
use std::sync::Arc;

pub const LOGIN_DELIVER: &str = "login.deliver";
pub const NODE_OPS: [&str; 1] = [LOGIN_DELIVER];
pub const LOGIN_ENTRY: &str = "claude-worker";

pub trait LoginEnv: Send + Sync {
    fn token(&self) -> Option<GrantValue>;
    fn standing(&self) -> bool;
}

pub struct LoginHold {
    store: Arc<LoginStore>,
    masks: Arc<TailMasks>,
    view: Arc<dyn View>,
    clock: Clock,
}

fn refused(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, message)
}

fn failed(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, message)
}

impl LoginHold {
    pub fn new(
        store: Arc<LoginStore>,
        masks: Arc<TailMasks>,
        view: Arc<dyn View>,
        clock: Clock,
    ) -> Arc<LoginHold> {
        let hold = Arc::new(LoginHold {
            store,
            masks,
            view,
            clock,
        });
        let _ = hold.token();
        hold
    }

    pub fn wipe(&self) {
        let _ = self.store.remove(CLAUDE);
    }

    fn known(&self, origin: &Origin) -> Result<(), ApiError> {
        if origin.tier != OriginTier::Node {
            return Err(refused("only another node does that"));
        }
        let state = self
            .view
            .state()
            .ok_or_else(|| failed("this node is in no cluster"))?;
        if origin.node != state.vault || state.home != state.vault {
            return Err(refused(
                "only the vault node, while it is the primary, delivers a login",
            ));
        }
        Ok(())
    }

    pub fn deliver(&self, origin: &Origin, args: &Value) -> Result<Value, ApiError> {
        self.known(origin)?;
        let usage = "login.deliver takes {tool, value, expiresMs}";
        if args.get("tool").and_then(Value::as_str) != Some(CLAUDE) {
            return Err(ApiError::bad_request(usage));
        }
        let raw = args
            .get("value")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::bad_request(usage))?;
        let expires_ms = args
            .get("expiresMs")
            .and_then(Value::as_i64)
            .ok_or_else(|| ApiError::bad_request(usage))?;
        if !valid_token(raw) {
            return Err(failed("the login isn't a token this machine can use"));
        }
        if expires_ms <= (self.clock)() {
            return Err(failed("the login has already expired"));
        }
        let value = GrantValue::new(raw.to_string());
        self.masks
            .add_standing(vec![(LOGIN_ENTRY.to_string(), value.clone())])
            .map_err(failed)?;
        self.store
            .put(&LoginFile {
                tool: CLAUDE.to_string(),
                expires_ms,
                value,
            })
            .map_err(|why| failed(format!("the login can't be stored: {why}")))?;
        Ok(json!({"stored": CLAUDE}))
    }
}

impl LoginEnv for LoginHold {
    fn token(&self) -> Option<GrantValue> {
        let found = self.store.get(CLAUDE, (self.clock)()).ok().flatten()?;
        self.masks
            .add_standing(vec![(LOGIN_ENTRY.to_string(), found.value.clone())])
            .ok()?;
        Some(found.value)
    }

    fn standing(&self) -> bool {
        self.store.standing(CLAUDE, (self.clock)())
    }
}

pub struct LoginCalls {
    inner: Arc<dyn LocalCalls>,
    hold: Arc<LoginHold>,
}

impl LoginCalls {
    pub fn new(inner: Arc<dyn LocalCalls>, hold: Arc<LoginHold>) -> LoginCalls {
        LoginCalls { inner, hold }
    }
}

impl LocalCalls for LoginCalls {
    fn call(&self, op: &str, args: &Value, origin: &Origin) -> Result<Value, ApiError> {
        match op {
            LOGIN_DELIVER => self.hold.deliver(origin, args),
            _ => self.inner.call(op, args, origin),
        }
    }
}

#[cfg(test)]
#[path = "cluster_logins_tests.rs"]
mod tests;
