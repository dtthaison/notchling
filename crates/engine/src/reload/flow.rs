use serde::{Deserialize, Serialize};

pub const POLL_MS: u64 = 3_000;
pub const WAIT_CAP_MS: u64 = 2 * 60 * 60 * 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "why", rename_all = "snake_case")]
pub enum Wait {
    Turn,
    Dialog,
    Tasks { count: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Restarting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Done {
    Reloaded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "need", rename_all = "snake_case")]
pub enum Need {
    Busy { by: Wait },
    TasksUnknown,
    Failed { text: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Phase {
    Waiting { by: Wait },
    Working { step: Step },
    Done { result: Done },
    NeedsYou { need: Need },
    Cancelled,
}

impl Phase {
    pub fn is_over(&self) -> bool {
        matches!(
            self,
            Phase::Done { .. } | Phase::NeedsYou { .. } | Phase::Cancelled
        )
    }
}

pub trait Host {
    fn turn_running(&mut self) -> bool;
    fn dialog_open(&mut self) -> bool;
    fn tasks_in_flight(&mut self) -> Option<u32>;
    fn restart(&mut self) -> Result<bool, String>;
    fn sleep(&mut self, ms: u64);
    fn now_ms(&mut self) -> u64;
    fn cancelled(&mut self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub after_turn: bool,
}

fn blocker(host: &mut dyn Host) -> Result<Option<Wait>, Need> {
    if host.turn_running() {
        return Ok(Some(Wait::Turn));
    }
    if host.dialog_open() {
        return Ok(Some(Wait::Dialog));
    }
    match host.tasks_in_flight() {
        None => Err(Need::TasksUnknown),
        Some(0) => Ok(None),
        Some(count) => Ok(Some(Wait::Tasks { count })),
    }
}

pub fn run(host: &mut dyn Host, options: Options, say: &mut dyn FnMut(Phase)) -> Phase {
    let end = steps(host, options, say);
    say(end.clone());
    end
}

fn steps(host: &mut dyn Host, options: Options, say: &mut dyn FnMut(Phase)) -> Phase {
    let started = host.now_ms();
    loop {
        if host.cancelled() {
            return Phase::Cancelled;
        }
        match blocker(host) {
            Err(need) => return Phase::NeedsYou { need },
            Ok(Some(by)) => {
                let holds_turn = matches!(by, Wait::Turn | Wait::Dialog);
                let over = host.now_ms().saturating_sub(started) >= WAIT_CAP_MS;
                if (holds_turn && !options.after_turn) || over {
                    return Phase::NeedsYou {
                        need: Need::Busy { by },
                    };
                }
                say(Phase::Waiting { by });
                host.sleep(POLL_MS);
                continue;
            }
            Ok(None) => {}
        }
        say(Phase::Working {
            step: Step::Restarting,
        });
        match host.restart() {
            Ok(true) => {
                return Phase::Done {
                    result: Done::Reloaded,
                }
            }
            Ok(false) => host.sleep(POLL_MS),
            Err(text) => {
                if host.cancelled() {
                    return Phase::Cancelled;
                }
                return Phase::NeedsYou {
                    need: Need::Failed { text },
                };
            }
        }
    }
}
