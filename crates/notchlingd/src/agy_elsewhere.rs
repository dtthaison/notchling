use crate::agy_sink::{lock, Shared};
use notchling_engine::accounts::procs::SystemProcs;
use notchling_engine::tools::agy::elsewhere::{self, Holder};
use notchling_engine::tools::agy::Runner;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

pub const GAP: Duration = Duration::from_secs(2);

pub type Finder = Arc<dyn Fn(&str) -> Option<Holder> + Send + Sync>;

type Stamp = Option<(u64, SystemTime)>;

struct Watched {
    holder: Holder,
    stamp: Stamp,
}

#[derive(Default)]
struct State {
    watched: HashMap<String, Watched>,
    running: bool,
}

pub(crate) struct Elsewhere {
    root: PathBuf,
    finder: Mutex<Finder>,
    gap: Mutex<Duration>,
    state: Mutex<State>,
    runner: Runner,
    shared: Arc<Shared>,
}

impl Elsewhere {
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

    #[cfg(test)]
    pub fn set_finder(&self, finder: Finder, gap: Duration) {
        *lock(&self.finder) = finder;
        *lock(&self.gap) = gap;
    }

    fn find(&self, session: &str) -> Option<Holder> {
        let finder = lock(&self.finder).clone();
        finder(session)
    }

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

    pub fn watched(&self, session: &str) -> Option<Holder> {
        lock(&self.state).watched.get(session).map(|w| w.holder)
    }

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
