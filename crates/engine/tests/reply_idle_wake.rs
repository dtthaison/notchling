use notchling_engine::reply::exec::{
    execute_noting, wait_attached, Failure, Markers, NoQueueLog, Outcome, Pty,
};
use notchling_engine::reply::keys::{self, timing::*};
use notchling_engine::reply::plan::{self, Plan};
use std::path::Path;
use std::time::Duration;

type Shot = (Vec<String>, Option<(usize, usize)>);

fn shot(name: &str) -> Shot {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/input-box")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut lines = text.lines();
    let head: Vec<&str> = lines.next().unwrap().split(' ').collect();
    let cursor = (head[1].parse().unwrap(), head[2].parse().unwrap());
    let shown = head[3] == "true";
    (lines.map(String::from).collect(), shown.then_some(cursor))
}

const BOX_ROW: usize = 4;
const RULE: &str =
    "────────────────────────────────────────────────────────────────────────────────────────────────────";

enum Look {
    Drawn(Shot),
    Live,
}

struct Idle {
    frames: Vec<(u64, Look)>,
    clock: u64,
    held: String,
    sent: Vec<String>,
    writes: Vec<(u64, String)>,
    stop_at: Option<u64>,
}

impl Idle {
    fn new(frames: Vec<(u64, Look)>) -> Idle {
        Idle {
            frames,
            clock: 0,
            held: String::new(),
            sent: Vec::new(),
            writes: Vec::new(),
            stop_at: None,
        }
    }

    fn stopped_at(mut self, ms: u64) -> Idle {
        self.stop_at = Some(ms);
        self
    }

    fn drafting(mut self, draft: &str) -> Idle {
        self.held = draft.to_string();
        self
    }

    fn look(&self) -> &Look {
        let now = self.frames.iter().rev().find(|(at, _)| *at <= self.clock);
        match now {
            Some((_, look)) => look,
            None => &self.frames[0].1,
        }
    }

    fn up(&self) -> bool {
        matches!(self.look(), Look::Live)
    }
}

fn drawn(name: &str) -> Look {
    Look::Drawn(shot(name))
}

impl Pty for Idle {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let typed = String::from_utf8_lossy(bytes).into_owned();
        self.writes.push((self.clock, typed.clone()));
        if !self.up() {
            return Ok(());
        }
        if typed == keys::clear_input() {
            self.held.clear();
        } else if typed == keys::ENTER {
            if !self.held.is_empty() {
                self.sent.push(std::mem::take(&mut self.held));
            }
        } else if let Some(text) = typed
            .strip_prefix(keys::PASTE_START)
            .and_then(|t| t.strip_suffix(keys::PASTE_END))
        {
            self.held.push_str(text);
        }
        Ok(())
    }

    fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> bool {
        let (quiet, timeout) = (quiet.as_millis() as u64, timeout.as_millis() as u64);
        let end = self.clock + timeout;
        let mut drew = false;
        loop {
            let next = self
                .frames
                .iter()
                .map(|(at, _)| *at)
                .find(|at| *at > self.clock);
            match next {
                Some(at) if at <= self.clock + quiet && at <= end => {
                    self.clock = at;
                    drew = true;
                }
                _ if drew || self.clock > 0 => {
                    self.clock = (self.clock + quiet).min(end);
                    return true;
                }
                Some(at) if at <= end => {
                    self.clock = at;
                    drew = true;
                }
                _ => {
                    self.clock = end;
                    return false;
                }
            }
        }
    }

    fn screen(&mut self) -> Vec<String> {
        match self.look() {
            Look::Drawn((lines, _)) => lines.clone(),
            Look::Live => vec![
                String::new(),
                "  An earlier line of the session.".into(),
                String::new(),
                RULE.into(),
                format!("❯\u{a0}{}", self.held),
                RULE.into(),
                "  ? for shortcuts".into(),
            ],
        }
    }

    fn cursor(&mut self) -> Option<(usize, usize)> {
        match self.look() {
            Look::Drawn((_, cursor)) => *cursor,
            Look::Live => Some((BOX_ROW, 2 + self.held.chars().count())),
        }
    }

    fn sleep(&mut self, duration: Duration) {
        self.clock += duration.as_millis() as u64;
    }

    fn now_ms(&self) -> u64 {
        self.clock
    }

    fn stopping(&self) -> bool {
        self.stop_at.is_some_and(|at| self.clock >= at)
    }
}

