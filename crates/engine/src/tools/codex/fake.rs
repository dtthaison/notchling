//! A scripted tool for tests: [`FakeSpawner`] hands out [`FakeProcess`]es that print the lines they
//! were given and end the way they were told to, so no test runs a real `codex`.

use super::plan::{ExitInfo, TurnSpec};
use super::process::{Next, SpawnError, Stream, ToolLine, ToolProcess, ToolSpawner};
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

/// What one fake process does.
#[derive(Clone, Debug)]
pub struct FakeRun {
    pub lines: Vec<(Stream, String)>,
    /// How it ends when it runs out of lines (and is released).
    pub exit: ExitInfo,
    /// Keeps running, after its lines, until [`FakeProcess::release`] or an interrupt.
    pub held: bool,
    /// How an interrupt ends it.
    pub on_interrupt: ExitInfo,
    /// Ignores an interrupt and a terminate (like a process that traps SIGINT and SIGTERM); only
    /// a kill (or a release) ends it.
    pub stubborn: bool,
    /// Prints nothing, not even its lines, until [`FakeProcess::release`]: a Codex slow to name
    /// its session.
    pub gated: bool,
}

impl FakeRun {
    /// Prints `lines` on standard output and exits 0.
    pub fn ok(lines: &[&str]) -> FakeRun {
        FakeRun {
            lines: lines
                .iter()
                .map(|l| (Stream::Out, (*l).to_string()))
                .collect(),
            exit: ExitInfo {
                code: Some(0),
                signaled: false,
            },
            held: false,
            on_interrupt: ExitInfo {
                code: None,
                signaled: true,
            },
            stubborn: false,
            gated: false,
        }
    }

    /// Prints `lines` (and exits 0) only once released.
    pub fn gated(lines: &[&str]) -> FakeRun {
        FakeRun {
            gated: true,
            ..FakeRun::ok(lines)
        }
    }

    /// Prints `lines`, then stays running through an interrupt and a terminate.
    pub fn stubborn(lines: &[&str]) -> FakeRun {
        FakeRun {
            stubborn: true,
            ..FakeRun::held(lines)
        }
    }

    /// Prints `lines`, then stays running.
    pub fn held(lines: &[&str]) -> FakeRun {
        FakeRun {
            held: true,
            ..FakeRun::ok(lines)
        }
    }

    pub fn exiting(mut self, code: i32) -> FakeRun {
        self.exit = ExitInfo {
            code: Some(code),
            signaled: false,
        };
        self
    }

    /// Adds a line of standard error.
    pub fn stderr(mut self, line: &str) -> FakeRun {
        self.lines.push((Stream::Err, line.to_string()));
        self
    }
}

#[derive(Default)]
struct State {
    lines: VecDeque<(Stream, String)>,
    ended: Option<ExitInfo>,
    released: bool,
    interrupts: usize,
    kills: usize,
}

pub struct FakeProcess {
    run: FakeRun,
    state: Mutex<State>,
    changed: Condvar,
}

impl FakeProcess {
    fn new(run: FakeRun) -> FakeProcess {
        let state = State {
            lines: run.lines.iter().cloned().collect(),
            ..State::default()
        };
        FakeProcess {
            run,
            state: Mutex::new(state),
            changed: Condvar::new(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Lets a held process end.
    pub fn release(&self) {
        self.lock().released = true;
        self.changed.notify_all();
    }

    /// How many times it was interrupted (a terminate counts as one).
    pub fn interrupts(&self) -> usize {
        self.lock().interrupts
    }

    /// How many times it was killed.
    pub fn kills(&self) -> usize {
        self.lock().kills
    }
}

impl ToolProcess for FakeProcess {
    fn next(&self) -> Next {
        let mut state = self.lock();
        loop {
            if self.run.gated && !state.released && state.ended.is_none() {
                state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
                continue;
            }
            if let Some((stream, text)) = state.lines.pop_front() {
                return Next::Line(ToolLine { stream, text });
            }
            if let Some(exit) = state.ended {
                return Next::Exited(exit);
            }
            if !self.run.held || state.released {
                state.ended = Some(self.run.exit);
                continue;
            }
            state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn interrupt(&self) {
        let mut state = self.lock();
        state.interrupts += 1;
        if state.ended.is_none() && !self.run.stubborn {
            state.ended = Some(self.run.on_interrupt);
        }
        self.changed.notify_all();
    }

    fn kill(&self) {
        let mut state = self.lock();
        state.kills += 1;
        if state.ended.is_none() {
            state.ended = Some(self.run.on_interrupt);
        }
        self.changed.notify_all();
    }
}

/// Runs once, in the thread that spawns, before the process exists.
type SpawnHook = Box<dyn FnOnce() + Send>;

#[derive(Default)]
pub struct FakeSpawner {
    runs: Mutex<VecDeque<Result<FakeRun, SpawnError>>>,
    spawned: Mutex<Vec<(TurnSpec, Arc<FakeProcess>)>>,
    hook: Mutex<Option<SpawnHook>>,
}

impl FakeSpawner {
    pub fn new() -> FakeSpawner {
        FakeSpawner::default()
    }

    /// The next spawn is this run (an empty queue runs an empty one that exits 0).
    pub fn push(&self, run: FakeRun) {
        self.queue().push_back(Ok(run));
    }

    /// Runs `hook` at the start of the next spawn, once: a test's way to do something in the
    /// window between a start's checks and its process being registered.
    pub fn on_spawn(&self, hook: SpawnHook) {
        *self.hook.lock().unwrap_or_else(|e| e.into_inner()) = Some(hook);
    }

    /// The next spawn fails.
    pub fn push_error(&self, why: &str) {
        self.queue().push_back(Err(SpawnError(why.to_string())));
    }

    fn queue(&self) -> std::sync::MutexGuard<'_, VecDeque<Result<FakeRun, SpawnError>>> {
        self.runs.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn started(&self) -> std::sync::MutexGuard<'_, Vec<(TurnSpec, Arc<FakeProcess>)>> {
        self.spawned.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// What was asked to start, in order.
    pub fn specs(&self) -> Vec<TurnSpec> {
        self.started().iter().map(|(s, _)| s.clone()).collect()
    }

    /// The processes started, in order.
    pub fn processes(&self) -> Vec<Arc<FakeProcess>> {
        self.started().iter().map(|(_, p)| p.clone()).collect()
    }
}

impl ToolSpawner for FakeSpawner {
    fn spawn(&self, spec: &TurnSpec) -> Result<Arc<dyn ToolProcess>, SpawnError> {
        let hook = self.hook.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(hook) = hook {
            hook();
        }
        let run = match self.queue().pop_front() {
            Some(answer) => answer?,
            None => FakeRun::ok(&[]),
        };
        let process = Arc::new(FakeProcess::new(run));
        self.started().push((spec.clone(), process.clone()));
        Ok(process)
    }
}
