use super::plan::{ExitInfo, TurnSpec};
use super::process::{Next, SpawnError, Stream, ToolLine, ToolProcess, ToolSpawner};
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

#[derive(Clone, Debug)]
pub struct FakeRun {
    pub lines: Vec<(Stream, String)>,
    pub exit: ExitInfo,
    pub held: bool,
    pub on_interrupt: ExitInfo,
    pub stubborn: bool,
    pub gated: bool,
}

impl FakeRun {
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

    pub fn gated(lines: &[&str]) -> FakeRun {
        FakeRun {
            gated: true,
            ..FakeRun::ok(lines)
        }
    }

    pub fn stubborn(lines: &[&str]) -> FakeRun {
        FakeRun {
            stubborn: true,
            ..FakeRun::held(lines)
        }
    }

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

    pub fn release(&self) {
        self.lock().released = true;
        self.changed.notify_all();
    }

    pub fn interrupts(&self) -> usize {
        self.lock().interrupts
    }

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

    pub fn push(&self, run: FakeRun) {
        self.queue().push_back(Ok(run));
    }

    pub fn on_spawn(&self, hook: SpawnHook) {
        *self.hook.lock().unwrap_or_else(|e| e.into_inner()) = Some(hook);
    }

    pub fn push_error(&self, why: &str) {
        self.queue().push_back(Err(SpawnError(why.to_string())));
    }

    fn queue(&self) -> std::sync::MutexGuard<'_, VecDeque<Result<FakeRun, SpawnError>>> {
        self.runs.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn started(&self) -> std::sync::MutexGuard<'_, Vec<(TurnSpec, Arc<FakeProcess>)>> {
        self.spawned.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn specs(&self) -> Vec<TurnSpec> {
        self.started().iter().map(|(s, _)| s.clone()).collect()
    }

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
