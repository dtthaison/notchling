use notchling_engine::reply::exec::{execute_noting, Failure, Markers, NoQueueLog, Outcome, Pty};
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
const TEXT: &str = "can you look at the failing check again?";
const DRAFT: &str = "a note somebody typed here";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum See {
    Up,
    Loose,
    Blank,
    Starting,
    Between,
    Picker,
    Dialog,
}

use See::*;

#[derive(Clone, Copy, Debug)]
struct Key {
    at: u64,
    call: usize,
    on: See,
    boxed: Option<u64>,
    focused: Option<u64>,
}

struct Screens {
    script: Vec<(usize, See)>,
    calls: usize,
    clock: u64,
    now: See,
    held: String,
    sent: Vec<String>,
    writes: Vec<(u64, String)>,
    boxed: Option<u64>,
    focused: Option<u64>,
    first_key: Option<Key>,
}

impl Screens {
    fn new(script: &[(usize, See)]) -> Screens {
        Screens {
            script: script.to_vec(),
            calls: 0,
            clock: 0,
            now: script[0].1,
            held: String::new(),
            sent: Vec::new(),
            writes: Vec::new(),
            boxed: None,
            focused: None,
            first_key: None,
        }
    }

    fn drafting(mut self, draft: &str) -> Screens {
        self.held = draft.to_string();
        self
    }

    fn live(&self) -> Vec<String> {
        vec![
            String::new(),
            "  An earlier line of the session.".into(),
            String::new(),
            RULE.into(),
            format!("❯\u{a0}{}", self.held),
            RULE.into(),
            "  ? for shortcuts".into(),
        ]
    }

    fn drawn(&self) -> Shot {
        match self.now {
            Blank => shot("idle-wake-blank.txt"),
            Starting => shot("idle-wake-starting.txt"),
            Between => shot("idle-wake-between.txt"),
            Picker => shot("idle-wake-picker.txt"),
            Dialog => (
                [
                    "",
                    "  Do you want to proceed?",
                    "  ❯ 1. Yes",
                    "    2. No",
                    "",
                    "  Esc to cancel",
                ]
                .map(String::from)
                .to_vec(),
                None,
            ),
            Up | Loose => (self.live(), None),
        }
    }
}

