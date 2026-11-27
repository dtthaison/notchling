use notchling_engine::reply::exec::{execute_noting, Failure, Markers, NoQueueLog, Outcome, Pty};
use notchling_engine::reply::keys::{self, timing::*};
use notchling_engine::reply::plan;
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

const BOX_ROW: usize = 46;

struct Waking {
    frames: Vec<(u64, Shot)>,
    up_at: u64,
    clock: u64,
    held: String,
    sent: Vec<String>,
    writes: Vec<(u64, String)>,
}

impl Waking {
    fn new(start_ms: u64) -> Waking {
        let up_at = 250 + start_ms + 90;
        Waking {
            frames: vec![
                (0, (vec![String::new(); 50], Some((0, 0)))),
                (110, shot("attach-waking.txt")),
                (250, shot("attach-starting.txt")),
                (250 + start_ms, shot("started-mid-draw.txt")),
                (up_at, shot("started.txt")),
            ],
            up_at,
            clock: 0,
            held: String::new(),
            sent: Vec::new(),
            writes: Vec::new(),
        }
    }

    fn up(&self) -> bool {
        self.clock >= self.up_at
    }

    fn frame(&self) -> &Shot {
        let now = self.frames.iter().rev().find(|(at, _)| *at <= self.clock);
        &now.unwrap_or(&self.frames[0]).1
    }
}

impl Pty for Waking {
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
        let mut lines = self.frame().0.clone();
        if self.up() {
            lines[BOX_ROW] = format!("❯\u{a0}{}", self.held);
        }
        lines
    }

    fn cursor(&mut self) -> Option<(usize, usize)> {
        match self.up() {
            true => Some((BOX_ROW, 2 + self.held.chars().count())),
            false => self.frame().1,
        }
    }

    fn sleep(&mut self, duration: Duration) {
        self.clock += duration.as_millis() as u64;
    }

    fn now_ms(&self) -> u64 {
        self.clock
    }
}

fn reply(pty: &mut Waking, trail: &mut Vec<String>) -> Result<Outcome, Failure> {
    execute_noting(
        &plan::reply("what is left to do on this one?"),
        pty,
        &Markers,
        &mut NoQueueLog,
        trail,
    )
}

#[test]
fn a_reply_to_a_session_the_attach_had_to_wake_waits_for_its_prompt() {
    let mut pty = Waking::new(1_900);
    let mut trail = Vec::new();
    let out = reply(&mut pty, &mut trail);
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    assert_eq!(pty.sent, ["what is left to do on this one?"]);
    assert_eq!(pty.writes.len(), 3);
    for (at, keys) in &pty.writes {
        assert!(
            *at >= pty.up_at + START_SETTLE_MS,
            "{keys:?} typed at {at} ms, the prompt came up at {} ms",
            pty.up_at
        );
    }
    assert!(
        trail[0].contains("starting: Session is starting"),
        "{trail:#?}"
    );
}

#[test]
fn a_session_that_starts_faster_than_the_attach_goes_quiet_is_typed_into_at_once() {
    let mut pty = Waking::new(230);
    let mut trail = Vec::new();
    let out = reply(&mut pty, &mut trail);
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    assert_eq!(pty.sent, ["what is left to do on this one?"]);
    assert_eq!(pty.writes[0].0, pty.up_at + ATTACH_QUIET_MS);
    assert!(trail.iter().all(|t| !t.contains("starting")), "{trail:#?}");
}

#[test]
fn a_session_that_takes_too_long_to_start_gets_no_key_and_says_so() {
    let mut pty = Waking::new(BOOT_WAIT_MS + 5_000);
    let mut trail = Vec::new();
    assert_eq!(reply(&mut pty, &mut trail), Err(Failure::Timeout));
    assert!(pty.writes.is_empty());
    assert!(pty.sent.is_empty());
}
