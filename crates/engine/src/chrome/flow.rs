use super::drive::{Goal, Reconnected, Refusal, Report};
use super::menu::{Extension, Link};
use serde::{Deserialize, Serialize};

pub const POLL_MS: u64 = 3_000;
pub const TURN_CAP_MS: u64 = 2 * 60 * 60 * 1_000;
pub const BOX_CAP_MS: u64 = 10 * 60 * 1_000;
pub const QUESTION_CAP_MS: u64 = 10 * 60 * 1_000;
pub const CONNECT_LOOKS: usize = 6;
pub const START_TRIES: usize = 3;

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
}

impl Phase {
    pub fn is_over(&self) -> bool {
        matches!(self, Phase::Done { .. } | Phase::NeedsYou { .. })
    }
}

pub trait Host {
    fn turn_running(&mut self) -> bool;
    fn tasks_in_flight(&mut self) -> u32;
    fn look(&mut self, goal: Goal) -> Result<Report, Refusal>;
    fn restart(&mut self, on: bool) -> Result<(), String>;
    fn sleep(&mut self, ms: u64);
    fn now_ms(&mut self) -> u64;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub on: bool,
    pub force: bool,
}

struct Patience {
    since: Option<(Wait, u64)>,
}

impl Patience {
    fn wait(&mut self, why: Wait, now: u64) -> Option<u64> {
        match self.since {
            Some((was, at)) if was == why => Some(now.saturating_sub(at)),
            _ => {
                self.since = Some((why, now));
                Some(0)
            }
        }
    }

    fn clear(&mut self) {
        self.since = None;
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
    let mut patience = Patience { since: None };
    let mut starts = 0;
    let report = loop {
        if host.turn_running() {
            let now = host.now_ms();
            let waited = patience.wait(Wait::Turn, now).unwrap_or(0);
            if waited >= cap(Wait::Turn) {
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
                if waited >= cap(why) {
                    return needs(Need::Busy { why });
                }
                say(Phase::Waiting { why });
                host.sleep(POLL_MS);
            }
        }
    };
    patience.clear();
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
        (true, Link::Connecting) => settle(host, options, menu.extension, say, false),
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
            true,
        ),
        Err(Refusal::Unavailable) => needs(missing(extension)),
        Err(refusal) => needs(Need::Failed {
            text: refusal.to_string(),
        }),
    }
}

fn settle(
    host: &mut dyn Host,
    options: Options,
    extension: Extension,
    say: &mut dyn FnMut(Phase),
    reconnected: bool,
) -> Phase {
    let mut extension = extension;
    for look in 0..CONNECT_LOOKS {
        if look > 0 || reconnected {
            host.sleep(POLL_MS);
        }
        say(Phase::Working {
            step: Step::Connecting,
        });
        match host.look(Goal::Read) {
            Ok(report) => {
                extension = report.menu.extension;
                match (options.on, report.menu.link) {
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
    if options.on {
        needs(missing(extension))
    } else {
        needs(Need::Failed {
            text: "Claude Code still has Chrome on.".to_string(),
        })
    }
}

fn restart(host: &mut dyn Host, options: Options, say: &mut dyn FnMut(Phase)) -> Phase {
    let tasks = host.tasks_in_flight();
    if tasks > 0 && !options.force {
        return needs(Need::Tasks { count: tasks });
    }
    say(Phase::Working {
        step: Step::Restarting,
    });
    if let Err(text) = host.restart(options.on) {
        return needs(Need::Failed { text });
    }
    settle(host, options, Extension::Unknown, say, true)
}
