//! One Antigravity CLI turn for a delegated job (Swift `AgyDelegate`), over `agy acp`.
//!
//! Nobody is there to answer the tool's permission requests, so they are declined (and listed in
//! the result), except edits in the workspace with `--write`, which also sets accept-edits mode.
//! `initialize` offers no file system or terminal, so any other call from the agent gets `-32601`.
//! What its subagents (the image generator) make is picked up from their own folders; see
//! [`super::progress`] for why the run waits for them.
//!
//! The reply is the agent's text after its last tool call (the last stretch of text that is not
//! empty). The history `session/load` replays is not part of it.

use super::agy_pick;
use super::deps::Deps;
use super::media;
use super::options::{Options, Tool};
use super::outcome::Outcome;
use super::progress::{self, Waited};
use crate::tools::acp::{Connection, Handler, METHOD_NOT_FOUND};
use crate::tools::agy::runner::NO_BINARY;
use crate::tools::agy::{ConfigOption, ConfigValue};
use serde_json::{json, Value};
use std::io::Write;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

const TICK: Duration = Duration::from_millis(50);

#[derive(Default)]
struct State {
    /// `session/load` is replaying history; none of it is this turn's reply.
    loading: bool,
    /// The agent's text since its last tool call.
    segment: String,
    last_segment: String,
    declined: Vec<String>,
    /// The process ended on its own, with its last stderr line.
    exited: Option<String>,
}

impl State {
    fn close_segment(&mut self) {
        let text = self.segment.trim();
        if !text.is_empty() {
            self.last_segment = text.to_string();
        }
        self.segment.clear();
    }
}

/// Hears what the agent says on its own.
struct Listener {
    write: bool,
    state: Mutex<State>,
    conn: OnceLock<Connection>,
}

impl Listener {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn permission(&self, id: &Value, params: &Value) {
        let Some(conn) = self.conn.get() else {
            return;
        };
        let call = params.get("toolCall").unwrap_or(&Value::Null);
        let kind = call.get("kind").and_then(Value::as_str).unwrap_or("");
        let options: &[Value] = params
            .get("options")
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice);
        // The `<prefix>_once` option, else any whose kind starts with the prefix.
        let option = |prefix: &str| -> Option<String> {
            let kind_of = |o: &Value| {
                o.get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string()
            };
            let id_of = |o: &Value| o.get("optionId").and_then(Value::as_str).map(String::from);
            options
                .iter()
                .find(|o| kind_of(o) == format!("{prefix}_once"))
                .and_then(id_of)
                .or_else(|| {
                    options
                        .iter()
                        .find(|o| kind_of(o).starts_with(prefix))
                        .and_then(id_of)
                })
        };
        if self.write && ["edit", "delete", "move"].contains(&kind) {
            if let Some(allow) = option("allow") {
                conn.respond(id, selected(&allow));
                return;
            }
        }
        let title = call
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or(kind)
            .to_string();
        self.lock().declined.push(title);
        match option("reject") {
            Some(reject) => conn.respond(id, selected(&reject)),
            None => conn.respond(id, json!({"outcome": {"outcome": "cancelled"}})),
        }
    }
}

fn selected(option: &str) -> Value {
    json!({"outcome": {"outcome": "selected", "optionId": option}})
}

impl Handler for Listener {
    fn notification(&self, method: &str, params: &Value) {
        if method != "session/update" {
            return;
        }
        let mut state = self.lock();
        if state.loading {
            return;
        }
        let Some(update) = params.get("update") else {
            return;
        };
        match update.get("sessionUpdate").and_then(Value::as_str) {
            Some("agent_message_chunk") => {
                if let Some(text) = update
                    .get("content")
                    .and_then(|c| c.get("text"))
                    .and_then(Value::as_str)
                {
                    state.segment.push_str(text);
                }
            }
            Some("tool_call") => state.close_segment(),
            _ => {}
        }
    }

    fn request(&self, id: &Value, method: &str, params: &Value) {
        if method == "session/request_permission" {
            self.permission(id, params);
        } else if let Some(conn) = self.conn.get() {
            conn.respond_error(id, METHOD_NOT_FOUND, "Not offered in a delegated run.");
        }
    }

    fn exit(&self, message: Option<String>) {
        self.lock().exited = Some(message.unwrap_or_else(|| "Antigravity stopped.".into()));
    }
}

/// Why the run is being cut short.
enum Stop {
    Error(String),
    Timeout,
    Signal,
}

struct Control<'a> {
    deadline: Instant,
    deps: &'a Deps,
}

