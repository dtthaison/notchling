use super::drive::{Goal, Reconnected, Refusal, Report};
use super::menu::{Extension, Link};
use serde::{Deserialize, Serialize};

pub const POLL_MS: u64 = 3_000;
pub const TURN_CAP_MS: u64 = 2 * 60 * 60 * 1_000;
pub const BOX_CAP_MS: u64 = 10 * 60 * 1_000;
pub const QUESTION_CAP_MS: u64 = 10 * 60 * 1_000;
pub const CONNECT_LOOKS: usize = 6;
pub const START_TRIES: usize = 3;
pub const RECONNECT_AFTER: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wait {
    Turn,
    Box,
    Question,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Checking,
    Restarting,
    Connecting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Done {
    AlreadyOn,
    On,
    Off,
    AlreadyOff,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "need", rename_all = "snake_case")]
pub enum Need {
    Tasks { count: u32 },
    TasksUnknown,
    NoExtension,
    NotConnected,
    TurnedOffInMcp,
    Unavailable,
    Busy { why: Wait },
    Failed { text: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Phase {
    Waiting { why: Wait },
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
    fn tasks_in_flight(&mut self) -> Option<u32>;
    fn look(&mut self, goal: Goal) -> Result<Report, Refusal>;
    fn restart(&mut self, on: bool) -> Result<(), String>;
    fn sleep(&mut self, ms: u64);
    fn now_ms(&mut self) -> u64;
    fn cancelled(&mut self) -> bool {
        false
    }
    fn set_cancel(&mut self, _cancel: std::sync::Arc<std::sync::atomic::AtomicBool>) {}
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub on: bool,
    pub force: bool,
}

pub const BACKOFF_MAX_MS: u64 = 30_000;

struct Patience {
    since: Option<(Wait, u64)>,
    first: Option<u64>,
    tries: u32,
}

impl Patience {
    fn total(&mut self, now: u64) -> u64 {
        let first = *self.first.get_or_insert(now);
        now.saturating_sub(first)
    }

    fn wait(&mut self, why: Wait, now: u64) -> Option<u64> {
        match self.since {
            Some((was, at)) if was == why => Some(now.saturating_sub(at)),
            _ => {
                self.since = Some((why, now));
                self.tries = 0;
                Some(0)
            }
        }
    }

    fn pause(&mut self, why: Wait) -> u64 {
        let tries = self.tries;
        self.tries += 1;
        match why {
            Wait::Turn => POLL_MS,
            _ => (POLL_MS << tries.min(8)).min(BACKOFF_MAX_MS),
        }
    }

    fn clear(&mut self) {
        self.since = None;
        self.tries = 0;
    }
}

fn cap(why: Wait) -> u64 {
    match why {
        Wait::Turn => TURN_CAP_MS,
        Wait::Box => BOX_CAP_MS,
        Wait::Question => QUESTION_CAP_MS,
    }
}

fn needs(need: Need) -> Phase {
    Phase::NeedsYou { need }
}

fn missing(menu_extension: Extension) -> Need {
    match menu_extension {
        Extension::NotDetected => Need::NoExtension,
        _ => Need::NotConnected,
    }
}

pub fn run(host: &mut dyn Host, options: Options, say: &mut dyn FnMut(Phase)) -> Phase {
    let end = steps(host, options, say);
    say(end.clone());
    end
}

fn steps(host: &mut dyn Host, options: Options, say: &mut dyn FnMut(Phase)) -> Phase {
    let mut patience = Patience {
        since: None,
        first: None,
        tries: 0,
    };
    let mut starts = 0;
    let report = loop {
        if host.cancelled() {
            return Phase::Cancelled;
        }
        if host.turn_running() {
            let now = host.now_ms();
            let waited = patience.wait(Wait::Turn, now).unwrap_or(0);
            if waited >= cap(Wait::Turn) || patience.total(now) >= TURN_CAP_MS {
                return needs(Need::Busy { why: Wait::Turn });
            }
            say(Phase::Waiting { why: Wait::Turn });
            host.sleep(POLL_MS);
            continue;
        }
        say(Phase::Working {
            step: Step::Checking,
        });
        match host.look(Goal::Read) {
            Ok(report) => break report,
            Err(refusal) => {
                let why = match refusal {
                    Refusal::Working => Wait::Turn,
                    Refusal::BoxHasText => Wait::Box,
                    Refusal::Dialog => Wait::Question,
                    Refusal::Unavailable => return needs(Need::Unavailable),
                    Refusal::Starting | Refusal::NoInput => {
                        starts += 1;
                        if starts >= START_TRIES {
                            return needs(Need::Failed {
                                text: refusal.to_string(),
                            });
                        }
                        host.sleep(POLL_MS);
                        continue;
                    }
                    Refusal::Stuck | Refusal::Pty(_) => {
                        return needs(Need::Failed {
                            text: refusal.to_string(),
                        })
                    }
                };
                let now = host.now_ms();
                let waited = patience.wait(why, now).unwrap_or(0);
                if waited >= cap(why) || patience.total(now) >= TURN_CAP_MS {
                    return needs(Need::Busy { why });
                }
                say(Phase::Waiting { why });
                host.sleep(patience.pause(why));
            }
        }
    };
    patience.clear();
    if host.cancelled() {
        return Phase::Cancelled;
    }
    decide(host, options, report, say)
}

fn decide(
    host: &mut dyn Host,
    options: Options,
    report: Report,
    say: &mut dyn FnMut(Phase),
) -> Phase {
    let menu = report.menu;
    match (options.on, &menu.link) {
        (_, Link::Other(text)) => needs(Need::Failed { text: text.clone() }),
        (true, Link::Enabled) => Phase::Done {
            result: Done::AlreadyOn,
        },
        (true, Link::Connecting) => settle(host, options, menu.extension, say, Reconnecting::May),
        (true, Link::NotConnected) => reconnect(host, menu.extension, say),
        (true, Link::NotEnabled) => restart(host, options, say),
        (true, Link::TurnedOffInMcp) => needs(Need::TurnedOffInMcp),
        (false, Link::NotEnabled | Link::TurnedOffInMcp) => Phase::Done {
            result: Done::AlreadyOff,
        },
        (false, _) => restart(host, options, say),
    }
}

fn reconnect(host: &mut dyn Host, extension: Extension, say: &mut dyn FnMut(Phase)) -> Phase {
    say(Phase::Working {
        step: Step::Connecting,
    });
    match host.look(Goal::Reconnect) {
        Ok(report) if matches!(report.reconnect, Reconnected::Failed(_)) => {
            needs(missing(report.menu.extension))
        }
        Ok(report) => settle(
            host,
            Options {
                on: true,
                force: false,
            },
            report.menu.extension,
            say,
            Reconnecting::Done,
        ),
        Err(Refusal::Unavailable) => needs(missing(extension)),
        Err(refusal) => needs(Need::Failed {
            text: refusal.to_string(),
        }),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reconnecting {
    May,
    Restarted,
    Done,
}

fn settle(
    host: &mut dyn Host,
    options: Options,
    extension: Extension,
    say: &mut dyn FnMut(Phase),
    how: Reconnecting,
) -> Phase {
    let mut extension = extension;
    let mut opened = false;
    let reconnected = how != Reconnecting::May;
    for look in 0..CONNECT_LOOKS {
        if host.cancelled() {
            return Phase::Cancelled;
        }
        if look > 0 || reconnected {
            host.sleep(POLL_MS);
        }
        say(Phase::Working {
            step: Step::Connecting,
        });
        match host.look(Goal::Read) {
            Ok(report) => {
                opened = true;
                extension = report.menu.extension;
                match (options.on, report.menu.link) {
                    (true, Link::NotConnected)
                        if look >= RECONNECT_AFTER && how != Reconnecting::Done =>
                    {
                        return reconnect(host, extension, say);
                    }
                    (true, Link::Enabled) => {
                        return Phase::Done {
                            result: if reconnected {
                                Done::On
                            } else {
                                Done::AlreadyOn
                            },
                        }
                    }
                    (false, Link::NotEnabled | Link::TurnedOffInMcp) => {
                        return Phase::Done { result: Done::Off }
                    }
                    (_, Link::Other(text)) => return needs(Need::Failed { text }),
                    (true, Link::NotEnabled) => {
                        return needs(Need::Failed {
                            text: "Claude Code started without Chrome.".to_string(),
                        })
                    }
                    (true, Link::TurnedOffInMcp) => return needs(Need::TurnedOffInMcp),
                    _ => {}
                }
            }
            Err(Refusal::Starting | Refusal::NoInput) => {}
            Err(Refusal::Unavailable) => return needs(Need::Unavailable),
            Err(refusal) => {
                return needs(Need::Failed {
                    text: refusal.to_string(),
                })
            }
        }
    }
    if options.on && !opened {
        needs(Need::Failed {
            text: Refusal::Starting.to_string(),
        })
    } else if options.on {
        needs(missing(extension))
    } else {
        needs(Need::Failed {
            text: "Claude Code still has Chrome on.".to_string(),
        })
    }
}

fn restart(host: &mut dyn Host, options: Options, say: &mut dyn FnMut(Phase)) -> Phase {
    if !options.force {
        match host.tasks_in_flight() {
            None => return needs(Need::TasksUnknown),
            Some(0) => {}
            Some(count) => return needs(Need::Tasks { count }),
        }
    }
    if host.cancelled() {
        return Phase::Cancelled;
    }
    say(Phase::Working {
        step: Step::Restarting,
    });
    if let Err(text) = host.restart(options.on) {
        if host.cancelled() {
            return Phase::Cancelled;
        }
        return needs(Need::Failed { text });
    }
    settle(
        host,
        options,
        Extension::Unknown,
        say,
        Reconnecting::Restarted,
    )
}
