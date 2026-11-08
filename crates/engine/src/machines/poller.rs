//! Asking every worker machine for `worker status --json` (Swift `MachinePoller` and
//! `MachineHealth`).
//!
//! The caller runs [`MachinePoller::refresh`] on a timer ([`INTERVAL_SECS`]) and keeps the answers
//! for the Machines view. Nothing here starts a thread of its own beyond the round's parallel ssh
//! calls, and it never watches the Mac's sleep: the caller stops its timer on sleep and, on wake,
//! calls `refresh(true)` once and starts the timer again.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use super::registry::Machine;
use super::runner::{Call, CommandResult, Runner};
use super::shell;
use super::status::{NeedsYouEntry, WorkerStatus};

/// The seconds between two polls.
pub const INTERVAL_SECS: u64 = 60;
const COMMAND_TIMEOUT_SECS: u64 = 30;

/// What the last poll of a machine returned: a status, or the error that stopped it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LastReport {
    pub status: Option<WorkerStatus>,
    /// Seconds since 1970.
    pub at: Option<f64>,
    pub error: Option<String>,
}

impl LastReport {
    pub fn ok(status: WorkerStatus, at: f64) -> LastReport {
        LastReport {
            status: Some(status),
            at: Some(at),
            error: None,
        }
    }

    pub fn failed(error: &str, at: f64) -> LastReport {
        LastReport {
            status: None,
            at: Some(at),
            error: Some(error.into()),
        }
    }

    /// The poll got a status back and reported no error.
    pub fn succeeded(&self) -> bool {
        self.status.is_some() && self.error.is_none()
    }
}

/// The health words the Machines view shows. Pure: the poller supplies the reports.
pub mod health {
    use super::{LastReport, WorkerStatus};

    pub const ONLINE: &str = "online";
    pub const OFFLINE: &str = "offline";
    pub const RESTARTED: &str = "restarted, waiting for unlock";
    pub const PAUSED: &str = "production busy, paused";

    /// One word for a machine.
    /// - `file_vault`: whether the machine is a FileVault Mac, which stops at the unlock screen
    ///   after a reboot. A VPS never gets `restarted, waiting for unlock`.
    /// - `last`: the poll just made.
    /// - `previous`: the report before it that held a status (the poller keeps the last good one,
    ///   so a run of failed polls still sees what the machine was doing before it went quiet).
    /// - `_now`: unused today; kept so a rule that depends on report age needs no signature change.
    pub fn word(
        file_vault: bool,
        last: &LastReport,
        previous: Option<&LastReport>,
        _now: f64,
    ) -> &'static str {
        if let (Some(status), None) = (&last.status, &last.error) {
            let before_boot = previous
                .and_then(|p| p.status.as_ref())
                .and_then(|s| s.booted_at);
            if let (true, Some(_), Some(booted), Some(before)) =
                (file_vault, &status.current, status.booted_at, before_boot)
            {
                if booted > before {
                    return RESTARTED;
                }
            }
            return if status.paused.is_some() {
                PAUSED
            } else {
                ONLINE
            };
        }
        if file_vault {
            if let Some(before) = previous.and_then(|p| p.status.as_ref()) {
                if before.current.is_some()
                    || !before.ready.is_empty()
                    || !before.needs_you.is_empty()
                {
                    return RESTARTED;
                }
            }
        }
        OFFLINE
    }

    /// "GitHub token on m1 expires in 14 days" once the token has 14 days or fewer; `None` before
    /// that or when the worker does not know the expiry.
    pub fn token_warning(status: &WorkerStatus) -> Option<String> {
        let days = status.token_expires_in_days.filter(|d| *d <= 14)?;
        let name = if status.machine.is_empty() {
            "this machine"
        } else {
            status.machine.as_str()
        };
        if days < 0 {
            return Some(format!("GitHub token on {name} has expired"));
        }
        let unit = if days == 1 { "day" } else { "days" };
        Some(format!("GitHub token on {name} expires in {days} {unit}"))
    }
}

/// The notification for the questions that were already waiting when polling began.
pub fn waiting_text(count: usize) -> String {
    if count == 1 {
        "1 question is waiting".into()
    } else {
        format!("{count} questions are waiting")
    }
}

