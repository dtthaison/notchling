//! The browser sign-in behind `accounts.add.begin/finish`: a listener on 127.0.0.1 that the
//! sign-in page sends its code back to (`AccountLogin.swift`), with the engine's
//! [`SignIn`] for the pure parts. One sign-in at a time; it listens for [`SIGN_IN_FOR`], and stops
//! at once when it is cancelled or the `accounts` switch goes off. The code is exchanged and the
//! login saved only while the switch is on.

use crate::acting_ops::{bad, Room};
use crate::switches::SwitchFlag;
use notchling_engine::accounts::claude::signin::{reply, Callback, SignIn};
use notchling_engine::accounts::claude::{account_value, ClaudeAccounts};
use notchling_local_api::{topics, ApiError};
use serde_json::{json, Value};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// How long a sign-in waits for the browser.
pub const SIGN_IN_FOR: Duration = Duration::from_secs(10 * 60);
/// The most of a request to the listener that is read.
const REQUEST_LIMIT: usize = 16 * 1024;

struct Job {
    login: String,
    stop: AtomicBool,
    outcome: Mutex<Option<Value>>,
    ended: Condvar,
}

impl Job {
    fn end(&self, outcome: Value) {
        let mut slot = self.outcome.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            *slot = Some(outcome);
        }
        self.ended.notify_all();
    }
}

/// What a finished sign-in calls (the ops publish the new list).
pub type Added = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
pub struct SignIns {
    current: Mutex<Option<Arc<Job>>>,
    next: AtomicU64,
}

impl SignIns {
    /// Starts listening and returns `{login, url}`: the sign-in's id and the page to open. A
    /// sign-in already going is cancelled.
    pub fn begin(
        &self,
        store: Arc<ClaudeAccounts>,
        flag: Arc<SwitchFlag>,
        room: Arc<dyn Room>,
        random: [u8; 64],
        added: Added,
    ) -> Result<Value, ApiError> {
        self.cancel_all();
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| ApiError::internal(format!("couldn't listen for the sign-in: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| ApiError::internal(e.to_string()))?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        let sign_in = SignIn::new(random, port);
        let url = sign_in.page_url();
        let login = format!("signin-{}", self.next.fetch_add(1, Ordering::SeqCst) + 1);
        let job = Arc::new(Job {
            login: login.clone(),
            stop: AtomicBool::new(false),
            outcome: Mutex::new(None),
            ended: Condvar::new(),
        });
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = Some(job.clone());
        std::thread::Builder::new()
            .name("accounts-signin".into())
            .spawn(move || {
                let outcome = listen(&listener, &sign_in, &job, &store, &flag, &added);
                let mut data = outcome.clone();
                data["login"] = json!(job.login);
                room.publish(topics::ACCOUNTS, topics::ACCOUNTS_ADD_ENDED, &data);
                job.end(data);
            })
            .map_err(|e| ApiError::internal(e.to_string()))?;
        Ok(json!({"login": login, "url": url}))
    }

    /// `{login, state: "waiting" | "added" | "failed" | "cancelled", account?, error?}`, waiting up
    /// to `wait` for the end. With `cancel`, the sign-in stops first.
    pub fn finish(&self, login: &str, cancel: bool, wait: Duration) -> Result<Value, ApiError> {
        let job = self
            .current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .filter(|j| j.login == login)
            .ok_or_else(|| {
                bad("no sign-in with that id is going (start one with accounts.add.begin)")
            })?;
        if cancel {
            job.stop.store(true, Ordering::SeqCst);
        }
        let slot = job.outcome.lock().unwrap_or_else(|e| e.into_inner());
        let (slot, _) = job
            .ended
            .wait_timeout_while(slot, wait, |o| o.is_none())
            .unwrap_or_else(|e| e.into_inner());
        Ok(slot
            .clone()
            .unwrap_or_else(|| json!({"login": login, "state": "waiting"})))
    }

    /// Stops every sign-in (a new one, or the switch going off).
    pub fn cancel_all(&self) {
        if let Some(job) = self
            .current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            job.stop.store(true, Ordering::SeqCst);
        }
    }
}

/// Listens until a code comes, the time is up, or it is stopped; the outcome without `login`.
fn listen(
    listener: &TcpListener,
    sign_in: &SignIn,
    job: &Job,
    store: &ClaudeAccounts,
    flag: &SwitchFlag,
    added: &Added,
) -> Value {
    let deadline = Instant::now() + SIGN_IN_FOR;
    loop {
        if job.stop.load(Ordering::SeqCst) || !flag.is_on() {
            return json!({"state": "cancelled"});
        }
        if Instant::now() >= deadline {
            return json!({"state": "failed", "error": "The sign-in took too long. Try again."});
        }
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => {
                return json!({"state": "failed", "error": format!("The sign-in stopped listening: {e}")})
            }
        };
        let Some(code) = answer(stream, sign_in) else {
            continue;
        };
        if !flag.is_on() {
            return json!({"state": "cancelled"});
        }
        let now = (store.env().now_ms)();
        let oauth = match sign_in.exchange(store.env().http.as_ref(), &code, now) {
            Ok(oauth) => oauth,
            Err(error) => return json!({"state": "failed", "error": error}),
        };
        return match store.add(&oauth) {
            Ok(account) => {
                added();
                json!({"state": "added", "account": account_value(&account)})
            }
            Err(e) => json!({"state": "failed", "error": e.0}),
        };
    }
}

/// Reads one request and answers it: the code for our callback (redirected to the success page),
/// `None` for anything else (404).
fn answer(mut stream: TcpStream, sign_in: &SignIn) -> Option<notchling_engine::accounts::Secret> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut data = Vec::new();
    let mut chunk = [0u8; 2048];
    while data.len() < REQUEST_LIMIT && !data.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => data.extend_from_slice(&chunk[..n]),
        }
    }
    let head = String::from_utf8_lossy(&data);
    match sign_in.callback(&head) {
        Callback::Code(code) => {
            let _ = stream.write_all(reply(true).as_bytes());
            Some(code)
        }
        Callback::NotOurs => {
            let _ = stream.write_all(reply(false).as_bytes());
            None
        }
    }
}