impl Pty for Screens {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let typed = String::from_utf8_lossy(bytes).into_owned();
        self.first_key.get_or_insert(Key {
            at: self.clock,
            call: self.calls,
            on: self.now,
            boxed: self.boxed,
            focused: self.focused,
        });
        self.writes.push((self.clock, typed.clone()));
        if !matches!(self.now, Up | Loose) {
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

    fn wait_quiet(&mut self, quiet: Duration, _timeout: Duration) -> bool {
        self.clock += quiet.as_millis() as u64;
        true
    }

    fn screen(&mut self) -> Vec<String> {
        self.calls += 1;
        let calls = self.calls;
        if let Some((_, see)) = self.script.iter().rev().find(|(from, _)| *from <= calls) {
            self.now = *see;
        }
        let at = self.clock;
        match self.now {
            Up => {
                self.boxed.get_or_insert(at);
                self.focused.get_or_insert(at);
            }
            Loose => {
                self.boxed.get_or_insert(at);
                self.focused = None;
            }
            _ => (self.boxed, self.focused) = (None, None),
        }
        self.drawn().0
    }

    fn cursor(&mut self) -> Option<(usize, usize)> {
        match self.now {
            Up => Some((BOX_ROW, 2 + self.held.chars().count())),
            _ => self.drawn().1,
        }
    }

    fn sleep(&mut self, duration: Duration) {
        self.clock += duration.as_millis() as u64;
    }

    fn now_ms(&self) -> u64 {
        self.clock
    }
}

fn run(plan: &Plan, pty: &mut Screens) -> (Result<Outcome, Failure>, Vec<String>) {
    let mut trail = Vec::new();
    let out = execute_noting(plan, pty, &Markers, &mut NoQueueLog, &mut trail);
    (out, trail)
}

fn waking() -> Plan {
    plan::reply_waking(TEXT)
}

fn plain() -> Plan {
    plan::reply(TEXT)
}

fn settled(pty: &Screens) -> Result<(), String> {
    let Some(key) = pty.first_key else {
        return Ok(());
    };
    let stood = |from: Option<u64>| from.map(|at| key.at - at);
    match (key.on, stood(key.focused), stood(key.boxed)) {
        (Up, Some(ms), _) if ms >= START_SETTLE_MS => Ok(()),
        (Loose, _, Some(ms)) if ms >= START_SETTLE_MS => Ok(()),
        _ => Err(format!("first key: {key:?}")),
    }
}

fn steady(plan: &Plan, script: &[(usize, See)]) -> usize {
    let mut pty = Screens::new(script);
    let (out, trail) = run(plan, &mut pty);
    assert_eq!(out, Ok(Outcome { taken: false }), "{trail:#?}");
    pty.first_key.expect("a key was typed").call
}

#[test]
fn a_box_focused_from_the_first_look_after_the_boot_wait_is_settled_before_the_first_key() {
    for up in 7..=15 {
        let mut pty = Screens::new(&[(1, Blank), (6, Between), (up, Up)]);
        let (out, trail) = run(&waking(), &mut pty);
        assert_eq!(out, Ok(Outcome { taken: false }), "up at {up}: {trail:#?}");
        assert_eq!(pty.sent, [TEXT], "up at {up}");
        settled(&pty).unwrap_or_else(|e| panic!("up at {up}: {e}: {trail:#?}"));
    }
}

#[test]
fn a_box_focused_right_after_a_dialog_in_the_boot_wait_is_settled_before_the_first_key() {
    let mut pty = Screens::new(&[(1, Starting), (5, Dialog), (6, Up)]);
    let (out, trail) = run(&waking(), &mut pty);
    if out.is_ok() {
        settled(&pty).unwrap_or_else(|e| panic!("{e}: {trail:#?}"));
    }
    assert!(
        out.is_ok() || pty.writes.is_empty(),
        "{out:?}: {:?}",
        pty.writes
    );
}

#[test]
fn a_screen_that_leaves_the_box_at_the_end_of_the_unfocused_wait_gets_no_key() {
    let base = [(1, Starting), (4, Loose)];
    let last = steady(&waking(), &base);
    for see in [Picker, Dialog, Blank, Starting] {
        for at in last - 20..=last {
            let mut pty = Screens::new(&[base[0], base[1], (at, see)]);
            let (out, trail) = run(&waking(), &mut pty);
            assert!(
                pty.writes.is_empty(),
                "{see:?} from look {at} of {last}: {out:?}, keys {:?}: {trail:#?}",
                pty.writes
            );
            assert!(out.is_err(), "{see:?} from look {at}");
        }
    }
}

#[test]
fn a_focus_that_flickers_off_starts_the_settle_again() {
    for (name, plan) in [("plain", plain()), ("waking", waking())] {
        let mut pty = Screens::new(&[(1, Between), (3, Up), (4, Loose), (8, Up)]);
        let (out, trail) = run(&plan, &mut pty);
        assert_eq!(out, Ok(Outcome { taken: false }), "{name}: {trail:#?}");
        settled(&pty).unwrap_or_else(|e| panic!("{name}: {e}: {trail:#?}"));
    }
}

#[test]
fn a_focus_that_flickers_at_the_end_of_the_unfocused_wait_starts_the_settle_again() {
    let base = [(1, Starting), (4, Loose)];
    let last = steady(&waking(), &base);
    for at in last - 20..=last + 2 {
        let script = [base[0], base[1], (at, Up), (at + 1, Loose), (at + 4, Up)];
        let mut pty = Screens::new(&script);
        let (out, trail) = run(&waking(), &mut pty);
        assert!(
            out.is_ok(),
            "flicker at {at} of {last}: {out:?}: {trail:#?}"
        );
        settled(&pty).unwrap_or_else(|e| panic!("flicker at {at} of {last}: {e}: {trail:#?}"));
    }
}

#[test]
fn a_screen_that_leaves_the_box_while_its_text_is_settled_gets_no_key() {
    let base = [(1, Starting), (4, Up)];
    let mut pty = Screens::new(&base).drafting(DRAFT);
    let (out, trail) = run(&waking(), &mut pty);
    assert_eq!(out, Err(Failure::NotCleared), "{trail:#?}");
    let last = pty.calls;
    for see in [Picker, Dialog, Blank] {
        for at in last - 6..=last {
            let mut pty = Screens::new(&[base[0], base[1], (at, see)]).drafting(DRAFT);
            let (out, trail) = run(&waking(), &mut pty);
            assert!(
                pty.writes.is_empty(),
                "{see:?} from look {at} of {last}: {out:?}, keys {:?}: {trail:#?}",
                pty.writes
            );
        }
    }
}

#[test]
fn a_session_already_at_its_prompt_gets_the_reply_at_once_on_either_plan() {
    for (name, plan) in [("plain", plain()), ("waking", waking())] {
        let mut pty = Screens::new(&[(1, Up)]);
        let (out, trail) = run(&plan, &mut pty);
        assert_eq!(out, Ok(Outcome { taken: false }), "{name}: {trail:#?}");
        assert_eq!(pty.sent, [TEXT], "{name}");
        assert_eq!(
            pty.first_key.map(|k| k.at),
            Some(ATTACH_QUIET_MS),
            "{name}: {trail:#?}"
        );
    }
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self, below: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % below
    }
}

fn shuffled(seed: u64) -> Vec<(usize, See)> {
    const LOOKS: [See; 9] = [Up, Up, Up, Loose, Loose, Between, Starting, Blank, Picker];
    let mut rng = Lcg(seed);
    let mut script = vec![(1, Starting)];
    let mut call = 4;
    while call < 160 {
        script.push((call, LOOKS[rng.next(LOOKS.len() as u64) as usize]));
        call += 1 + rng.next(8) as usize;
    }
    script.push((call, Up));
    script
}

#[test]
fn no_run_of_screens_gets_a_key_outside_a_box_that_has_stood() {
    for seed in 0..300 {
        let script = shuffled(seed);
        for (name, plan) in [("plain", plain()), ("waking", waking())] {
            let mut pty = Screens::new(&script);
            let (out, trail) = run(&plan, &mut pty);
            settled(&pty).unwrap_or_else(|e| {
                panic!("{name} seed {seed}: {e}, {out:?}: {script:?}: {trail:#?}")
            });
        }
    }
}
