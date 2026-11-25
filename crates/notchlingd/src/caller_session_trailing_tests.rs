use crate::caller_session::session_id_in;
use crate::hook::headless;
use std::path::PathBuf;

const SESSION: &str = "6f1c2a9e-3b7d-4e21-9a0c-5d8e7f6a1b2c";

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Scratch {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "notchlingd-trailing-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    Scratch(dir)
}

#[test]
fn a_session_file_with_a_stray_closing_brace_still_names_its_session() {
    let s = scratch("id");
    let file = s.0.join("300.json");
    std::fs::write(&file, format!("{{\"sessionId\":\"{SESSION}\"}}}}")).unwrap();
    assert_eq!(session_id_in(&file).as_deref(), Some(SESSION));
    std::fs::write(&file, format!("{{\"sessionId\":\"{SESSION}")).unwrap();
    assert_eq!(session_id_in(&file), None, "a half-written file names none");
}

#[test]
fn a_headless_session_file_with_a_stray_closing_brace_is_still_headless() {
    let s = scratch("headless");
    std::fs::write(
        s.0.join("4242.json"),
        "{\"pid\":4242,\"entrypoint\":\"sdk-cli\"}}",
    )
    .unwrap();
    assert!(headless(&s.0, &[4242]));
}