const TEXT: &str = "can you look at the failing check again?";

fn the_outbox_plan() -> Plan {
    plan::reply_waking(TEXT)
}

fn reply(pty: &mut Idle, trail: &mut Vec<String>) -> Result<Outcome, Failure> {
    execute_noting(&the_outbox_plan(), pty, &Markers, &mut NoQueueLog, trail)
}

fn slow_to_start(up_at: u64) -> Idle {
    Idle::new(vec![
        (150, drawn("idle-wake-blank.txt")),
        (250, drawn("idle-wake-starting.txt")),
        (up_at * 2 / 3, drawn("idle-wake-restarting.txt")),
        (up_at, Look::Live),
    ])
}

#[test]
fn a_job_that_needs_forty_seconds_to_start_still_gets_the_reply() {
    let mut pty = slow_to_start(40_000);
    let mut trail = Vec::new();
    let out = reply(&mut pty, &mut trail);
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    assert_eq!(pty.sent, [TEXT]);
    for (at, typed) in &pty.writes {
        assert!(
            *at >= 40_000,
            "{typed:?} typed at {at} ms, before the job was up"
        );
    }
}

#[test]
fn a_blank_screen_is_a_session_still_drawing_and_is_waited_on() {
    let mut pty = Idle::new(vec![
        (150, drawn("idle-wake-blank.txt")),
        (12_000, Look::Live),
    ]);
    let mut trail = Vec::new();
    let out = reply(&mut pty, &mut trail);
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    assert_eq!(pty.sent, [TEXT]);
}

#[test]
fn a_screen_that_stays_blank_ends_as_not_open_in_time_and_gets_no_key() {
    let mut pty = Idle::new(vec![(150, drawn("idle-wake-blank.txt"))]);
    let mut trail = Vec::new();
    assert_eq!(
        reply(&mut pty, &mut trail),
        Err(Failure::Timeout),
        "{trail:#?}"
    );
    assert!(pty.writes.is_empty());
    assert!(
        trail.iter().any(|t| t.contains("nothing drawn yet")),
        "{trail:#?}"
    );
}

#[test]
fn a_session_that_was_starting_and_then_shows_no_box_has_not_opened_in_time() {
    let mut pty = Idle::new(vec![
        (150, drawn("idle-wake-starting.txt")),
        (3_000, drawn("idle-wake-between.txt")),
    ]);
    let mut trail = Vec::new();
    assert_eq!(
        reply(&mut pty, &mut trail),
        Err(Failure::Timeout),
        "{trail:#?}"
    );
    assert!(pty.writes.is_empty());
}

#[test]
fn a_screen_that_never_shows_a_box_or_a_start_still_fails_at_once_as_no_input() {
    let mut pty = Idle::new(vec![(150, drawn("idle-wake-picker.txt"))]);
    let mut trail = Vec::new();
    assert_eq!(
        reply(&mut pty, &mut trail),
        Err(Failure::NoInput),
        "{trail:#?}"
    );
    assert!(pty.writes.is_empty());
    assert!(
        pty.clock < BOOT_WAIT_MS,
        "gave up after {} ms, the boot wait is {BOOT_WAIT_MS} ms",
        pty.clock
    );
}

#[test]
fn a_job_that_never_starts_gets_no_key() {
    let mut pty = Idle::new(vec![
        (150, drawn("idle-wake-blank.txt")),
        (250, drawn("idle-wake-starting.txt")),
    ]);
    let mut trail = Vec::new();
    assert_eq!(
        reply(&mut pty, &mut trail),
        Err(Failure::Timeout),
        "{trail:#?}"
    );
    assert!(pty.writes.is_empty());
    assert!(pty.sent.is_empty());
}

