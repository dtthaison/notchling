//! Accounts ops that can outlast their call. A switch can wait up to 120 s on a Keychain prompt,
//! the profile endpoint 15 s, and Move All stops and resumes every lane session, but a client gives
//! up on a call after 5 s. So each such op runs on its own thread ([`late_reply`]) and the reply
//! waits [`REPLY_WAIT`]: an op done by then answers as before; one still going answers
//! `{state: "working", ticket, op}`, and its result is kept under `ticket` for `accounts.result`
//! and published on `accounts` as `accounts.done {ticket, op, ok, result?, error?}` (Move All's
//! per-session errors are in its `result.errors`). The result is never lost: the app asks
//! `accounts.result` until it has it, and a client that reconnected still gets the event.

use crate::acting_ops::{bad, Room};
use crate::late_reply;
use notchling_local_api::{topics, ApiError, ErrorCode};
use serde_json::{json, Value};
use std::collections::{BTreeSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How long a call waits for its op, under a client's 5 s call timeout.
pub const REPLY_WAIT: Duration = Duration::from_millis(4500);
/// How long `accounts.result` waits for a ticket that is still running.
pub const RESULT_WAIT: Duration = Duration::from_millis(4000);
/// Results kept for `accounts.result`, newest last.
const KEPT: usize = 32;

#[derive(Default)]
struct Book {
    running: BTreeSet<u64>,
    done: VecDeque<(u64, Result<Value, ApiError>)>,
}

/// The ops still running and the results not yet asked for.
#[derive(Default)]
pub struct Tickets {
    next: AtomicU64,
    book: Mutex<Book>,
    ended: Condvar,
}

impl Tickets {
    fn book(&self) -> MutexGuard<'_, Book> {
        self.book.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn start(&self) -> u64 {
        let ticket = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        self.book().running.insert(ticket);
        ticket
    }

    /// The op answered its own call: nothing to keep.
    fn answered(&self, ticket: u64) {
        self.book().running.remove(&ticket);
    }

    fn finish(&self, ticket: u64, result: Result<Value, ApiError>) {
        let mut book = self.book();
        book.running.remove(&ticket);
        book.done.push_back((ticket, result));
        while book.done.len() > KEPT {
            book.done.pop_front();
        }
        drop(book);
        self.ended.notify_all();
    }

    /// The result for `ticket`, waiting up to `wait` while it runs: `Ok(None)` when it still runs.
    pub fn result(&self, ticket: u64, wait: Duration) -> Result<Option<Value>, ApiError> {
        let deadline = Instant::now() + wait;
        let mut book = self.book();
        loop {
            if let Some(i) = book.done.iter().position(|(t, _)| *t == ticket) {
                let (_, result) = book.done.remove(i).unwrap_or((ticket, Ok(Value::Null)));
                return result.map(Some);
            }
            if !book.running.contains(&ticket) {
                return Err(ApiError::new(
                    ErrorCode::Failed,
                    "No accounts job has that ticket (its result may have been read already).",
                ));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            book = self
                .ended
                .wait_timeout(book, left)
                .map(|(b, _)| b)
                .unwrap_or_else(|e| e.into_inner().0);
        }
    }
}

/// What a call answers while its op still runs.
pub fn working(ticket: u64, op: &str) -> Value {
    json!({"state": "working", "ticket": ticket, "op": op})
}

/// Runs `job` for `op`: its result if it ends within `wait`, else [`working`] and the result later
/// on `accounts.done` and through `accounts.result`.
pub fn run(
    tickets: &Arc<Tickets>,
    room: &Arc<dyn Room>,
    wait: Duration,
    op: &'static str,
    job: impl FnOnce() -> Result<Value, ApiError> + Send + 'static,
) -> Result<Value, ApiError> {
    let ticket = tickets.start();
    let (late, room) = (tickets.clone(), room.clone());
    let answer = late_reply::run(
        wait,
        job,
        move |result: Result<Value, ApiError>| {
            let mut data = json!({"ticket": ticket, "op": op, "ok": result.is_ok()});
            match &result {
                Ok(value) => data["result"] = value.clone(),
                Err(e) => data["error"] = json!(e.message),
            }
            late.finish(ticket, result);
            room.publish(topics::ACCOUNTS, topics::ACCOUNTS_DONE, &data);
        },
        Err(ApiError::internal(
            "couldn't start a thread for the accounts op",
        )),
    );
    match answer {
        Some(result) => {
            tickets.answered(ticket);
            result
        }
        None => Ok(working(ticket, op)),
    }
}

/// `args.ticket`.
pub fn ticket_arg(args: &Value) -> Result<u64, ApiError> {
    args.get("ticket")
        .and_then(Value::as_u64)
        .ok_or_else(|| bad("ticket is the number a working reply gave"))
}
