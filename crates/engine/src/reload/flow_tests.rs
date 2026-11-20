use super::flow::{
    run, Done, Host, Need, Options, Phase, Step, Wait, POLL_MS, RESTART_TRIES, WAIT_CAP_MS,
};
use std::collections::VecDeque;

#[derive(Default)]
struct Fake {
    turns: VecDeque<bool>,
    dialogs: VecDeque<bool>,
    tasks: VecDeque<Option<u32>>,
    restarts: VecDeque<Result<bool, String>>,
    now: u64,
    slept: u64,
    restarted: u32,
    cancel_after: Option<u32>,
    asked: u32,
}

impl Host for Fake {
    fn turn_running(&mut self) -> bool {
        self.turns.pop_front().unwrap_or(false)
    }
    fn dialog_open(&mut self) -> bool {
        self.dialogs.pop_front().unwrap_or(false)
    }
    fn tasks_in_flight(&mut self) -> Option<u32> {
        self.tasks.pop_front().unwrap_or(Some(0))
    }
    fn restart(&mut self) -> Result<bool, String> {
        self.restarted += 1;
        self.restarts.pop_front().unwrap_or(Ok(true))
    }
    fn sleep(&mut self, ms: u64) {
        self.now += ms;
        self.slept += ms;
    }
    fn now_ms(&mut self) -> u64 {
        self.now
    }
    fn cancelled(&mut self) -> bool {
        self.asked += 1;
        self.cancel_after.is_some_and(|n| self.asked > n)
    }
}

fn go(host: &mut Fake, after_turn: bool) -> (Phase, Vec<Phase>) {
    let mut said = Vec::new();
    let end = run(host, Options { after_turn }, &mut |p| {
        said.push(p);
        true
    });
    (end, said)
}

fn reloaded() -> Phase {
    Phase::Done {
        result: Done::Reloaded,
    }
}

#[test]
fn an_idle_session_is_restarted_at_once() {
    let mut host = Fake::default();
    let (end, said) = go(&mut host, true);
    assert_eq!(end, reloaded());
    assert_eq!(host.restarted, 1);
    assert_eq!(host.slept, 0);
    assert_eq!(
        said[0],
        Phase::Working {
            step: Step::Restarting
        }
    );
}

#[test]
fn it_waits_for_the_turn_to_end_and_never_restarts_mid_turn() {
    let mut host = Fake {
        turns: VecDeque::from([true, true, true]),
        ..Fake::default()
    };
    let (end, said) = go(&mut host, true);
    assert_eq!(end, reloaded());
    assert_eq!(host.slept, 3 * POLL_MS);
    assert_eq!(
        said.iter()
            .filter(|p| **p == Phase::Waiting { by: Wait::Turn })
            .count(),
        3
    );
    assert_eq!(host.restarted, 1);
}

#[test]
fn without_after_turn_a_running_turn_is_refused() {
    let mut host = Fake {
        turns: VecDeque::from([true]),
        ..Fake::default()
    };
    let (end, _) = go(&mut host, false);
    assert_eq!(
        end,
        Phase::NeedsYou {
            need: Need::Busy { by: Wait::Turn }
        }
    );
    assert_eq!(host.restarted, 0);
}

#[test]
fn an_open_dialog_waits_and_a_tool_dialog_is_refused_without_after_turn() {
    let mut host = Fake {
        dialogs: VecDeque::from([true, true]),
        ..Fake::default()
    };
    let (end, said) = go(&mut host, true);
    assert_eq!(end, reloaded());
    assert!(said.contains(&Phase::Waiting { by: Wait::Dialog }));
    let mut host = Fake {
        dialogs: VecDeque::from([true]),
        ..Fake::default()
    };
    let (end, _) = go(&mut host, false);
    assert_eq!(
        end,
        Phase::NeedsYou {
            need: Need::Busy { by: Wait::Dialog }
        }
    );
    assert_eq!(host.restarted, 0);
}

#[test]
fn background_tasks_hold_it_until_they_finish() {
    let mut host = Fake {
        tasks: VecDeque::from([Some(3), Some(3), Some(1), Some(0)]),
        ..Fake::default()
    };
    let (end, said) = go(&mut host, false);
    assert_eq!(end, reloaded());
    assert!(said.contains(&Phase::Waiting {
        by: Wait::Tasks { count: 3 }
    }));
    assert!(said.contains(&Phase::Waiting {
        by: Wait::Tasks { count: 1 }
    }));
    assert_eq!(host.restarted, 1);
}