type Machines = Box<dyn Fn() -> Vec<Machine> + Send + Sync>;
type Clock = Box<dyn Fn() -> f64 + Send + Sync>;
type NewNeedsYou = Box<dyn Fn(&Machine, &NeedsYouEntry) + Send + Sync>;
type QuestionsWaiting = Box<dyn Fn(usize) + Send + Sync>;

#[derive(Default)]
struct State {
    reports: BTreeMap<String, LastReport>,
    previous: BTreeMap<String, LastReport>,
    announced: BTreeSet<String>,
    /// Machines with a good poll behind them.
    baselined: BTreeSet<String>,
    refreshing: bool,
    /// A refresh asked for while one was running, to run when that one ends (the wake after a sleep).
    refresh_queued: bool,
}

pub struct MachinePoller {
    runner: Arc<dyn Runner>,
    machines: Machines,
    clock: Clock,
    state: Mutex<State>,
    on_new_needs_you: Option<NewNeedsYou>,
    on_questions_waiting: Option<QuestionsWaiting>,
}

fn lock(m: &Mutex<State>) -> MutexGuard<'_, State> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Gives `refreshing` back even if a poll panics.
struct Refreshing<'a>(&'a Mutex<State>);

impl Drop for Refreshing<'_> {
    fn drop(&mut self) {
        lock(self.0).refreshing = false;
    }
}

impl MachinePoller {
    /// `clock` gives the seconds since 1970 stamped on each report.
    pub fn new(
        runner: Arc<dyn Runner>,
        machines: impl Fn() -> Vec<Machine> + Send + Sync + 'static,
        clock: impl Fn() -> f64 + Send + Sync + 'static,
    ) -> MachinePoller {
        MachinePoller {
            runner,
            machines: Box::new(machines),
            clock: Box::new(clock),
            state: Mutex::new(State::default()),
            on_new_needs_you: None,
            on_questions_waiting: None,
        }
    }

    /// Fires once per (machine, repo, issue) the first time it shows in a status, until the
    /// poller is dropped. An item that leaves and comes back does not fire again.
    pub fn on_new_needs_you(
        mut self,
        f: impl Fn(&Machine, &NeedsYouEntry) + Send + Sync + 'static,
    ) -> MachinePoller {
        self.on_new_needs_you = Some(Box::new(f));
        self
    }

    /// Fires once for the first good poll of each machine (all machines of one round together)
    /// with the number of questions that were already waiting, instead of announcing them one by
    /// one: Notchling relaunches on every install, and each launch would else ring once per open
    /// question. Not called for none.
    pub fn on_questions_waiting(
        mut self,
        f: impl Fn(usize) + Send + Sync + 'static,
    ) -> MachinePoller {
        self.on_questions_waiting = Some(Box::new(f));
        self
    }

    /// The latest poll of each machine by name: a status, or the error that stopped it.
    pub fn reports(&self) -> BTreeMap<String, LastReport> {
        lock(&self.state).reports.clone()
    }

    pub fn report(&self, machine: &str) -> Option<LastReport> {
        lock(&self.state).reports.get(machine).cloned()
    }

    /// The report before the latest one that held a status. A failed poll leaves it as it was, so
    /// a run of failures still shows what the machine was doing before it went quiet (see
    /// [`health::word`]).
    pub fn previous(&self) -> BTreeMap<String, LastReport> {
        lock(&self.state).previous.clone()
    }

    pub fn previous_of(&self, machine: &str) -> Option<LastReport> {
        lock(&self.state).previous.get(machine).cloned()
    }

    /// Polls every machine at once. A poll that is still running makes this a no-op, unless
    /// `queue_if_busy`: then one more poll runs when it ends. The wake from sleep asks that way,
    /// because the poll in flight at sleep may be stuck on a connection that died, and its answer
    /// is a stale "unreachable". Blocks until the polls it ran are done.
    pub fn refresh(&self, queue_if_busy: bool) {
        {
            let mut s = lock(&self.state);
            if s.refreshing {
                if queue_if_busy {
                    s.refresh_queued = true;
                }
                return;
            }
            s.refreshing = true;
        }
        let _guard = Refreshing(&self.state);
        loop {
            lock(&self.state).refresh_queued = false;
            self.poll_once();
            if !lock(&self.state).refresh_queued {
                break;
            }
        }
    }