#[test]
fn a_box_that_holds_someone_elses_text_after_a_slow_start_is_never_typed_over() {
    let mut pty = slow_to_start(20_000).drafting("a note somebody typed here");
    let mut trail = Vec::new();
    assert_eq!(
        reply(&mut pty, &mut trail),
        Err(Failure::NotCleared),
        "{trail:#?}"
    );
    assert!(pty.writes.is_empty(), "{:?}", pty.writes);
    assert!(pty.sent.is_empty());
}

#[test]
fn a_prompt_drawn_after_a_slow_start_is_settled_for_half_a_second_before_the_first_key() {
    let mut pty = slow_to_start(20_000);
    let mut trail = Vec::new();
    let out = reply(&mut pty, &mut trail);
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    for (at, typed) in &pty.writes {
        assert!(
            *at >= 20_000 + START_SETTLE_MS,
            "{typed:?} typed at {at} ms, the prompt was drawn at 20000 ms"
        );
    }
}

#[test]
fn a_box_that_flashes_without_focus_during_the_start_does_not_end_the_wait() {
    let mut pty = Idle::new(vec![
        (150, drawn("idle-wake-starting.txt")),
        (5_000, drawn("idle-wake-unfocused.txt")),
        (5_400, drawn("idle-wake-between.txt")),
        (30_000, Look::Live),
    ]);
    let mut trail = Vec::new();
    let out = reply(&mut pty, &mut trail);
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    assert_eq!(pty.sent, [TEXT]);
    for (at, typed) in &pty.writes {
        assert!(
            *at >= 30_000,
            "{typed:?} typed at {at} ms, before the job was up"
        );
    }
}

#[test]
fn a_blank_screen_followed_by_a_picker_fails_as_no_input_and_does_not_wait_out_the_wake() {
    let mut pty = Idle::new(vec![
        (150, drawn("idle-wake-blank.txt")),
        (2_000, drawn("idle-wake-picker.txt")),
    ]);
    let mut trail = Vec::new();
    assert_eq!(
        reply(&mut pty, &mut trail),
        Err(Failure::NoInput),
        "{trail:#?}"
    );
    assert!(pty.writes.is_empty());
    assert!(
        pty.clock < BOOT_WAIT_MS,
        "waited {} ms on a picker, the boot wait is {BOOT_WAIT_MS} ms",
        pty.clock
    );
}

#[test]
fn a_stop_during_the_wait_ends_it_at_once_with_no_key() {
    let mut pty = Idle::new(vec![
        (150, drawn("idle-wake-blank.txt")),
        (250, drawn("idle-wake-starting.txt")),
    ])
    .stopped_at(2_000);
    let mut trail = Vec::new();
    assert_eq!(
        reply(&mut pty, &mut trail),
        Err(Failure::Timeout),
        "{trail:#?}"
    );
    assert!(pty.writes.is_empty());
    assert!(
        pty.clock < 3_000,
        "stopped at 2000 ms, ended at {} ms",
        pty.clock
    );
}

struct Busy {
    clock: u64,
    stop_at: u64,
    writes: usize,
}

impl Pty for Busy {
    fn write(&mut self, _bytes: &[u8]) -> std::io::Result<()> {
        self.writes += 1;
        Ok(())
    }

    fn wait_quiet(&mut self, _quiet: Duration, timeout: Duration) -> bool {
        self.clock += timeout.as_millis() as u64;
        false
    }

    fn wait_attached(&mut self, quiet: Duration, timeout: Duration, busy: Duration) -> bool {
        wait_attached(self, quiet, timeout, busy)
    }

    fn screen(&mut self) -> Vec<String> {
        vec!["⠋ Reconnecting".to_string()]
    }

    fn sleep(&mut self, duration: Duration) {
        self.clock += duration.as_millis() as u64;
    }

    fn now_ms(&self) -> u64 {
        self.clock
    }

