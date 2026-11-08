use super::*;
use std::path::PathBuf;

fn sh(script: &str) -> TurnSpec {
    TurnSpec {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".into(), script.into()],
        cwd: "/".into(),
        env: Vec::new(),
    }
}

/// Every line and the exit, to the end.
fn drain(process: &Arc<dyn ToolProcess>) -> (Vec<ToolLine>, ExitInfo) {
    let mut lines = Vec::new();
    loop {
        match process.next() {
            Next::Line(line) => lines.push(line),
            Next::Exited(info) => return (lines, info),
        }
    }
}

fn out(text: &str) -> ToolLine {
    ToolLine {
        stream: Stream::Out,
        text: text.into(),
    }
}

#[test]
fn it_reads_both_streams_and_the_exit_code() {
    let process = RealSpawner
        .spawn(&sh("echo one; echo two 1>&2; echo three; exit 3"))
        .expect("starts");
    let (lines, exit) = drain(&process);
    assert_eq!(exit.code, Some(3));
    assert!(!exit.signaled);
    let outs: Vec<_> = lines.iter().filter(|l| l.stream == Stream::Out).collect();
    assert_eq!(outs, [&out("one"), &out("three")]);
    assert!(lines
        .iter()
        .any(|l| l.stream == Stream::Err && l.text == "two"));
    // Asked again, it says the same.
    assert_eq!(process.next(), Next::Exited(exit));
}

#[test]
fn no_color_is_never_passed_and_the_environment_applies() {
    std::env::set_var("NO_COLOR", "1");
    let mut spec = sh("echo [${NO_COLOR-unset}][${NL_PROBE-unset}]");
    spec.env.push(("NL_PROBE".into(), Some("x".into())));
    let (lines, _) = drain(&RealSpawner.spawn(&spec).expect("starts"));
    assert_eq!(lines, [out("[unset][x]")]);
}

#[test]
fn it_runs_in_the_given_folder() {
    let (lines, _) = drain(&RealSpawner.spawn(&sh("pwd")).expect("starts"));
    assert_eq!(lines, [out("/")]);
}

#[test]
fn interrupt_is_sigint_and_a_stop_is_not_a_failure() {
    // `sh` dies of SIGINT while it waits.
    let process = RealSpawner
        .spawn(&sh("echo ready; sleep 30"))
        .expect("starts");
    assert_eq!(process.next(), Next::Line(out("ready")));
    process.interrupt();
    let (_, exit) = drain(&process);
    assert!(exit.signaled || exit.code == Some(130), "{exit:?}");
    assert_eq!(super::super::plan::failure_of(&exit, ""), None);
    // After the end an interrupt does nothing (and doesn't signal a reused pid).
    process.interrupt();
}

#[test]
fn a_line_over_the_cap_is_cut_not_buffered() {
    // 3 MB without a newline, then a normal line: the first is cut to the cap, the next is intact.
    let (lines, exit) = drain(
        &RealSpawner
            .spawn(&sh(
                "head -c 3000000 /dev/zero | tr '\\0' 'a'; echo; echo after",
            ))
            .expect("starts"),
    );
    assert_eq!(exit.code, Some(0));
    assert_eq!(lines.len(), 2, "{}", lines.len());
    assert_eq!(lines[0].text.len(), MAX_LINE_BYTES);
    assert_eq!(lines[1], out("after"));
}

#[test]
fn a_grandchild_holding_the_pipe_does_not_hold_the_turn_open() {
    let started = Instant::now();
    let (lines, exit) = drain(
        &RealSpawner
            .spawn(&sh("echo done; sleep 20 &"))
            .expect("starts"),
    );
    assert_eq!(exit.code, Some(0));
    assert_eq!(lines, [out("done")]);
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_program_that_is_not_there_is_a_spawn_error() {
    let spec = TurnSpec {
        program: PathBuf::from("/no/such/codex"),
        args: Vec::new(),
        cwd: "/".into(),
        env: Vec::new(),
    };
    assert!(RealSpawner.spawn(&spec).is_err());
    let mut spec = sh("true");
    spec.cwd = "/no/such/folder".into();
    assert!(RealSpawner.spawn(&spec).is_err());
}

#[test]
fn the_capped_reader_handles_the_edges() {
    let read = |bytes: &[u8], cap: usize| {
        let mut got = Vec::new();
        read_capped_lines(bytes, cap, |l| {
            got.push(l);
            true
        });
        got
    };
    assert_eq!(read(b"a\nb\n", 10), ["a", "b"]);
    assert_eq!(read(b"a\r\nb", 10), ["a", "b"], "no final newline, CRLF");
    assert_eq!(read(b"abcdef\nxy\n", 3), ["abc", "xy"]);
    assert_eq!(read(b"\n\n", 3), ["", ""]);
    assert!(read(b"", 3).is_empty());
    let mut count = 0;
    read_capped_lines(&b"a\nb\nc\n"[..], 10, |_| {
        count += 1;
        false
    });
    assert_eq!(count, 1, "stops when asked");
}
