//! One Codex turn for a delegated job (Swift `CodexDelegate`): `codex exec --json`, read-only
//! sandbox unless `--write` (then workspace-write, the folder only, with the network when
//! "Allow Network in Codex Turns" is on).
//!
//! Read: `thread.started` names the session, the last non-empty `agent_message` is the reply,
//! `turn.completed` has the token usage, `error` and `turn.failed` say what went wrong.

use super::deps::Deps;
use super::media;
use super::options::{Options, Tool, DEPTH_VARIABLE};
use super::outcome::Outcome;
use crate::tools::codex::plan::{
    arguments, valid_effort, valid_session_id, ExitInfo, Pick, TurnSpec, MAX_PROMPT_BYTES,
    WORKSPACE_WRITE,
};
use crate::tools::codex::process::{Next, Stream, ToolProcess};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

pub const NO_BINARY: &str = "Couldn't find the codex command. Install the Codex CLI first.";
const READ_ONLY: &str = "read-only";
const TICK: Duration = Duration::from_millis(50);

/// What `codex exec --json` printed that the run uses.
#[derive(Default)]
struct Seen {
    session: Option<String>,
    reply: String,
    tokens: Option<u64>,
    last_error: Option<String>,
}

fn read_event(line: &str, seen: &mut Seen) {
    let Ok(Value::Object(event)) = serde_json::from_str::<Value>(line.trim()) else {
        return;
    };
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(String::from);
    let root = Value::Object(event);
    match text(&root, "type").as_deref() {
        Some("thread.started") => {
            if seen.session.is_none() {
                seen.session = text(&root, "thread_id");
            }
        }
        Some("item.completed") => {
            if let Some(item) = root.get("item") {
                if text(item, "type").as_deref() == Some("agent_message") {
                    if let Some(t) = text(item, "text").filter(|t| !t.is_empty()) {
                        seen.reply = t;
                    }
                }
            }
        }
        Some("turn.completed") => {
            if let Some(u) = root.get("usage").filter(|u| u.is_object()) {
                let count = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
                seen.tokens = Some(count("input_tokens") + count("output_tokens"));
            }
        }
        Some("error" | "turn.failed") => {
            let message = text(&root, "message").or_else(|| {
                root.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .map(String::from)
            });
            if message.is_some() {
                seen.last_error = message;
            }
        }
        _ => {}
    }
}

/// Stops the process when the time is up or a stop is asked for: SIGINT first, SIGTERM if it is
/// still running `terminate_after` later. Ends when `finished` is dropped.
fn watch(
    process: Arc<dyn ToolProcess>,
    deps: &Deps,
    timeout: Duration,
    timed_out: Arc<AtomicBool>,
    finished: std::sync::mpsc::Receiver<()>,
) {
    let deadline = Instant::now() + timeout;
    let mut interrupted_at: Option<Instant> = None;
    loop {
        match finished.recv_timeout(TICK) {
            Err(RecvTimeoutError::Timeout) => {}
            _ => return,
        }
        let now = Instant::now();
        match interrupted_at {
            None => {
                let due = now >= deadline;
                if due || deps.stop.load(Ordering::SeqCst) {
                    if due {
                        timed_out.store(true, Ordering::SeqCst);
                    }
                    process.interrupt();
                    interrupted_at = Some(now);
                }
            }
            Some(at) if now.duration_since(at) >= deps.terminate_after => {
                process.terminate();
                return;
            }
            Some(_) => {}
        }
    }
}

pub fn run(options: &Options, deps: &Deps) -> Outcome {
    let started = Instant::now();
    let since = SystemTime::now();
    let mut o = Outcome::new(Tool::Codex);
    o.session = options.session.clone();
    o.model = options.model.clone();
    o.effort = options.effort.clone();
    let Some(codex) = &deps.codex_binary else {
        o.error = Some(NO_BINARY.into());
        return o;
    };
    if let Some(effort) = options.effort.as_deref().filter(|e| !valid_effort(e)) {
        o.error = Some(format!("--effort {effort} isn't a level Codex takes."));
        return o;
    }
    if let Some(session) = options.session.as_deref().filter(|s| !valid_session_id(s)) {
        o.error = Some(format!("{session} isn't a Codex session id."));
        return o;
    }
    // One argument over 128 KB is refused by Linux's execve; macOS takes more.
    if !cfg!(target_os = "macos") && options.prompt.len() > MAX_PROMPT_BYTES {
        o.error = Some(format!(
            "The prompt is over {} KB; put the long part in a file and tell Codex to read it.",
            MAX_PROMPT_BYTES / 1024
        ));
        return o;
    }
    let pick = Pick {
        model: options.model.clone(),
        effort: options.effort.clone(),
    };
    let sandbox = if options.write {
        WORKSPACE_WRITE
    } else {
        READ_ONLY
    };
    let spec = TurnSpec {
        program: codex.clone(),
        args: arguments(
            &options.prompt,
            options.session.as_deref(),
            &options.folder,
            &pick,
            sandbox,
            options.write && deps.network,
        ),
        cwd: options.folder.clone(),
        env: vec![
            ("NO_COLOR".into(), None),
            ("PATH".into(), Some(deps.path.clone())),
            (DEPTH_VARIABLE.into(), Some(deps.depth.to_string())),
        ],
    };
    let process = match deps.codex.spawn(&spec) {
        Ok(p) => p,
        Err(e) => {
            o.error = Some(e.0);
            return o;
        }
    };

    let timed_out = Arc::new(AtomicBool::new(false));
    let (finished_tx, finished_rx) = channel::<()>();
    let watcher = {
        let (process, timed_out, deps) = (process.clone(), timed_out.clone(), deps.clone());
        let timeout = options.timeout;
        std::thread::spawn(move || watch(process, &deps, timeout, timed_out, finished_rx))
    };

    let mut seen = Seen {
        session: options.session.clone(),
        ..Seen::default()
    };
    let mut stderr_line: Option<String> = None;
    let exit: ExitInfo = loop {
        match process.next() {
            Next::Line(line) => match line.stream {
                Stream::Out => read_event(&line.text, &mut seen),
                Stream::Err => {
                    if line.text.contains("rror") {
                        stderr_line = Some(line.text);
                    }
                }
            },
            Next::Exited(info) => break info,
        }
    };
    drop(finished_tx);
    let _ = watcher.join();

    o.session = seen.session;
    o.reply = seen.reply;
    o.tokens = seen.tokens;
    o.timed_out = timed_out.load(Ordering::SeqCst);
    if o.timed_out {
        o.error = Some(format!("Stopped after {} s.", options.timeout.as_secs()));
    } else if exit.code != Some(0) {
        o.error = Some(
            seen.last_error
                .or(stderr_line)
                .unwrap_or_else(|| match exit.code {
                    Some(code) => format!("Codex exited with code {code}."),
                    None => "Codex was stopped.".into(),
                }),
        );
    }
    if let Some(session) = &o.session {
        o.media = media::session_media(&deps.codex_media_root, session, since)
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
    }
    o.seconds = started.elapsed().as_secs_f64();
    o
}

#[cfg(test)]
#[path = "codex_run_tests.rs"]
mod tests;
