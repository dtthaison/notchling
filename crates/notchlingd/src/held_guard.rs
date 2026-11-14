use notchling_engine::reply::exec::Pty;
use notchling_engine::sessions::fresh::job_row;
use notchling_engine::sessions::{Agent, AgentTarget, Probes, Roots};
use notchling_engine::transcript::restart;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub(crate) trait Rest: Send + Sync {
    fn fresh(&self, agent: &Agent) -> Option<Agent>;
    fn restarted(&self, agent: &Agent, now_ms: i64) -> bool;
}

pub(crate) struct FromFiles {
    roots: Roots,
}

impl FromFiles {
    pub(crate) fn new(roots: Roots) -> FromFiles {
        FromFiles { roots }
    }
}

impl Rest for FromFiles {
    fn fresh(&self, agent: &Agent) -> Option<Agent> {
        let AgentTarget::Job(job) = &agent.target else {
            return None;
        };
        job_row(&self.roots, &Probes::from_proc(), job)
    }

    fn restarted(&self, agent: &Agent, now_ms: i64) -> bool {
        agent
            .transcript_path
            .as_deref()
            .is_some_and(|path| restart::restarted(path, now_ms))
    }
}

pub(crate) struct Guarded<'a> {
    pub(crate) inner: Box<dyn Pty + 'a>,
    pub(crate) guard: &'a dyn Fn() -> bool,
    pub(crate) refused: &'a AtomicBool,
    pub(crate) checked: bool,
}

impl Pty for Guarded<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        if !self.checked {
            self.checked = true;
            if !(self.guard)() {
                self.refused.store(true, Ordering::SeqCst);
                return Err(std::io::Error::other("the session is not at rest"));
            }
        }
        self.inner.write(bytes)
    }

    fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> bool {
        self.inner.wait_quiet(quiet, timeout)
    }

    fn wait_attached(&mut self, quiet: Duration, timeout: Duration, busy: Duration) -> bool {
        self.inner.wait_attached(quiet, timeout, busy)
    }

    fn screen(&mut self) -> Vec<String> {
        self.inner.screen()
    }

    fn cursor(&mut self) -> Option<(usize, usize)> {
        self.inner.cursor()
    }

    fn sleep(&mut self, duration: Duration) {
        self.inner.sleep(duration)
    }

    fn now_ms(&self) -> u64 {
        self.inner.now_ms()
    }
}
