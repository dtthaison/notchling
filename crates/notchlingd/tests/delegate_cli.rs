#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Dir {
    let dir = PathBuf::from(format!("/tmp/nl-dg-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".local/bin")).unwrap();
    std::fs::create_dir_all(dir.join("work")).unwrap();
    Dir(dir)
}

fn fake_codex(home: &Path, script: &str) {
    let path = home.join(".local/bin/codex");
    std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn command(home: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_notchling"));
    c.args(args)
        .current_dir(home.join("work"))
        .env("HOME", home)
        .env("NOTCHLING_DATA_ROOT", home.join("root"))
        .env("PATH", "/usr/bin:/bin")
        .env_remove("NOTCHLING_DELEGATE_DEPTH")
        .env_remove("CODEX_HOME")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    c
}

fn run(home: &Path, args: &[&str]) -> Output {
    command(home, args).output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

const THREAD: &str = r#"{"type":"thread.started","thread_id":"019e-s1"}"#;
const MESSAGE: &str =
    r#"{"type":"item.completed","item":{"type":"agent_message","text":"Looks fine."}}"#;

#[test]
fn a_codex_job_runs_read_only_prints_the_reply_and_copies_its_images() {
    let dir = scratch("job");
    let home = &dir.0;
    fake_codex(
        home,
        &format!(
            "echo \"$NOTCHLING_DELEGATE_DEPTH|$@\" > \"$HOME/seen.txt\"\n\
             mkdir -p \"$NOTCHLING_DATA_ROOT/codex/generated_images/019e-s1\"\n\
             echo img > \"$NOTCHLING_DATA_ROOT/codex/generated_images/019e-s1/cup.png\"\n\
             echo '{THREAD}'\n\
             echo '{MESSAGE}'"
        ),
    );
    let out = run(
        home,
        &[
            "delegate", "codex", "--out", "assets", "--model", "m1", "--", "draw", "a cup",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(stdout.starts_with("Codex (m1): done in "), "{stdout}");
    assert!(
        stdout.contains("Session: 019e-s1\nGenerated:\n  "),
        "{stdout}"
    );
    assert!(
        stdout.contains("Copied to:\n  /") && stdout.contains("/work/assets/cup.png\n"),
        "{stdout}"
    );
    assert!(stdout.ends_with("\n\nLooks fine.\n"), "{stdout}");
    assert_eq!(
        std::fs::read_to_string(home.join("work/assets/cup.png")).unwrap(),
        "img\n"
    );

    let seen = std::fs::read_to_string(home.join("seen.txt")).unwrap();
    assert!(
        seen.starts_with("1|exec --json --skip-git-repo-check -c sandbox_mode=\"read-only\""),
        "{seen}"
    );
    assert!(seen.trim_end().ends_with("draw a cup"), "{seen}");
    assert!(!seen.contains("network_access"), "{seen}");
}

#[test]
fn json_and_the_exit_code_of_a_failed_job() {
    let dir = scratch("fail");
    fake_codex(
        &dir.0,
        "echo '{\"type\":\"error\",\"message\":\"usage limit\"}'\nexit 1",
    );
    let out = run(&dir.0, &["delegate", "codex", "--json", "x"]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_str(&text(&out.stdout)).unwrap();
    assert_eq!(
        (v["ok"].as_bool(), v["error"].as_str()),
        (Some(false), Some("usage limit"))
    );
    assert_eq!(v["tool"], "codex");
}

#[test]
fn a_third_level_of_delegation_is_refused_without_starting_anything() {
    let dir = scratch("depth");
    fake_codex(&dir.0, "touch \"$HOME/ran\"");
    let out = command(&dir.0, &["delegate", "codex", "x"])
        .env("NOTCHLING_DELEGATE_DEPTH", "2")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        text(&out.stderr),
        "This job was itself delegated 2 times; do it yourself instead of delegating again.\n"
    );
    assert_eq!(text(&out.stdout), "");
    assert!(!dir.0.join("ran").exists());
}

#[test]
fn a_bad_command_line_prints_the_usage_and_exits_2() {
    let dir = scratch("usage");
    for args in [
        &["delegate"][..],
        &["delegate", "claude", "x"],
        &["delegate", "codex"],
    ] {
        let out = run(&dir.0, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(
            text(&out.stderr).contains("Usage: notchling delegate"),
            "{args:?}"
        );
    }
}

#[test]
fn delegate_models_lists_codexs_cached_models() {
    let dir = scratch("models");
    let codex = dir.0.join("root/codex");
    std::fs::create_dir_all(&codex).unwrap();
    std::fs::write(
        codex.join("models_cache.json"),
        r#"{"models":[{"slug":"gpt-a","visibility":"list","default_reasoning_level":"medium",
            "supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"}]}]}"#,
    )
    .unwrap();
    let out = run(&dir.0, &["delegate-models", "codex"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        text(&out.stdout),
        "codex (--model <slug> --effort <level>):\n  gpt-a  efforts: low, medium (default medium)\n"
    );
}

#[test]
fn a_stop_signal_stops_the_tool_and_reports_it() {
    let dir = scratch("stop");
    fake_codex(
        &dir.0,
        &format!("echo '{THREAD}'\necho started > \"$HOME/started\"\nexec sleep 30"),
    );
    let child = command(&dir.0, &["delegate", "codex", "--json", "slow"])
        .spawn()
        .unwrap();
    let begin = Instant::now();
    while !dir.0.join("started").exists() {
        assert!(
            begin.elapsed() < Duration::from_secs(20),
            "the fake tool never started"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    let v: serde_json::Value = serde_json::from_str(&text(&out.stdout)).unwrap();
    assert_eq!(v["error"], "Codex was stopped.");
    assert_eq!(v["session"], "019e-s1");
    assert!(v.get("timedOut").is_none());
    assert!(begin.elapsed() < Duration::from_secs(25));
}
