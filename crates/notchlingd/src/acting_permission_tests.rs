//! `session.permission` for a permission no hook holds: read off the job's screen, checked against
//! what the app showed, answered with one digit. A fake terminal whose dialog closes on a digit
//! (made up from Claude Code's known layout; no recording of the dialog exists yet).

use crate::acting::{Acting, ACTING_OFF};
use crate::acting_ops::{ActingOps, Attacher};
use crate::acting_ops_tests::{app, code, row, FakeRoom};
use notchling_engine::reply::exec::{Markers, Pty, ScreenCheck};
use notchling_engine::sessions::Roots;
use notchling_local_api::{ops, ApiError, ErrorCode};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn lines(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|r| r.to_string()).collect()
}

fn dialog() -> Vec<String> {
    let rule = "─".repeat(120);
    lines(&[
        "⏺ Bash(ls -la && pwd)",
        &rule,
        " Bash command",
        "",
        "   ls -la && pwd",
        "   List the folder",
        "",
        " Do you want to proceed?",
        " ❯ 1. Yes",
        "   2. Yes, and don't ask again for ls commands in /tmp/demo",
        "   3. No, and tell Claude what to do differently (esc)",
        "",
        " Esc to cancel",
    ])
}

/// Shows `screen` until a digit is typed, then the prompt.
struct DialogPty {
    screen: Vec<String>,
    writes: Arc<Mutex<Vec<String>>>,
    clock: u64,
}

impl Pty for DialogPty {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let keys = String::from_utf8_lossy(bytes).into_owned();
        if keys.chars().all(|c| c.is_ascii_digit()) {
            self.screen = lines(&["> ", "  ? for shortcuts"]);
        }
        self.writes.lock().unwrap().push(keys);
        Ok(())
    }
    fn wait_quiet(&mut self, quiet: Duration, _timeout: Duration) -> bool {
        self.clock += quiet.as_millis() as u64;
        true
    }
    fn screen(&mut self) -> Vec<String> {
        self.screen.clone()
    }
    fn sleep(&mut self, d: Duration) {
        self.clock += d.as_millis() as u64;
    }
    fn now_ms(&self) -> u64 {
        self.clock
    }
}

struct DialogAttacher {
    screen: Vec<String>,
    attached: Arc<Mutex<Vec<String>>>,
    writes: Arc<Mutex<Vec<String>>>,
}

impl Attacher for DialogAttacher {
    fn attach(&self, job: &str, _config_dir: Option<&str>) -> Result<Box<dyn Pty>, ApiError> {
        self.attached.lock().unwrap().push(job.to_string());
        Ok(Box::new(DialogPty {
            screen: self.screen.clone(),
            writes: self.writes.clone(),
            clock: 0,
        }))
    }
    fn screen_check(&self) -> &dyn ScreenCheck {
        &Markers
    }
}

struct Setup {
    ops: ActingOps,
    attached: Arc<Mutex<Vec<String>>>,
    writes: Arc<Mutex<Vec<String>>>,
}

fn setup(on: bool, screen: Vec<String>) -> Setup {
    let mut waiting = row("job-p", json!({"job": "perm0001"}), "needsInput", None);
    waiting["detail"] = json!("approve Bash: ls -la && pwd");
    let list = json!([
        waiting,
        row("term", json!({"process": 4242}), "needsInput", None)
    ]);
    let room = Arc::new(FakeRoom::default());
    room.app.store(true, Ordering::SeqCst);
    let attached = Arc::new(Mutex::new(Vec::new()));
    let writes = Arc::new(Mutex::new(Vec::new()));
    let ops = ActingOps::new(
        Arc::new(Acting::in_memory(on)),
        Arc::new(Mutex::new(list)),
        Roots {
            main_dir: PathBuf::from("/nonexistent/claude"),
            lanes_file: PathBuf::from("/nonexistent/lanes.json"),
        },
        room,
        Box::new(DialogAttacher {
            screen,
            attached: attached.clone(),
            writes: writes.clone(),
        }),
    );
    Setup {
        ops,
        attached,
        writes,
    }
}

fn ask(s: &Setup, args: Value) -> Result<Value, ApiError> {
    s.ops.call(&app(), ops::SESSION_PERMISSION, &args)
}

fn args(decision: &str, tool: &str, summary: &str) -> Value {
    json!({"id": "job-p", "decision": decision, "expect": {"tool": tool, "summary": summary}})
}

#[test]
fn each_decision_types_its_options_digit_and_nothing_else() {
    for (decision, digit) in [("allow", "1"), ("always", "2"), ("deny", "3")] {
        let s = setup(true, dialog());
        let out = ask(&s, args(decision, "Bash", "ls -la && pwd")).unwrap();
        assert_eq!(out, json!({"decided": decision, "answered": "terminal"}));
        assert_eq!(*s.attached.lock().unwrap(), ["perm0001"]);
        assert_eq!(*s.writes.lock().unwrap(), [digit], "{decision}");
    }
}

