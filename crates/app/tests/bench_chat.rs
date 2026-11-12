//! The Chat bench (`--render chat` with `NOTCHLING_RENDER_BENCH_MODE`) over a generated
//! 200-turn conversation: every mode must run and print its `bench-json:` line. Run on a Mac with
//! a display, in a release build (a debug GPUI is far too slow to judge):
//! `cargo test --release --locked -p notchling-app --test bench_chat -- --ignored --nocapture --test-threads=1`.
//! `NOTCHLING_BENCH_SECONDS` sets how long each mode runs (default 10).
//!
//! The only file writes in the Chat bench work are here: the generated data root under the temp
//! folder (removed when the test ends) and, in `bench_append`, the line appended to its open
//! transcript. Nothing reads or writes the real `~/.claude`.

#![cfg(target_os = "macos")]

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notchling_app::chat::bench_fixture::{files, WORKING_TRANSCRIPT};
use notchling_engine::bridge::net::files::{iso8601, now_ms};
use serde_json::{json, Value};

const TURNS: usize = 200;

/// A generated data root in the temp folder, removed on drop.
struct TempRoot(PathBuf);

impl TempRoot {
    fn new(turns: usize) -> TempRoot {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("notchling-bench-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (path, text) in files(turns, now_ms()) {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("make the folder");
            std::fs::write(&path, text).expect("write the file");
        }
        TempRoot(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Kills the render if the test fails before it ends.
struct Render(Child);

impl Drop for Render {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn seconds() -> u64 {
    std::env::var("NOTCHLING_BENCH_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10)
}

/// Runs the render in `mode` over `root`. `during` is called every 500 ms from `bench: ready`
/// until the render ends. Returns the `bench-json:` object.
fn run(mode: &str, seconds: u64, root: &TempRoot, mut during: impl FnMut(&Path)) -> Value {
    let out = root.path().join("bench.png");
    let child = Command::new(env!("CARGO_BIN_EXE_notchling-app"))
        .args(["--render", "chat", "--out"])
        .arg(&out)
        // Never the real ~/.claude: the generated root, named explicitly.
        .env("NOTCHLING_DATA_ROOT", root.path())
        .env("NOTCHLING_RENDER_BENCH", seconds.to_string())
        .env("NOTCHLING_RENDER_BENCH_MODE", mode)
        .stdout(Stdio::piped())
        .spawn()
        .expect("run notchling-app");
    let mut render = Render(child);
    let stdout = render.0.stdout.take().expect("piped stdout");
    let (lines, heard) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if lines.send(line).is_err() {
                break;
            }
        }
    });

    let deadline = Instant::now() + Duration::from_secs(seconds + 90);
    let mut seen = Vec::new();
    let mut ready = false;
    let mut json_line = None;
    loop {
        assert!(
            Instant::now() < deadline,
            "the render took too long: {seen:?}"
        );
        while let Ok(line) = heard.try_recv() {
            ready |= line == "bench: ready";
            if let Some(text) = line.strip_prefix("bench-json: ") {
                json_line = Some(text.to_owned());
            }
            seen.push(line);
        }
        let ended = render.0.try_wait().expect("wait").is_some();
        if ready && !ended {
            during(root.path());
        }
        if ended {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    // The last lines may still be on their way from the reader.
    std::thread::sleep(Duration::from_millis(200));
    while let Ok(line) = heard.try_recv() {
        if let Some(text) = line.strip_prefix("bench-json: ") {
            json_line = Some(text.to_owned());
        }
        seen.push(line);
    }
    let status = render.0.wait().expect("wait");
    assert!(status.success(), "exit {status:?}: {seen:?}");
    let text = json_line.unwrap_or_else(|| panic!("no bench-json line: {seen:?}"));
    println!("bench-json: {text}");
    serde_json::from_str(&text).expect("bench-json is JSON")
}

fn bench(mode: &str) -> Value {
    bench_for(mode, seconds())
}

fn bench_for(mode: &str, seconds: u64) -> Value {
    let root = TempRoot::new(TURNS);
    let result = run(mode, seconds, &root, |_| {});
    check(mode, &result);
    result
}

fn check(mode: &str, result: &Value) {
    assert_eq!(result["mode"], mode);
    assert!(result["frames"].as_u64().unwrap_or(0) > 0, "{result}");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_idle() {
    bench("idle");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_scroll() {
    bench("scroll");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_hover() {
    bench("hover");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_select() {
    bench("select");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_typing() {
    bench("typing");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_terminal() {
    bench("terminal");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_reload() {
    bench("reload");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_menu() {
    bench("menu");
}

/// The working session's transcript grows by one assistant line every 500 ms, as a session that
/// is answering does.
#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_append() {
    let root = TempRoot::new(TURNS);
    let mut n = 0;
    let result = run("idle", seconds(), &root, |root| {
        n += 1;
        let line = json!({
            "type": "assistant",
            "timestamp": iso8601(now_ms()),
            "effort": "high",
            "message": {
                "id": format!("bench-append-{n}"),
                "model": "claude-opus-5-5",
                "content": [{"type": "text", "text": format!("Appended line {n}: one more paragraph of the answer.")}],
                "usage": {"input_tokens": 1500, "output_tokens": 30},
            },
        });
        let mut file = OpenOptions::new()
            .append(true)
            .open(root.join(WORKING_TRANSCRIPT))
            .expect("open the working transcript");
        writeln!(file, "{line}").expect("append a line");
    });
    check("idle", &result);
    assert!(n > 0, "nothing was appended");
}

/// Counts of the parts' draws in a run, from its `bench-json:` line.
fn count(result: &Value, key: &str) -> u64 {
    result[key]
        .as_u64()
        .unwrap_or_else(|| panic!("no {key}: {result}"))
}

/// A terminal's output redraws the window's outer parts, not the conversation, the Sidebar or the
/// Details Panel.
#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_terminal_draws_no_part() {
    let result = bench_for("terminal", 3);
    assert_eq!(count(&result, "conversation"), 0, "{result}");
    assert_eq!(count(&result, "sidebar"), 0, "{result}");
    assert_eq!(count(&result, "details"), 0, "{result}");
    assert!(count(&result, "frames") >= 30, "{result}");
}
