//! The real `ProcessRunner` with harmless local commands only, and the scripted fake (Swift
//! `MachinesRunnerTests`): the environment a check runs in, what a timeout leaves behind, stdin, the
//! output cap. Nothing here starts ssh, `gh`, `git` or `sudo`.
//!
//! Not ported: the lost-exit-status case (std reaps its child itself, so there is no way to lose
//! it), and the runner-without-stdin-support case (the trait has one `run`, so a runner cannot
//! drop a stdin).

use super::fakes::{lock, TempDir};
use super::process::{ProcessRunner, CUT_NOTICE, OUTPUT_CAP};
use super::runner::{argv, Call, Channel, CommandResult, Refusing, Runner, ScriptedRunner};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Whether the process is running. A zombie (ended, waiting to be reaped) is not.
fn is_running(pid: i32) -> bool {
    let out = std::process::Command::new("/bin/ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output();
    let Ok(out) = out else { return false };
    let stat = String::from_utf8_lossy(&out.stdout).trim().to_string();
    !stat.is_empty() && !stat.starts_with('Z')
}

/// The pids of `pids` still running after they had up to `secs` to go.
fn still_running(pids: &[i32], secs: u64) -> Vec<i32> {
    let end = Instant::now() + Duration::from_secs(secs);
    let mut alive: Vec<i32> = pids.iter().copied().filter(|p| is_running(*p)).collect();
    while !alive.is_empty() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(100));
        alive.retain(|p| is_running(*p));
    }
    alive
}

/// What a test script wrote about its own processes. The tests find their children by these pids and
/// never by a process name, which another test run on the same machine can share.
#[derive(Default, Debug)]
struct Spawned {
    shell: Option<i32>,
    children: Vec<(i32, Option<i32>)>,
}

impl Spawned {
    fn all(&self) -> Vec<i32> {
        self.shell
            .into_iter()
            .chain(self.children.iter().map(|c| c.0))
            .collect()
    }