    /// One round, without the busy check: ask every machine that has an sd worker job and an sd
    /// path (a machine without either has nothing to ask: `python3 '' worker status` would only
    /// fail), keep the answers, and fire the callbacks.
    pub fn poll_once(&self) {
        let list: Vec<Machine> = (self.machines)()
            .into_iter()
            .filter(|m| m.sd_worker().is_some() && !m.sd_path().is_empty())
            .collect();
        let results: Vec<CommandResult> = std::thread::scope(|scope| {
            let handles: Vec<_> = list
                .iter()
                .map(|m| {
                    let runner = &self.runner;
                    scope.spawn(move || {
                        // `shell::path` lets a `~/…` path expand for the worker user
                        let script = format!(
                            "{} {} worker status --json",
                            shell::path(m.python()),
                            shell::path(m.sd_path())
                        );
                        runner.run(&Call::secs(
                            shell::ssh_script(m, &script),
                            COMMAND_TIMEOUT_SECS,
                        ))
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| {
                    h.join()
                        .unwrap_or_else(|_| CommandResult::new(1, "", "the poll stopped"))
                })
                .collect()
        });

        let now = (self.clock)();
        let names: BTreeSet<&str> = list.iter().map(|m| m.name.as_str()).collect();
        let mut waiting = 0;
        let mut announce: Vec<(&Machine, NeedsYouEntry)> = Vec::new();
        {
            let mut s = lock(&self.state);
            s.reports.retain(|k, _| names.contains(k.as_str()));
            s.previous.retain(|k, _| names.contains(k.as_str()));
            s.baselined.retain(|k| names.contains(k.as_str()));
            for (m, result) in list.iter().zip(&results) {
                let report = report_from(result, now);
                if let Some(old) = s.reports.get(&m.name) {
                    if old.status.is_some() {
                        let old = old.clone();
                        s.previous.insert(m.name.clone(), old);
                    }
                }
                s.reports.insert(m.name.clone(), report.clone());
                let (Some(status), None) = (&report.status, &report.error) else {
                    continue;
                };
                if s.baselined.insert(m.name.clone()) {
                    waiting += status.needs_you.len();
                    for e in &status.needs_you {
                        s.announced.insert(key(m, e));
                    }
                } else {
                    for e in &status.needs_you {
                        if s.announced.insert(key(m, e)) {
                            announce.push((m, e.clone()));
                        }
                    }
                }
            }
        }
        // the callbacks run outside the lock: they may ask for the reports
        if let Some(f) = &self.on_new_needs_you {
            for (m, e) in &announce {
                f(m, e);
            }
        }
        if waiting > 0 {
            if let Some(f) = &self.on_questions_waiting {
                f(waiting);
            }
        }
    }
}

fn key(m: &Machine, e: &NeedsYouEntry) -> String {
    format!("{}\u{1F}{}\u{1F}{}", m.name, e.repo, e.issue)
}

fn report_from(result: &CommandResult, now: f64) -> LastReport {
    match result.code {
        0 => match WorkerStatus::decode(result.out.as_bytes()) {
            Some(status) => LastReport::ok(status, now),
            None => LastReport::failed("unreadable status", now),
        },
        124 => LastReport::failed("timed out", now),
        127 => {
            // 127 is also what the shell on the machine says for a missing python or `sd.py`
            let text = result.err.trim();
            if text == "/usr/bin/ssh not found" {
                return LastReport::failed("ssh not found", now);
            }
            let first = text.split('\n').find(|l| !l.is_empty()).unwrap_or("");
            if first.is_empty() {
                LastReport::failed("not found on the machine", now)
            } else {
                LastReport::failed(&format!("not found on the machine: {first}"), now)
            }
        }
        code => {
            let text = result.err.trim();
            if text.is_empty() {
                LastReport::failed(&format!("ssh exited {code}"), now)
            } else {
                LastReport::failed(text, now)
            }
        }
    }
}