impl Control<'_> {
    fn cut(&self) -> Option<Stop> {
        if Instant::now() >= self.deadline {
            Some(Stop::Timeout)
        } else if self.deps.stop.load(Ordering::SeqCst) {
            Some(Stop::Signal)
        } else {
            None
        }
    }
}

struct Run<'a> {
    listener: Arc<Listener>,
    conn: Connection,
    ctl: Control<'a>,
}

impl Run<'_> {
    /// A request, waited for. `ends_load` clears the replay flag on the connection's own thread,
    /// before anything that follows the result is heard.
    fn call(&self, method: &str, params: Value, ends_load: bool) -> Result<Value, Stop> {
        let (tx, rx) = channel();
        let listener = self.listener.clone();
        self.conn.request(
            method,
            params,
            Box::new(move |result| {
                if ends_load {
                    listener.lock().loading = false;
                }
                let _ = tx.send(result);
            }),
        );
        loop {
            match rx.recv_timeout(TICK) {
                Ok(Ok(result)) => return Ok(result),
                Ok(Err(e)) => return Err(Stop::Error(e)),
                Err(RecvTimeoutError::Timeout) => {
                    if let Some(stop) = self.ctl.cut() {
                        return Err(stop);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(Stop::Error("Antigravity stopped.".into()))
                }
            }
        }
    }
}

pub fn run(options: &Options, deps: &Deps, progress_out: &mut dyn Write) -> Outcome {
    let started = Instant::now();
    let since = SystemTime::now();
    let mut o = Outcome::new(Tool::Agy);
    let Some(binary) = &deps.agy_binary else {
        o.error = Some(NO_BINARY.into());
        return o;
    };
    let spawned = match deps
        .agy
        .spawn(binary, &["acp".to_string()], &options.folder)
    {
        Ok(s) => s,
        Err(e) => {
            o.error = Some(e);
            return o;
        }
    };
    o.session = options.session.clone();
    let listener = Arc::new(Listener {
        write: options.write,
        state: Mutex::new(State::default()),
        conn: OnceLock::new(),
    });
    let conn = Connection::new(spawned, "Antigravity");
    let _ = listener.conn.set(conn.clone());
    conn.start(listener.clone());
    let run = Run {
        listener: listener.clone(),
        conn: conn.clone(),
        ctl: Control {
            deadline: Instant::now() + options.timeout,
            deps,
        },
    };

    let mut subagents: Vec<String> = Vec::new();
    let result = drive(
        &run,
        options,
        deps,
        &mut o,
        &mut subagents,
        since,
        progress_out,
    );
    if let Err(stop) = result {
        match stop {
            Stop::Error(e) => o.error = Some(e),
            Stop::Timeout | Stop::Signal => {
                o.timed_out = matches!(stop, Stop::Timeout);
                o.error = Some("Stopped.".into());
                cancel(&run, &o, deps);
            }
        }
    }

    let mut state = listener.lock();
    state.close_segment();
    o.reply = state.last_segment.clone();
    o.declined = std::mem::take(&mut state.declined);
    drop(state);
    if o.timed_out {
        o.error = Some(format!("Stopped after {} s.", options.timeout.as_secs()));
    }
    if let Some(session) = o.session.clone() {
        o.media = progress::media_sessions(&session, &subagents)
            .iter()
            .flat_map(|id| media::session_media(&deps.agy_media_root, id, since))
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
    }
    o.seconds = started.elapsed().as_secs_f64();
    conn.close();
    o
}

/// Asks the agent to cancel its turn and gives it a moment to end cleanly.
fn cancel(run: &Run<'_>, o: &Outcome, deps: &Deps) {
    if let Some(session) = &o.session {
        run.conn
            .notify("session/cancel", json!({"sessionId": session}));
    }
    let until = Instant::now() + deps.cancel_grace;
    while Instant::now() < until && run.listener.lock().exited.is_none() {
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn drive(
    run: &Run<'_>,
    options: &Options,
    deps: &Deps,
    o: &mut Outcome,
    subagents: &mut Vec<String>,
    since: SystemTime,
    progress_out: &mut dyn Write,
) -> Result<(), Stop> {
    run.call(
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientCapabilities": {"fs": {"readTextFile": false, "writeTextFile": false}, "terminal": false},
            "clientInfo": {"name": "Notchling", "version": "1"},
        }),
        false,
    )?;

    // `session/load` replays the history first; none of it is this turn's reply.
    let opened = match &options.session {
        Some(id) => {
            run.listener.lock().loading = true;
            run.call(
                "session/load",
                json!({"sessionId": id, "cwd": options.folder, "mcpServers": []}),
                true,
            )?
        }
        None => run.call(
            "session/new",
            json!({"cwd": options.folder, "mcpServers": []}),
            false,
        )?,
    };
    if let Some(id) = opened.get("sessionId").and_then(Value::as_str) {
        o.session = Some(id.to_string());
    }
    let session = o
        .session
        .clone()
        .ok_or_else(|| Stop::Error("Antigravity didn't open a session.".into()))?;

    configure(
        run,
        options,
        o,
        &session,
        &ConfigOption::parse(opened.get("configOptions")),
    )?;

    let answer = run.call(
        "session/prompt",
        json!({"sessionId": session, "prompt": [{"type": "text", "text": options.prompt}]}),
        false,
    )?;
    o.tokens = answer
        .get("usage")
        .and_then(|u| u.get("totalTokens"))
        .and_then(Value::as_u64);
    let reason = answer
        .get("stopReason")
        .and_then(Value::as_str)
        .unwrap_or("end_turn");
    if reason != "end_turn" {
        return Err(Stop::Error(format!("Antigravity stopped ({reason}).")));
    }
    await_subagents(run, deps, &session, since, subagents, progress_out)
}