    fn kill_all(&self) {
        for pid in self.all() {
            // SAFETY: a signal to a pid a test script printed; a gone pid answers ESRCH.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

/// `shell <pid>` and `child <pid> [<process group>]` lines, as the scripts below write them.
fn spawned(file: &std::path::Path) -> Spawned {
    let mut found = Spawned::default();
    let text = std::fs::read_to_string(file).unwrap_or_default();
    for line in text.lines() {
        let words: Vec<&str> = line.split(' ').collect();
        if words.len() == 2 && words[0] == "shell" {
            found.shell = words[1].parse().ok();
        }
        if words.len() >= 2 && words[0] == "child" {
            if let Ok(pid) = words[1].parse() {
                found
                    .children
                    .push((pid, words.get(2).and_then(|g| g.parse().ok())));
            }
        }
    }
    found
}

/// Writes the shell's pid, then starts two sleeps in the background and writes each one's pid and
/// process group.
fn two_sleeps(file: &std::path::Path, prefix: &str) -> String {
    let f = format!("'{}'", file.display());
    let note = r#"echo "child $! $(ps -o pgid= -p $! | tr -d ' ')""#;
    format!("{prefix}echo \"shell $$\" >> {f}; sleep 30 & {note} >> {f}; sleep 30 & {note} >> {f}; wait")
}

fn sh(script: &str, secs: u64) -> Call {
    Call::secs(argv(&["/bin/sh", "-c", script]), secs)
}

#[test]
fn environment_is_laid_over_the_apps_own() {
    let call = sh(r#"printf '%s|%s' "$MACHINES_TEST_VAR" "$HOME""#, 5)
        .env("MACHINES_TEST_VAR", "from-the-caller");
    let r = ProcessRunner.run(&call);
    assert_eq!(r.code, 0);
    assert_eq!(
        r.out,
        format!(
            "from-the-caller|{}",
            std::env::var("HOME").unwrap_or_default()
        )
    );
}

#[test]
fn no_environment_override_keeps_the_apps_own() {
    let r = ProcessRunner.run(&sh(r#"printf '%s' "$HOME""#, 5));
    assert_eq!(r.out, std::env::var("HOME").unwrap_or_default());
}

#[test]
fn cwd_is_used() {
    let r = ProcessRunner.run(&Call::secs(argv(&["/bin/pwd", "-P"]), 5).cwd("/usr".into()));
    assert_eq!(r.out, "/usr\n");
}

#[test]
fn a_start_that_fails_is_127() {
    let r =
        ProcessRunner.run(&Call::secs(argv(&["/bin/pwd"]), 5).cwd("/nonexistent/dir/here".into()));
    assert_eq!(r.code, 127);
}

#[test]
fn timeout_kills_the_whole_process_group() {
    let dir = TempDir::new("run");
    let file = dir.join("pids");
    let start = Instant::now();
    let r = ProcessRunner.run(&sh(&two_sleeps(&file, ""), 2));
    let seen = spawned(&file);
    assert_eq!(r.code, 124);
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "the run waited for the sleeps to end"
    );
    assert_eq!(seen.children.len(), 2, "both sleeps started");
    // the sleeps are in the group the runner made for the shell, so a kill of the group reaches them
    assert!(seen.shell.is_some());
    assert!(
        seen.children.iter().all(|c| c.1 == seen.shell),
        "a sleep is outside the shell's group: {seen:?}"
    );
    let left = still_running(&seen.all(), 5);
    seen.kill_all();
    assert!(
        left.is_empty(),
        "a process of the group outlived its timeout: {left:?}"
    );
}

#[test]
fn timeout_kills_a_group_that_ignores_term() {
    let dir = TempDir::new("run");
    let file = dir.join("pids");
    let start = Instant::now();
    let r = ProcessRunner.run(&sh(&two_sleeps(&file, "trap '' TERM; "), 2));
    let seen = spawned(&file);
    assert_eq!(r.code, 124);
    assert!(start.elapsed() < Duration::from_secs(12));
    assert_eq!(seen.children.len(), 2, "both sleeps started");
    assert!(seen.shell.is_some());
    assert!(
        seen.children.iter().all(|c| c.1 == seen.shell),
        "a sleep is outside the shell's group: {seen:?}"
    );
    // SIGTERM is ignored by the shell and (inherited) by its sleeps: only the kill that follows
    // ends them
    let left = still_running(&seen.all(), 5);
    seen.kill_all();
    assert!(
        left.is_empty(),
        "a process that ignores SIGTERM outlived its timeout: {left:?}"
    );
}

#[test]
fn an_orphan_holding_the_pipe_does_not_hold_the_result() {
    let dir = TempDir::new("run");
    let file = dir.join("pids");
    let start = Instant::now();
    let r = ProcessRunner.run(&sh(
        &format!(
            "(sleep 12 & echo \"child $!\" >> '{}') ; echo done",
            file.display()
        ),
        60,
    ));
    spawned(&file).kill_all();
    assert_eq!(r.code, 0);
    assert_eq!(r.out, "done\n");
    assert!(start.elapsed() < Duration::from_secs(8));
}

// MARK: stdin, streaming, the cap, orphans of a normal exit

#[test]
fn stdin_reaches_the_command() {
    let call = Call::secs(argv(&["/bin/cat"]), 5).stdin(b"line one\nline two".to_vec());
    let r = ProcessRunner.run(&call);
    assert_eq!(r.code, 0);
    assert_eq!(r.out, "line one\nline two");
}

#[test]
fn a_child_that_exits_without_reading_stdin_does_not_hang() {
    let start = Instant::now();
    let call = Call::secs(argv(&["/usr/bin/true"]), 20).stdin(vec![0; 4 * 1024 * 1024]);
    let r = ProcessRunner.run(&call);
    assert_eq!(r.code, 0);
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_full_stdout_pipe_does_not_block_the_stdin_write() {
    // cat echoes 4 MiB back while we are still writing it; the output is capped at 2 MiB
    let call = Call::secs(argv(&["/bin/cat"]), 20).stdin(vec![b'a'; 4 * 1024 * 1024]);
    let r = ProcessRunner.run(&call);
    assert_eq!(r.code, 0);
    assert!(r.out.starts_with(CUT_NOTICE));
    assert!(r.out.ends_with("aaaa"));
}

type Chunks = Arc<Mutex<Vec<(Instant, Channel, String)>>>;

fn collecting(call: Call) -> (Call, Chunks) {
    let chunks: Chunks = Arc::new(Mutex::new(Vec::new()));
    let sink = chunks.clone();
    let call = call.streaming(Some(Arc::new(move |c, t: &str| {
        lock(&sink).push((Instant::now(), c, t.to_string()));
    })));
    (call, chunks)
}

#[test]
fn output_streams_while_the_command_runs() {
    let (call, chunks) = collecting(sh("echo one; sleep 1; echo two", 10));
    let r = ProcessRunner.run(&call);
    let ended = Instant::now();
    assert_eq!(r.code, 0);
    assert_eq!(r.out, "one\ntwo\n");
    let seen = lock(&chunks).clone();
    let first = seen
        .iter()
        .find(|c| c.2.contains("one"))
        .expect("a chunk with one");
    assert!(ended.duration_since(first.0) >= Duration::from_millis(700));
    assert_eq!(
        seen.iter().map(|c| c.2.as_str()).collect::<String>(),
        "one\ntwo\n"
    );
}

#[test]
fn a_multi_byte_character_split_across_reads_is_not_broken() {
    // "é" is two bytes; the shell writes the first byte, waits, then the second
    let (call, chunks) = collecting(sh(r"printf '\303'; sleep 0.5; printf '\251'", 10));
    let r = ProcessRunner.run(&call);
    assert_eq!(r.out, "\u{e9}");
    assert_eq!(
        lock(&chunks)
            .iter()
            .map(|c| c.2.as_str())
            .collect::<String>(),
        "\u{e9}"
    );
}

#[test]
fn output_is_capped_at_two_mib_per_pipe() {
    let r = ProcessRunner.run(&sh(r#"head -c 3000000 /dev/zero | tr "\000" a"#, 20));
    assert!(r.out.len() <= OUTPUT_CAP + 32, "{}", r.out.len());
    assert!(r.out.starts_with(CUT_NOTICE));
    assert!(r.out.ends_with("aaaa"));
}

#[test]
fn the_two_pipes_are_tagged() {
    let (call, chunks) = collecting(sh("echo out; echo err >&2", 10));
    let r = ProcessRunner.run(&call);
    assert_eq!((r.out.as_str(), r.err.as_str()), ("out\n", "err\n"));
    let seen = lock(&chunks).clone();
    assert!(seen
        .iter()
        .any(|c| c.1 == Channel::Stdout && c.2.contains("out")));
    assert!(seen
        .iter()
        .any(|c| c.1 == Channel::Stderr && c.2.contains("err")));
}

#[test]
fn orphans_of_a_normal_exit_are_ended() {
    let dir = TempDir::new("run");
    let file = dir.join("pids");
    let r = ProcessRunner.run(&sh(
        &format!(
            "sleep 30 & echo \"child $!\" >> '{}'; exit 0",
            file.display()
        ),
        10,
    ));
    let seen = spawned(&file);
    assert_eq!(r.code, 0);
    assert_eq!(seen.children.len(), 1, "the sleep started");
    let left = still_running(&seen.all(), 5);
    seen.kill_all();
    assert!(
        left.is_empty(),
        "an orphan of a finished command is still running"
    );
}

#[test]
fn a_stdin_value_is_not_in_argv_environment_or_result() {
    let secret = "s3cr3t-value-xyz";
    let call = sh(r#"cat >/dev/null; env; echo "$0 $*""#, 5).stdin(secret.as_bytes().to_vec());
    let r = ProcessRunner.run(&call);
    assert_eq!(r.code, 0);
    assert!(!r.out.contains(secret));
    assert!(!r.err.contains(secret));
    assert!(
        !format!("{call:?}").contains(secret),
        "Call's Debug shows only the stdin's length"
    );
}

// MARK: the fakes

#[test]
fn the_scripted_runner_answers_the_first_matching_rule_and_records_every_call() {
    let runner = ScriptedRunner::new();
    runner.on(
        |a| a.iter().any(|x| x == "one"),
        CommandResult::new(1, "first", ""),
    );
    runner.on(|_| true, CommandResult::new(2, "second", ""));
    assert_eq!(runner.run(&Call::secs(argv(&["x", "one"]), 1)).code, 1);
    assert_eq!(runner.run(&Call::secs(argv(&["x", "two"]), 1)).code, 2);
    let none = ScriptedRunner::new();
    assert_eq!(
        none.run(&Call::secs(argv(&["x"]), 1)),
        CommandResult::ok("")
    );
    let calls = runner.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].argv, argv(&["x", "one"]));
}

#[test]
fn the_scripted_runner_streams_a_rules_chunks_and_records_stdin_and_env() {
    let runner = ScriptedRunner::new();
    runner.on_with(
        |_| true,
        CommandResult::ok(""),
        vec![(Channel::Stdout, "a".into()), (Channel::Stderr, "b".into())],
    );
    let (call, chunks) = collecting(
        Call::secs(argv(&["x"]), 1)
            .stdin(b"input".to_vec())
            .env("K", "v"),
    );
    runner.run(&call);
    let seen: Vec<(Channel, String)> = lock(&chunks).iter().map(|c| (c.1, c.2.clone())).collect();
    assert_eq!(
        seen,
        vec![(Channel::Stdout, "a".into()), (Channel::Stderr, "b".into())]
    );
    let recorded = &runner.calls()[0];
    assert_eq!(recorded.stdin_text(), "input");
    assert_eq!(recorded.env, vec![("K".to_string(), "v".to_string())]);
}

#[test]
fn the_refusing_runner_runs_nothing() {
    let r = Refusing("off").run(&Call::secs(argv(&["/usr/bin/ssh"]), 1));
    assert_eq!(r, CommandResult::refused("off"));
}