    fn stopping(&self) -> bool {
        self.clock >= self.stop_at
    }
}

#[test]
fn a_stop_while_the_attach_keeps_redrawing_ends_within_one_busy_slice_and_types_nothing() {
    let mut pty = Busy {
        clock: 0,
        stop_at: 3_000,
        writes: 0,
    };
    let mut trail = Vec::new();
    let out = execute_noting(
        &the_outbox_plan(),
        &mut pty,
        &Markers,
        &mut NoQueueLog,
        &mut trail,
    );
    assert_eq!(out, Err(Failure::Timeout), "{trail:#?}");
    assert_eq!(pty.writes, 0);
    assert!(
        pty.clock <= 3_000 + ATTACH_BUSY_MS,
        "stopped at 3000 ms, ended at {} ms",
        pty.clock
    );
    assert!(trail.iter().any(|t| t.contains("stopping")), "{trail:#?}");
}

fn unfocused_then_up_at(focus_at: u64) -> Idle {
    Idle::new(vec![
        (150, drawn("idle-wake-starting.txt")),
        (1_000, drawn("idle-wake-unfocused.txt")),
        (focus_at, Look::Live),
    ])
}

struct Flip {
    inner: Idle,
    calls: usize,
    flip_at: usize,
    flipped: Option<u64>,
    looks_before_first_write: Option<usize>,
}

impl Flip {
    fn tick(&mut self) {
        self.calls += 1;
        if self.calls >= self.flip_at && self.flipped.is_none() {
            self.flipped = Some(self.inner.clock);
            self.inner.frames = vec![(0, Look::Live)];
        }
    }
}

impl Pty for Flip {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.looks_before_first_write.get_or_insert(self.calls);
        self.inner.write(bytes)
    }

    fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> bool {
        self.inner.wait_quiet(quiet, timeout)
    }

    fn screen(&mut self) -> Vec<String> {
        self.tick();
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

fn flipped_at(flip_at: usize) -> Flip {
    Flip {
        inner: unfocused_then_up_at(u64::MAX),
        calls: 0,
        flip_at,
        flipped: None,
        looks_before_first_write: None,
    }
}

#[test]
fn a_focus_that_arrives_between_two_looks_at_the_end_of_the_unfocused_wait_is_still_settled() {
    let mut steady = flipped_at(usize::MAX);
    let mut trail = Vec::new();
    let _ = execute_noting(
        &the_outbox_plan(),
        &mut steady,
        &Markers,
        &mut NoQueueLog,
        &mut trail,
    );
    let last_look = steady.looks_before_first_write.expect("a key was typed");
    for flip_at in last_look - 30..last_look {
        let mut pty = flipped_at(flip_at);
        let mut trail = Vec::new();
        let out = execute_noting(
            &the_outbox_plan(),
            &mut pty,
            &Markers,
            &mut NoQueueLog,
            &mut trail,
        );
        assert_eq!(
            out,
            Ok(Outcome { taken: false }),
            "flip at {flip_at}: {trail:#?}"
        );
        let flipped = pty.flipped.expect("the screen flipped");
        for (at, typed) in &pty.inner.writes {
            assert!(
                *at >= flipped + START_SETTLE_MS,
                "flip at look {flip_at} of {last_look}: {} bytes typed at {at} ms, focus arrived at {flipped} ms: {trail:#?}",
                typed.len()
            );
        }
    }
}

#[test]
fn a_box_that_stays_unfocused_is_waited_on_once_not_twice() {
    let mut pty = Idle::new(vec![
        (150, drawn("idle-wake-starting.txt")),
        (1_000, drawn("idle-wake-unfocused.txt")),
    ]);
    let mut trail = Vec::new();
    let _ = reply(&mut pty, &mut trail);
    let first = pty.writes.first().map(|(at, _)| *at).unwrap_or(u64::MAX);
    assert!(
        first < 1_000 + INPUT_WAIT_MS + 2_000,
        "first key at {first} ms, the box was drawn at 1000 ms: {trail:#?}"
    );
}