#[test]
fn a_dialog_about_something_else_is_refused_with_nothing_typed() {
    let s = setup(true, dialog());
    let (c, m) = code(ask(&s, args("allow", "Bash", "rm -rf /tmp/demo")));
    assert_eq!(c, ErrorCode::Failed);
    assert!(
        m.starts_with("The terminal asks about something else"),
        "{m}"
    );
    let (c, _) = code(ask(&s, args("allow", "Write", "ls -la && pwd")));
    assert_eq!(c, ErrorCode::Failed);
    assert!(s.writes.lock().unwrap().is_empty());
}

#[test]
fn no_dialog_on_screen_is_refused_with_nothing_typed() {
    let s = setup(true, lines(&["> ", "  ? for shortcuts"]));
    let (c, m) = code(ask(&s, args("deny", "Bash", "ls -la && pwd")));
    assert_eq!(c, ErrorCode::Failed);
    assert!(m.contains("isn't showing that permission"), "{m}");
    assert!(s.writes.lock().unwrap().is_empty());
}

/// The dialog reader was written from made-up screens (docs/recording.md, `dialog-permission`):
/// a screen it refuses is kept, private, so a real one can become the fixture.
#[test]
fn a_refused_screen_is_kept_private_and_a_typed_answer_keeps_none() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("nl-perm-screen-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let file = dir.join("permission-screen.txt");

    let s = setup(true, lines(&["> ", "  ? for shortcuts"]));
    let ops = s.ops.with_screen_file(file.clone());
    let refused = ops.call(
        &app(),
        ops::SESSION_PERMISSION,
        &args("allow", "Bash", "ls -la && pwd"),
    );
    assert!(refused.is_err());
    let kept = std::fs::read_to_string(&file).unwrap();
    assert!(kept.contains("? for shortcuts"), "{kept}");
    assert!(
        kept.contains("Bash: ls -la && pwd"),
        "the card's request is kept too: {kept}"
    );
    let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    let s = setup(true, dialog());
    let ops = s.ops.with_screen_file(file.clone());
    std::fs::remove_file(&file).unwrap();
    ops.call(
        &app(),
        ops::SESSION_PERMISSION,
        &args("allow", "Bash", "ls -la && pwd"),
    )
    .unwrap();
    assert!(!file.exists(), "nothing is kept when the answer went in");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_args_are_checked_before_anything_is_attached() {
    let s = setup(true, dialog());
    let bad = [
        json!({"id": "job-p", "decision": "allow"}),
        json!({"id": "job-p", "decision": "maybe", "expect": {"tool": "Bash", "summary": ""}}),
        json!({"id": "job-p", "decision": "allow", "expect": {"summary": "ls"}}),
        json!({"id": "job-p", "decision": "allow", "expect": {"tool": " ", "summary": "ls"}}),
        json!({"id": "job-p", "decision": "allow", "expect": {"tool": "Bash", "summary": 3}}),
        json!({"id": "job-p", "decision": "allow", "expect": "Bash"}),
        json!({"id": "job-p", "decision": "deny", "message": "no", "expect": {"tool": "Bash"}}),
        json!({"id": "nope", "decision": "allow", "expect": {"tool": "Bash", "summary": ""}}),
        json!({"decision": "allow", "expect": {"tool": "Bash", "summary": ""}}),
    ];
    for a in bad {
        assert_eq!(code(ask(&s, a.clone())).0, ErrorCode::BadRequest, "{a}");
    }
    // A terminal session has no attach terminal to type into.
    let (c, m) = code(ask(
        &s,
        json!({"id": "term", "decision": "allow", "expect": {"tool": "Bash", "summary": ""}}),
    ));
    assert_eq!(
        (c, m.as_str()),
        (ErrorCode::BadRequest, "this session takes no replies")
    );
    assert!(s.attached.lock().unwrap().is_empty());
    // A summary left out is the tool alone.
    let out = ask(
        &s,
        json!({"id": "job-p", "decision": "allow", "expect": {"tool": "Bash"}}),
    );
    assert_eq!(
        out.unwrap(),
        json!({"decided": "allow", "answered": "terminal"})
    );
}

#[test]
fn refused_while_acting_is_off() {
    let s = setup(false, dialog());
    assert_eq!(
        code(ask(&s, args("allow", "Bash", "ls -la && pwd"))),
        (ErrorCode::Forbidden, ACTING_OFF.to_string())
    );
    assert!(s.attached.lock().unwrap().is_empty());
}