#[test]
fn unknown_task_count_stops_without_restarting() {
    let mut host = Fake {
        tasks: VecDeque::from([None]),
        ..Fake::default()
    };
    let (end, _) = go(&mut host, true);
    assert_eq!(
        end,
        Phase::NeedsYou {
            need: Need::TasksUnknown
        }
    );
    assert_eq!(host.restarted, 0);
}

#[test]
fn a_session_that_got_busy_again_is_looked_at_again() {
    let mut host = Fake {
        restarts: VecDeque::from([Ok(false), Ok(true)]),
        ..Fake::default()
    };
    let (end, _) = go(&mut host, true);
    assert_eq!(end, reloaded());
    assert_eq!(host.restarted, 2);
}

#[test]
fn a_failed_restart_is_the_users_to_see() {
    let mut host = Fake {
        restarts: VecDeque::from([Err("Claude Code is slow".to_string())]),
        ..Fake::default()
    };
    let (end, _) = go(&mut host, true);
    assert_eq!(
        end,
        Phase::NeedsYou {
            need: Need::Failed {
                text: "Claude Code is slow".to_string()
            }
        }
    );
}

#[test]
fn cancel_stops_the_wait_before_any_restart() {
    let mut host = Fake {
        turns: VecDeque::from([true; 50]),
        cancel_after: Some(2),
        ..Fake::default()
    };
    let (end, _) = go(&mut host, true);
    assert_eq!(end, Phase::Cancelled);
    assert_eq!(host.restarted, 0);
}

#[test]
fn a_wait_that_never_ends_gives_up() {
    let mut host = Fake {
        turns: VecDeque::from(vec![true; (WAIT_CAP_MS / POLL_MS) as usize + 5]),
        ..Fake::default()
    };
    let (end, _) = go(&mut host, true);
    assert_eq!(
        end,
        Phase::NeedsYou {
            need: Need::Busy { by: Wait::Turn }
        }
    );
    assert_eq!(host.restarted, 0);
}

#[test]
fn phases_round_trip_through_json() {
    let phase = Phase::Waiting {
        by: Wait::Tasks { count: 3 },
    };
    let text = serde_json::to_string(&phase).unwrap();
    assert!(text.contains("\"phase\":\"waiting\""));
    assert_eq!(serde_json::from_str::<Phase>(&text).unwrap(), phase);
}

#[test]
fn a_restart_that_never_goes_through_is_bounded_and_says_why() {
    let mut host = Fake {
        restarts: VecDeque::from(vec![Ok(false); 1_000]),
        ..Fake::default()
    };
    let (end, _) = go(&mut host, true);
    assert!(
        matches!(end, Phase::NeedsYou { need: Need::Failed { ref text } } if !text.is_empty()),
        "{end:?}"
    );
    assert_eq!(host.restarted, RESTART_TRIES);
    assert!(host.slept <= u64::from(RESTART_TRIES) * POLL_MS);
}

#[test]
fn a_restart_that_goes_through_after_a_few_tries_is_done() {
    let mut host = Fake {
        restarts: VecDeque::from([Ok(false), Ok(false), Ok(true)]),
        ..Fake::default()
    };
    let (end, _) = go(&mut host, true);
    assert_eq!(end, reloaded());
    assert_eq!(host.restarted, 3);
}

#[test]
fn a_refused_step_stops_before_the_restart() {
    let mut host = Fake {
        turns: VecDeque::from([true, true]),
        ..Fake::default()
    };
    let mut said = Vec::new();
    let end = run(&mut host, Options { after_turn: true }, &mut |p| {
        let working = matches!(p, Phase::Working { .. });
        said.push(p);
        !working
    });
    assert_eq!(end, Phase::Cancelled);
    assert_eq!(host.restarted, 0);
    assert_eq!(
        said.last(),
        Some(&Phase::Working {
            step: Step::Restarting
        })
    );
}

#[test]
fn a_restart_that_finds_the_session_busy_waits_again_and_never_says_the_end() {
    let mut host = Fake {
        restarts: VecDeque::from([Ok(false), Ok(true)]),
        turns: VecDeque::from([false, true]),
        ..Fake::default()
    };
    let (end, said) = go(&mut host, true);
    assert_eq!(end, reloaded());
    assert!(said.contains(&Phase::Waiting { by: Wait::Turn }));
    assert!(said.iter().all(|p| !p.is_over()));
}
