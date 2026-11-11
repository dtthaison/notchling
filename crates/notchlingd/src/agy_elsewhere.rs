//! Antigravity sessions another `agy` holds ([`notchling_engine::tools::agy::elsewhere`]): a
//! delegation, an `agy` in a terminal. While one does, the daemon starts no `agy` of its own on
//! the session and sends nothing to it; `agy.state` and `agy.conversation` read Antigravity's log
//! instead, with `elsewhere` set to what Chat says.
//!
//! A watched session is looked at every [`GAP`] by one thread, which runs only while something is
//! watched: a log that grew is `session.changed {change: "elsewhere"}` (Chat reads it again), and
//! once nothing else holds the session it is `released`, after closing the daemon's own `agy` on it
//! if one was open, so the next open loads the whole conversation.

use crate::agy_sink::{lock, Shared};
use notchling_engine::accounts::procs::SystemProcs;
use notchling_engine::tools::agy::elsewhere::{self, Holder};
use notchling_engine::tools::agy::Runner;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// How often a watched session is looked at.
pub const GAP: Duration = Duration::from_secs(2);

/// Who else holds a session, by its id.
pub type Finder = Arc<dyn Fn(&str) -> Option<Holder> + Send + Sync>;

type Stamp = Option<(u64, SystemTime)>;

struct Watched {
    holder: Holder,
    stamp: Stamp,
}

#[derive(Default)]
struct State {
    watched: HashMap<String, Watched>,
    /// The thread is running.
    running: bool,
}

pub(crate) struct Elsewhere {
    /// Antigravity's folder (`~/.gemini/antigravity-cli`).
    root: PathBuf,
    finder: Mutex<Finder>,
    gap: Mutex<Duration>,
    state: Mutex<State>,
    runner: Runner,
    shared: Arc<Shared>,
}

impl Elsewhere {
    /// Over the real processes: an `agy` that isn't this daemon's child holds a session when it
    /// has the session's folder open.
    pub(crate) fn new(
        root: PathBuf,
        home: PathBuf,
        runner: Runner,
        shared: Arc<Shared>,
    ) -> Elsewhere {
        let procs = SystemProcs { home };
        let own = std::process::id();
        let at = root.clone();
        let finder: Finder =
            Arc::new(move |session: &str| elsewhere::find(&procs, &at, session, own));
        Elsewhere {
            root,
            finder: Mutex::new(finder),
            gap: Mutex::new(GAP),
            state: Mutex::new(State::default()),
            runner,
            shared,
        }
    }

    /// A test's processes and pace.
    #[cfg(test)]
    pub fn set_finder(&self, finder: Finder, gap: Duration) {
        *lock(&self.finder) = finder;
        *lock(&self.gap) = gap;
    }

    fn find(&self, session: &str) -> Option<Holder> {
        let finder = lock(&self.finder).clone();
        finder(session)
    }

    /// Asks now whether another `agy` holds the session; if one does, the session is watched.
    pub fn check(self: &Arc<Self>, session: &str) -> Option<Holder> {
        let holder = self.find(session)?;
        let start = {
            let mut state = lock(&self.state);
            let stamp = elsewhere::log_stamp(&self.root, session);
            state
                .watched
                .entry(session.to_owned())
                .and_modify(|w| w.holder = holder)
                .or_insert(Watched { holder, stamp });
            !std::mem::replace(&mut state.running, true)
        };
        if start {
            let me = self.clone();
            std::thread::spawn(move || me.watch());
        }
        Some(holder)
    }

    /// The holder of a watched session, as last seen (no process is asked).
    pub fn watched(&self, session: &str) -> Option<Holder> {
        lock(&self.state).watched.get(session).map(|w| w.holder)
    }

    /// The conversation as Antigravity's log has it, and whether a turn goes on there.
    pub fn conversation(
        &self,
        session: &str,
        holder: &Holder,
    ) -> (notchling_engine::transcript::model::Conversation, bool) {
        elsewhere::from_log(&self.root, session, holder.note())
    }

    fn watch(&self) {
        loop {
            let gap = *lock(&self.gap);
            std::thread::sleep(gap);
            let ids: Vec<String> = {
                let mut state = lock(&self.state);
                if state.watched.is_empty() {
                    state.running = false;
                    return;
                }
                state.watched.keys().cloned().collect()
            };
            for id in ids {
                self.look(&id);
            }
        }
    }

    fn look(&self, session: &str) {
        if self.find(session).is_none() {
            lock(&self.state).watched.remove(session);
            // Ours loaded before the other one wrote; a fresh open loads all of it.
            if self.runner.is_open(session) {
                self.runner.close(session);
            }
            self.shared.session_changed(session, "released", None);
            return;
        }
        let stamp = elsewhere::log_stamp(&self.root, session);
        let grew = {
            let mut state = lock(&self.state);
            match state.watched.get_mut(session) {
                Some(w) if w.stamp != stamp => {
                    w.stamp = stamp;
                    true
                }
                _ => false,
            }
        };
        if grew {
            self.shared.session_changed(session, "elsewhere", None);
        }
    }
}
