//! Asking the daemon about Claude Code's hooks: `install.plan` (reads only, works with `install`
//! off) and `install.apply` (a dry run while `install` is off, answering the plan and the off
//! message). The app never edits `settings.json` itself.

use notchling_engine::install::{PAGE_HOOKS, PROMPT_HOOKS};
use notchling_local_api::{ClientError, SOCKET_NAME};
use serde_json::{json, Value};

use super::facts::HookFacts;
use crate::actions::{Socket, Transport};
use crate::chat::data::DataRoot;

pub const OP_PLAN: &str = "install.plan";
pub const OP_APPLY: &str = "install.apply";

/// What is said while there is no daemon to ask.
pub const DETACHED: &str = "Notchling's daemon isn't attached, so the hook can't be checked.";

pub struct Hooks {
    transport: Option<Box<dyn Transport>>,
}

fn line(e: &ClientError) -> String {
    match e {
        ClientError::Api(api) => api.message.clone(),
        ClientError::Closed | ClientError::NotRunning(_) | ClientError::Timeout => DETACHED.into(),
        other => other.to_string(),
    }
}

/// The arguments both ops take: the hooks only (the skills are the Handover view's business).
fn args() -> Value {
    json!({"what": [PROMPT_HOOKS, PAGE_HOOKS]})
}

impl Hooks {
    pub fn over(transport: Box<dyn Transport>) -> Hooks {
        Hooks {
            transport: Some(transport),
        }
    }

    /// No daemon: a render's, and a fixture root's.
    pub fn none() -> Hooks {
        Hooks { transport: None }
    }

    pub fn for_run(root: Option<&DataRoot>) -> Hooks {
        match root.filter(|r| r.fixture.is_none()) {
            Some(root) => Hooks::over(Box::new(Socket {
                socket: root.data_dir.join(SOCKET_NAME),
                version: env!("CARGO_PKG_VERSION").to_string(),
            })),
            None => Hooks::none(),
        }
    }

    fn ask(&self, op: &str) -> Result<HookFacts, String> {
        let Some(transport) = &self.transport else {
            return Err(DETACHED.into());
        };
        let reply = transport.call(op, args()).map_err(|e| line(&e))?;
        HookFacts::parse(&reply).ok_or_else(|| "the daemon's plan isn't one this app reads".into())
    }

    /// What is in the file and what an install would change. Blocking.
    pub fn plan(&self) -> Result<HookFacts, String> {
        self.ask(OP_PLAN)
    }

    /// Asks for the install. While the daemon's `install` switch is off it writes nothing and the
    /// answer carries the plan and the off message. Blocking.
    pub fn apply(&self) -> Result<HookFacts, String> {
        self.ask(OP_APPLY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_local_api::{ApiError, ErrorCode};
    use std::sync::{Arc, Mutex};

    struct Fake {
        reply: Mutex<Result<Value, ()>>,
        seen: Mutex<Vec<(String, Value)>>,
    }

    impl Transport for Arc<Fake> {
        fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
            self.seen.lock().unwrap().push((op.to_owned(), args));
            match &*self.reply.lock().unwrap() {
                Ok(v) => Ok(v.clone()),
                Err(()) => Err(ClientError::Api(ApiError::new(
                    ErrorCode::Failed,
                    "settings.json isn't JSON",
                ))),
            }
        }
    }

    fn fake(reply: Result<Value, ()>) -> (Hooks, Arc<Fake>) {
        let f = Arc::new(Fake {
            reply: Mutex::new(reply),
            seen: Mutex::new(Vec::new()),
        });
        (Hooks::over(Box::new(f.clone())), f)
    }

    fn envelope(on: bool, applied: bool) -> Value {
        json!({"on": on, "applied": applied,
            "message": if on { Value::Null } else { json!("install is off: the Swift app owns hooks and skills until the maintainer turns this on") },
            "plan": {"blocked": null, "skills": [],
                "hooks": {"state": "empty", "refused": null,
                          "changes": ["add PreToolUse (AskUserQuestion): 'x' hook (timeout 300 s)"],
                          "notes": [], "present": []}}})
    }

    #[test]
    fn plan_and_apply_ask_for_the_hooks_only() {
        let (hooks, f) = fake(Ok(envelope(false, false)));
        let plan = hooks.plan().unwrap();
        let applied = hooks.apply().unwrap();
        assert_eq!(plan, applied);
        assert!(!applied.applied && !applied.install_on);
        assert_eq!(applied.changes.len(), 1);
        let seen = f.seen.lock().unwrap();
        assert_eq!(seen[0].0, "install.plan");
        assert_eq!(seen[1].0, "install.apply");
        for (_, args) in seen.iter() {
            assert_eq!(args, &json!({"what": ["prompt_hooks", "page_hooks"]}));
        }
    }

    #[test]
    fn failures_are_lines_not_panics() {
        let (hooks, _) = fake(Err(()));
        assert_eq!(hooks.plan(), Err("settings.json isn't JSON".to_owned()));
        let (odd, _) = fake(Ok(json!({"hello": 1})));
        assert!(odd
            .apply()
            .unwrap_err()
            .contains("isn't one this app reads"));
        assert_eq!(Hooks::none().plan(), Err(DETACHED.to_owned()));
    }
}
