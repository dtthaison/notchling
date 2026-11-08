//! The hook's side of [`ActingOps`]: `hook.request`, and who may make it.
//!
//! The hook is who its process is. Its pid comes from the socket's peer credentials
//! ([`Caller::pid`]), never from its arguments, and that process must descend from the Claude Code
//! process whose `sessions/<pid>.json` (in the main config folder or a lane's) holds the event's
//! `session_id`: the parent pids are walked with `proc_pidinfo` on macOS and `/proc/<pid>/stat` on
//! Linux ([`crate::hook::parent_pid`]). Anything else is `forbidden`, and `notchling hook` passes
//! on it. An agent inside that same session still descends from it; what this stops is a process
//! answering for a session it doesn't belong to, and a pid it names for itself.

use crate::acting_ops::{bad, forbidden, now_ms, ActingOps, HOOK_WAIT};
use crate::hook_output::is_prompt_event;
use crate::prompts::pass;
use notchling_engine::sessions::config_dirs;
use notchling_local_api::{topics, ApiError, Caller, ClientKind};
use serde_json::Value;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

/// How far up from the hook's process its Claude Code is looked for (Claude Code runs a hook
/// through a shell or two).
pub const LINEAGE_DEPTH: usize = 16;

/// A process's parent: the kernel's (`proc_pidinfo` on macOS, `/proc/<pid>/stat` on Linux),
/// swapped in tests.
pub type Parents = Box<dyn Fn(u32) -> Option<u32> + Send + Sync>;

impl ActingOps {
    /// Finds a process's parent with `parents` (tests).
    pub fn with_parents(mut self, parents: Parents) -> ActingOps {
        self.parent_of = parents;
        self
    }

    /// Whether `pid` descends from the Claude Code process whose `sessions/<pid>.json`, in any
    /// config folder, holds `session_id`.
    fn started_by_session(&self, pid: u32, session_id: &str) -> bool {
        if session_id.is_empty() {
            return false;
        }
        let folders: Vec<std::path::PathBuf> = config_dirs(&self.roots)
            .into_iter()
            .map(|c| c.dir.join("sessions"))
            .collect();
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

    /// Holds a hook's prompt until the app decides, the hook's deadline, the app leaving or the
    /// hook's process going; every way but a decision is a pass. Only a client that said hello as
    /// the hook, from a process the event's Claude Code session started, gets here.
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
        // Who the hook is comes from the kernel, and it must be this session's.
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
            // Decided just now: take the decision.
            return Ok(rx.try_recv().unwrap_or_else(|_| pass(why)));
        }
    }
}

/// Whether the Claude Code session file at `path` names `session_id`.
fn session_file_holds(path: &std::path::Path, session_id: &str) -> bool {
    std::fs::read(path)
        .ok()
        .and_then(|data| serde_json::from_slice::<Value>(&data).ok())
        .is_some_and(|s| s.get("sessionId").and_then(Value::as_str) == Some(session_id))
}

/// The hook's process still runs.
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
