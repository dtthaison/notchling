use crate::acting_ops::{bad, forbidden, now_ms, ActingOps, HOOK_WAIT};
use crate::hook_output::is_prompt_event;
use crate::prompts::pass;
use notchling_engine::sessions::config_dirs;
use notchling_local_api::{topics, ApiError, Caller, ClientKind};
use serde_json::Value;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

pub const LINEAGE_DEPTH: usize = 16;

pub type Parents = Box<dyn Fn(u32) -> Option<u32> + Send + Sync>;

impl ActingOps {
    pub fn with_parents(mut self, parents: Parents) -> ActingOps {
        self.parent_of = parents;
        self
    }

    fn session_folders(&self) -> Vec<std::path::PathBuf> {
        config_dirs(&self.roots)
            .into_iter()
            .map(|c| c.dir.join("sessions"))
            .collect()
    }

    fn started_by_session(&self, pid: u32, session_id: &str) -> bool {
        if session_id.is_empty() {
            return false;
        }
        let folders = self.session_folders();
        let mut next = (self.parent_of)(pid);
        for _ in 0..LINEAGE_DEPTH {
            let Some(p) = next.filter(|p| *p > 1) else {
                return false;
            };
            if folders
                .iter()
                .any(|f| session_file_holds(&f.join(format!("{p}.json")), session_id))
            {
                return true;
            }
            next = (self.parent_of)(p);
        }
        false
    }

    pub(crate) fn session_of(&self, pid: u32) -> Option<String> {
        let folders = self.session_folders();
        let mut next = (self.parent_of)(pid);
        for _ in 0..LINEAGE_DEPTH {
            let p = next.filter(|p| *p > 1)?;
            if let Some(id) = folders
                .iter()
                .find_map(|f| session_id_in(&f.join(format!("{p}.json"))))
            {
                return Some(id);
            }
            next = (self.parent_of)(p);
        }
        None
    }

    pub(crate) fn hook_request(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        if caller.client != ClientKind::Hook {
            return Err(forbidden(
                "hook.request is only for `notchling hook` (hello as client \"hook\")",
            ));
        }
        let event = args
            .get("event")
            .filter(|e| e.is_object())
            .ok_or_else(|| bad("hook.request takes {\"event\": {...}}"))?;
        if !self.acting.is_on() {
            return Ok(pass("acting is off"));
        }
        if !is_prompt_event(event) {
            return Ok(pass("not a question or permission"));
        }
        let session = event
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        let Some(pid) = caller
            .pid
            .filter(|pid| self.started_by_session(*pid, session))
        else {
            return Err(forbidden(
                "hook.request comes only from a hook the event's Claude Code session started",
            ));
        };
        if !self.room.can_answer() {
            return Ok(pass("no app is attached"));
        }
        let wait = args
            .get("waitMs")
            .and_then(Value::as_u64)
            .map(Duration::from_millis)
            .map_or(HOOK_WAIT, |w| w.min(HOOK_WAIT));
        let Some((prompt, rx)) = self.prompts.open(event.clone(), Some(pid), now_ms()) else {
            return Ok(pass("the daemon is stopping"));
        };
        self.room
            .publish(topics::PROMPTS, topics::PROMPT_PENDING, &prompt.to_json());
        let started = Instant::now();
        loop {
            match rx.recv_timeout(self.tick) {
                Ok(decision) => return Ok(decision),
                Err(RecvTimeoutError::Disconnected) => return Ok(pass("closed")),
                Err(RecvTimeoutError::Timeout) => {}
            }
            let why = if started.elapsed() >= wait {
                "timeout"
            } else if !self.room.can_answer() {
                "no_app"
            } else if !alive(pid) {
                "hook_gone"
            } else {
                continue;
            };
            if self.prompts.forget(&prompt.id).is_some() {
                self.resolved(&prompt, "pass", why);
                return Ok(pass(why));
            }
            return Ok(rx.try_recv().unwrap_or_else(|_| pass(why)));
        }
    }
}

fn session_id_in(path: &std::path::Path) -> Option<String> {
    std::fs::read(path)
        .ok()
        .and_then(|data| serde_json::from_slice::<Value>(&data).ok())
        .and_then(|s| {
            s.get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .filter(|id| !id.is_empty())
}

fn session_file_holds(path: &std::path::Path, session_id: &str) -> bool {
    session_id_in(path).as_deref() == Some(session_id)
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 only asks whether the process exists.
    let r = unsafe { libc::kill(pid, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn alive(_pid: u32) -> bool {
    true
}