/// Sets model, thinking level and (with `--write`) accept-edits, one after another.
fn configure(
    run: &Run<'_>,
    options: &Options,
    o: &mut Outcome,
    session: &str,
    config: &[ConfigOption],
) -> Result<(), Stop> {
    let find = |id: &str| config.iter().find(|c| c.id == id);
    let models: Vec<String> = find("model")
        .map(|c| {
            c.values
                .iter()
                .map(|v: &ConfigValue| v.value.clone())
                .collect()
        })
        .unwrap_or_default();
    let (model, level) =
        agy_pick::pick(options.model.as_deref(), options.effort.as_deref(), &models);
    let mut steps: Vec<(&str, &str, String)> = Vec::new();
    if let Some(m) = &model {
        steps.push(("model", "--model", m.clone()));
    }
    if let Some(e) = &level {
        steps.push(("thought_level", "--effort", e.clone()));
    }
    if options.write {
        steps.push(("mode", "--write", "accept-edits".into()));
    }
    for (id, flag, value) in &steps {
        let Some(option) = find(id) else {
            continue;
        };
        if !option.values.iter().any(|v| &v.value == value) {
            let choices: Vec<&str> = option.values.iter().map(|v| v.value.as_str()).collect();
            return Err(Stop::Error(format!(
                "{flag} {value} isn't offered. Choices: {}.",
                choices.join(", ")
            )));
        }
    }
    let current = |id: &str| {
        find(id)
            .map(|c| c.current.clone())
            .filter(|c| !c.is_empty())
    };
    o.model = model.or_else(|| current("model"));
    o.effort = level.or_else(|| current("thought_level"));
    for (id, _, value) in steps {
        run.call(
            "session/set_config_option",
            json!({"sessionId": session, "configId": id, "value": value}),
            false,
        )
        .map_err(|stop| match stop {
            Stop::Error(e) => Stop::Error(format!("Couldn't set {id}: {e}")),
            other => other,
        })?;
    }
    Ok(())
}

/// The turn may have left subagents working (the image generator does). Closing agy now would kill
/// them, so wait, woken by changes to the session's files, until they report and agy answers.
fn await_subagents(
    run: &Run<'_>,
    deps: &Deps,
    session: &str,
    since: SystemTime,
    subagents: &mut Vec<String>,
    progress_out: &mut dyn Write,
) -> Result<(), Stop> {
    let Some(folder) = media::session_folder(&deps.agy_media_root, session) else {
        return Ok(());
    };
    let system = folder.join(".system_generated");
    let stopped = || run.ctl.cut().is_some() || run.listener.lock().exited.is_some();
    let waited = progress::wait(&system, since, &stopped, &mut |n| {
        let _ = writeln!(progress_out, "Waiting for Antigravity's subagents ({n})…");
    });
    match waited {
        Waited::Done {
            text,
            subagents: ids,
        } => {
            *subagents = ids;
            if let Some(text) = text {
                let mut state = run.listener.lock();
                state.last_segment = text;
                state.segment.clear();
            }
            Ok(())
        }
        Waited::Stopped { subagents: ids } => {
            *subagents = ids;
            Err(run.ctl.cut().unwrap_or_else(|| {
                let tail = run.listener.lock().exited.clone();
                Stop::Error(tail.unwrap_or_else(|| "Antigravity stopped.".into()))
            }))
        }
        Waited::CannotWatch => Err(Stop::Error(format!(
            "Couldn't watch {} for the subagents' results.",
            system.display()
        ))),
    }
}

#[cfg(test)]
#[path = "agy_run_tests.rs"]
mod tests;
