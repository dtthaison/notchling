//! What the vault service waits on: an approval (Deny / Allow Once / Always Allow) or a request's
//! form, each answered by the app under an id.
//!
//! - Ids are 128 random bits from the OS ([`notchling_engine::vault::random_id`]): nobody can
//!   answer one without having been sent it, and only the app follows the topic it is sent on.
//!   No randomness, no id, and the wait is refused.
//! - At most `max` wait at once, so agents can't pile them up.
//! - A wait ends at its deadline, when `still` says stop (the app left, the vault went off), or
//!   when [`Waiters::drain`] drops every answer channel (the daemon stops). All of those are a
//!   "no": the caller gets `None`.

use std::collections::HashMap;
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Why a wait couldn't start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// `max` already wait.
    Full,
    /// The OS gave no randomness for an id.
    NoId,
}

/// Open waits of one kind.
pub struct Waiters<T> {
    prefix: &'static str,
    max: usize,
    waiting: Mutex<HashMap<String, SyncSender<T>>>,
}

/// One open wait: its id, and where its answer arrives.
pub struct Ticket<T> {
    pub id: String,
    rx: Receiver<T>,
}

impl<T> Waiters<T> {
    pub fn new(prefix: &'static str, max: usize) -> Waiters<T> {
        Waiters {
            prefix,
            max,
            waiting: Mutex::new(HashMap::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, SyncSender<T>>> {
        self.waiting.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A new wait under a fresh id.
    pub fn open(&self) -> Result<Ticket<T>, Refused> {
        let mut waiting = self.lock();
        if waiting.len() >= self.max {
            return Err(Refused::Full);
        }
        let id = notchling_engine::vault::random_id(self.prefix).ok_or(Refused::NoId)?;
        let (tx, rx) = sync_channel(1);
        waiting.insert(id.clone(), tx);
        Ok(Ticket { id, rx })
    }

    /// The app's answer; false when nothing waits under `id` (answered, ended or never opened).
    pub fn answer(&self, id: &str, value: T) -> bool {
        match self.lock().remove(id) {
            Some(tx) => tx.try_send(value).is_ok(),
            None => false,
        }
    }

    /// Ends a wait without an answer.
    pub fn close(&self, id: &str) {
        self.lock().remove(id);
    }

    /// Ends every wait without an answer.
    pub fn drain(&self) {
        self.lock().clear();
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Waits for `ticket`'s answer up to `within`, looking at `still` every `slice`; `None` for
    /// no answer. The id is closed either way.
    pub fn wait(
        &self,
        ticket: Ticket<T>,
        within: Duration,
        slice: Duration,
        still: &dyn Fn() -> bool,
    ) -> Option<T> {
        let deadline = Instant::now() + within;
        let answer = loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || !still() {
                break None;
            }
            match ticket.rx.recv_timeout(left.min(slice)) {
                Ok(value) => break Some(value),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break None,
            }
        };
        self.close(&ticket.id);
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn ids_are_long_random_and_distinct() {
        let w: Waiters<bool> = Waiters::new("vault-approval-", 8);
        let a = w.open().unwrap();
        let b = w.open().unwrap();
        assert_ne!(a.id, b.id);
        let hex = a.id.strip_prefix("vault-approval-").unwrap();
        assert_eq!(hex.len(), 32);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn at_most_max_wait() {
        let w: Waiters<bool> = Waiters::new("x-", 2);
        let _a = w.open().unwrap();
        let _b = w.open().unwrap();
        assert_eq!(w.open().err(), Some(Refused::Full));
    }

    #[test]
    fn an_answer_reaches_its_wait_once() {
        let w: Arc<Waiters<bool>> = Arc::new(Waiters::new("x-", 2));
        let ticket = w.open().unwrap();
        let id = ticket.id.clone();
        let answerer = {
            let w = w.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(30));
                (w.answer(&id, true), w.answer(&id, false))
            })
        };
        let got = w.wait(
            ticket,
            Duration::from_secs(5),
            Duration::from_millis(10),
            &|| true,
        );
        assert_eq!(got, Some(true));
        assert_eq!(
            answerer.join().unwrap(),
            (true, false),
            "the second finds nothing"
        );
        assert!(w.is_empty());
        assert!(!w.answer("x-guessed", true));
    }

    #[test]
    fn a_wait_ends_with_no_at_its_deadline_or_when_told() {
        let w: Waiters<bool> = Waiters::new("x-", 2);
        let t = w.open().unwrap();
        let started = Instant::now();
        assert_eq!(
            w.wait(
                t,
                Duration::from_millis(40),
                Duration::from_millis(10),
                &|| true
            ),
            None
        );
        assert!(started.elapsed() >= Duration::from_millis(40));
        let t = w.open().unwrap();
        assert_eq!(
            w.wait(
                t,
                Duration::from_secs(60),
                Duration::from_millis(10),
                &|| false
            ),
            None
        );
        let t = w.open().unwrap();
        w.drain();
        assert_eq!(
            w.wait(
                t,
                Duration::from_secs(60),
                Duration::from_millis(10),
                &|| true
            ),
            None
        );
        assert!(w.is_empty());
    }
}
