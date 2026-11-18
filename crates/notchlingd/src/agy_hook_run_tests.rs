use super::*;
use crate::agy_link::{DaemonLink, LinkError};
use notchling_engine::tools::agy_sidecar::{PRE_INVOCATION, PRE_TOOL};
use std::time::Instant;

struct Scripted {
    answer: Result<Value, LinkError>,
    calls: Vec<(String, Value, Duration)>,
}

impl Scripted {
    fn answering(answer: Result<Value, LinkError>) -> Scripted {
        Scripted {
            answer,
            calls: Vec::new(),
        }
    }
}

impl Link for Scripted {
    fn call(&mut self, op: &str, args: Value, within: Duration) -> Result<Value, LinkError> {
        self.calls.push((op.to_string(), args, within));
        self.answer.clone()
    }
}

const INPUT: &[u8] = br#"{"conversationId":"457369a9-35a1-40f8-92b9-26b6398b78a1"}"#;

#[test]
fn the_daemons_output_is_what_the_hook_prints() {
    let out = json!({"decision": "auto_approve"});
    let mut link = Scripted::answering(Ok(json!({"output": out})));
    assert_eq!(decide(&mut link, PRE_TOOL, INPUT), Some(out));
    let (op, args, _) = &link.calls[0];
    assert_eq!(op, ops::AGY_HOOK);
    assert_eq!(args["event"], PRE_TOOL);
    assert_eq!(args["input"].as_str().unwrap().as_bytes(), INPUT);
}

#[test]
fn any_trouble_prints_nothing() {
    for answer in [
        Err(LinkError::Gone),
        Err(LinkError::Refused("no".into())),
        Ok(json!({"output": null})),
        Ok(json!({"output": "text"})),
        Ok(json!({})),
        Ok(Value::Null),
    ] {
        let mut link = Scripted::answering(answer.clone());
        assert_eq!(decide(&mut link, PRE_INVOCATION, INPUT), None, "{answer:?}");
    }
}

#[test]
fn an_unknown_event_or_unreadable_input_never_reaches_the_daemon() {
    let mut link = Scripted::answering(Ok(json!({"output": {"x": 1}})));
    assert_eq!(decide(&mut link, "post-tool", INPUT), None);
    assert_eq!(decide(&mut link, PRE_TOOL, &[0xff, 0xfe]), None);
    assert!(link.calls.is_empty());
}

#[test]
fn the_hook_always_finishes_inside_the_time_antigravity_gives_it() {
    for event in [PRE_TOOL, PRE_INVOCATION] {
        assert!(worst_case(event) < allowed(event), "{event}");
    }
    let mut link = Scripted::answering(Err(LinkError::Gone));
    decide(&mut link, PRE_INVOCATION, INPUT);
    assert!(link.calls[0].2 < allowed(PRE_INVOCATION));
}

fn socket(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-agyhook-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("d.sock")
}

#[test]
fn no_daemon_means_an_instant_nothing() {
    let started = Instant::now();
    assert!(DaemonLink::connect(&socket("none"), Duration::from_secs(1)).is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[cfg(unix)]
#[test]
fn a_daemon_that_never_answers_cannot_hold_the_hook_past_its_time() {
    let path = socket("mute");
    let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    let keep = std::thread::spawn(move || {
        let held = listener.accept();
        std::thread::sleep(Duration::from_secs(4));
        drop(held);
    });
    let started = Instant::now();
    let within = Duration::from_millis(500);
    assert!(DaemonLink::connect(&path, within).is_err());
    assert!(started.elapsed() < within + Duration::from_secs(2));
    drop(keep);
}
