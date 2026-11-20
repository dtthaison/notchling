use super::exec::Pty;
use super::fake::FakePrompt;
use super::keys::timing::{ATTACH_QUIET_MS, PROBE_TIMEOUT_MS};
use super::read::{dialog_up, read_dialog};
use std::time::Duration;

const SWITCH: &[&str] = &[
    "────────────────────────────────────────",
    " Switch model?",
    " Your next response will be slower and use more tokens",
    " ❯ 1. Yes, switch to Opus 5.5",
    "   2. No, go back",
    " Enter to confirm · Esc to cancel",
];

#[test]
fn a_dialog_on_the_screen_is_read_without_typing() {
    let mut pty = FakePrompt::new(&[]).with_dialog(SWITCH);
    let dialog = read_dialog(&mut pty).expect("a read").expect("a dialog");
    assert_eq!(dialog.title, "Switch model?");
    assert_eq!(dialog.options.len(), 2);
    assert_eq!(dialog.options[0].label, "Yes, switch to Opus 5.5");
    assert!(dialog.options[0].selected);
    assert!(pty.writes.is_empty(), "nothing typed");
    assert!(pty.dialog_shown(), "the dialog is left as it was");
}

#[test]
fn a_prompt_with_no_dialog_reads_as_none() {
    let mut pty = FakePrompt::new(&[
        "⏺ Done.",
        "────────────────────────────────────────",
        "❯",
        "────────────────────────────────────────",
        "  ? for shortcuts",
    ]);
    assert_eq!(read_dialog(&mut pty), Ok(None));
    assert!(pty.writes.is_empty(), "nothing typed");
}

#[test]
fn any_open_dialog_is_seen_without_typing() {
    let mut pty = FakePrompt::new(&[]).with_dialog(SWITCH);
    assert!(dialog_up(&mut pty));
    let mut unknown =
        FakePrompt::new(&[]).with_dialog(&[" Odd", " Enter to confirm · Esc to cancel"]);
    assert!(dialog_up(&mut unknown));
    assert!(pty.writes.is_empty() && unknown.writes.is_empty());
}

#[test]
fn a_plain_prompt_shows_no_dialog() {
    let mut pty = FakePrompt::new(&[
        "⏺ Done.",
        "────────────────────────────────────────",
        "❯",
        "────────────────────────────────────────",
        "  ? for shortcuts",
    ]);
    assert!(!dialog_up(&mut pty));
}

struct Noisy {
    lines: Vec<String>,
    waits: Vec<(u64, u64)>,
    writes: usize,
}

impl Noisy {
    fn showing(lines: &[&str]) -> Noisy {
        Noisy {
            lines: lines.iter().map(|l| l.to_string()).collect(),
            waits: Vec::new(),
            writes: 0,
        }
    }
}

impl Pty for Noisy {
    fn write(&mut self, _bytes: &[u8]) -> std::io::Result<()> {
        self.writes += 1;
        Ok(())
    }
    fn wait_quiet(&mut self, quiet: Duration, timeout: Duration) -> bool {
        self.waits
            .push((quiet.as_millis() as u64, timeout.as_millis() as u64));
        false
    }
    fn screen(&mut self) -> Vec<String> {
        self.lines.clone()
    }
    fn sleep(&mut self, _duration: Duration) {}
    fn now_ms(&self) -> u64 {
        0
    }
}

#[test]
fn a_noisy_screen_is_read_as_it_is_and_not_taken_for_a_dialog() {
    let mut closed = Noisy::showing(&["⏺ Streaming more output", "✶ Brewing… (esc to interrupt)"]);
    assert!(!dialog_up(&mut closed), "a timeout is no dialog by itself");
    let mut open = Noisy::showing(SWITCH);
    assert!(dialog_up(&mut open));
}

#[test]
fn a_probe_writes_nothing_and_waits_only_for_its_attach() {
    let mut pty = Noisy::showing(SWITCH);
    dialog_up(&mut pty);
    assert_eq!(pty.writes, 0);
    assert_eq!(pty.waits, [(ATTACH_QUIET_MS, PROBE_TIMEOUT_MS)]);
    let mut quiet = FakePrompt::new(&[]).with_dialog(SWITCH);
    let before = quiet.now_ms();
    read_dialog(&mut quiet).unwrap();
    assert_eq!(quiet.now_ms() - before, ATTACH_QUIET_MS, "no detach wait");
}

#[test]
fn prose_that_names_a_key_is_no_dialog_for_the_probe() {
    let mut pty = FakePrompt::new(&[
        "⏺ Setup is ready.",
        "  Before you go on",
        "  1. Install the tools",
        "  2. Run the tests",
        "  Press Enter to continue",
    ]);
    assert!(!dialog_up(&mut pty));
}
