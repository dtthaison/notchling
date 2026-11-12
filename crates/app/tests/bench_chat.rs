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

/// A session that works costs the window no frame of its own: its spinner is Core Animation's
/// (the app draws nothing to turn it), and the Activity Line's clock is drawn once a second. Before,
/// the spinner drew the window ten times a second (100 in 10 s). `root` counts the window's draws,
/// which are the frames the app asks for, so this holds on a locked screen too, where the bench
/// draws frames by hand but only a window that was made dirty draws anything.
#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_idle_working_requests_few_frames() {
    let result = bench_for("idle", 10);
    let rings = count(&result, "native_rings");
    assert!(
        rings >= 1,
        "the working session's spinner isn't native: {result}"
    );
    let roots = count(&result, "root");
    assert!(
        (8..=16).contains(&roots),
        "the clock's once-a-second redraws, not a spinner's ten: {result}"
    );
    // A busy Mac adds a stray draw of every part now and then (see the bench doc).
    for part in ["conversation", "sidebar", "details"] {
        assert!(count(&result, part) <= 1, "{part}: {result}");
    }
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

/// Typing in the reply box redraws the window's outer parts (the box grows, the slash menu shows),
/// not the conversation, the Sidebar or the Details Panel: no part shows a draft.
#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_typing_draws_no_part() {
    let result = bench_for("typing", 3);
    assert_eq!(count(&result, "conversation"), 0, "{result}");
    assert_eq!(count(&result, "sidebar"), 0, "{result}");
    assert_eq!(count(&result, "details"), 0, "{result}");
}

/// A selection drag redraws the conversation (the wash follows the pointer), not the Sidebar or the
/// Details Panel.
#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_select_draws_no_sidebar_or_details() {
    let result = bench_for("select", 3);
    assert_eq!(count(&result, "sidebar"), 0, "{result}");
    assert_eq!(count(&result, "details"), 0, "{result}");
}

/// A selection drag draws the conversation (so the press reached it: a run whose press landed on
/// nothing draws none, which a count of 0 elsewhere would never show) and draws it only: the
/// selection belongs to the conversation's own view, and nothing else is drawn for it.
#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_select_draws_the_conversation_only() {
    let result = bench_for("select", 3);
    assert!(count(&result, "conversation") > 0, "{result}");
    assert_eq!(count(&result, "sidebar"), 0, "{result}");
    assert_eq!(count(&result, "details"), 0, "{result}");
}

/// The `bench-json:` line says whether the bench had to draw frames itself (a locked screen sends
/// the window no display-link frames), since a forced frame's ms/frame is not a display link's.
#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_says_whether_frames_were_forced() {
    let result = bench_for("idle", 2);
    assert!(result["forced"].is_boolean(), "{result}");
}

/// A reload that finds nothing changed in the open session redraws the Sidebar only when a row's
/// age, status or account changed (an age may tick over once in the run).
#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_reload_draws_no_sidebar() {
    let result = bench_for("reload", 3);
    assert!(count(&result, "sidebar") <= 1, "{result}");
}

/// Opening and closing the account menu is a `ChatView` change no part shows: the Sidebar's model
/// stays equal, so it isn't drawn.
#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_menu_draws_no_sidebar() {
    let result = bench_for("menu", 3);
    assert_eq!(count(&result, "sidebar"), 0, "{result}");
}

/// A reload that finds nothing changed in the open session draws no part but, once in a while, a
/// Sidebar row's age that ticked over.
#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_reload_draws_no_part() {
    let result = bench_for("reload", 3);
    assert_eq!(count(&result, "conversation"), 0, "{result}");
    assert!(count(&result, "sidebar") <= 1, "{result}");
    assert_eq!(count(&result, "details"), 0, "{result}");
}

/// The account menu opening and closing changes nothing the Details Panel shows.
#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_menu_draws_no_details() {
    let result = bench_for("menu", 3);
    assert_eq!(count(&result, "details"), 0, "{result}");
}

/// The account menu opening and closing changes nothing any part shows: no part is drawn, only the
/// window's outer parts.
#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_menu_draws_no_part() {
    let result = bench_for("menu", 3);
    assert_eq!(count(&result, "conversation"), 0, "{result}");
    assert_eq!(count(&result, "sidebar"), 0, "{result}");
    assert_eq!(count(&result, "details"), 0, "{result}");
    assert!(count(&result, "root") > 0, "{result}");
}

/// Typing still draws no part now that the conversation draws from its own input: the box and the
/// slash menu are the window's.
#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_typing_still_draws_no_part() {
    let result = bench_for("typing", 3);
    assert_eq!(count(&result, "conversation"), 0, "{result}");
    assert_eq!(count(&result, "sidebar"), 0, "{result}");
    assert_eq!(count(&result, "details"), 0, "{result}");
    assert!(count(&result, "root") > 0, "{result}");
}

/// Conversation rows built per conversation draw in a run (0 when it drew none).
fn rows_per_draw(result: &Value) -> f64 {
    count(result, "rows") as f64 / count(result, "conversation").max(1) as f64
}

/// The most rows a draw may build: the rows on screen with the list's overdraw (the viewport is
/// about ten messages tall here), against the 500 a draw built before the list.
const ON_SCREEN: f64 = 30.0;

/// Scrolling draws the conversation on every frame and builds only the rows on screen.
#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_scroll_draws_only_rows_on_screen() {
    let result = bench_for("scroll", 3);
    assert!(count(&result, "conversation") > 0, "{result}");
    assert!(rows_per_draw(&result) <= ON_SCREEN, "{result}");
}

/// The scroll bar after a second of scrolling holds for 0.7 s and fades over 0.3 s, then the window
/// is still: the conversation is drawn a few times (the fade's steps), each building only the rows
/// on screen, and not at all once the bar is gone. The numbers go in
/// `docs/perf/chat-state-split-bench.md`.
#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_scroll_fade() {
    let result = bench_for("scroll-fade", 2);
    assert!(rows_per_draw(&result) <= ON_SCREEN, "{result}");
    // 0.3 s at 30 steps a second is 9 steps, and the hold's own wake-up: nowhere near a draw a
    // frame for the two seconds (which would be 60 or more even at 30 a second).
    assert!(count(&result, "conversation") <= 20, "{result}");
}

/// Hovering builds only the rows on screen when it draws the conversation at all.
#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_hover_draws_only_rows_on_screen() {
    let result = bench_for("hover", 3);
    assert!(rows_per_draw(&result) <= ON_SCREEN, "{result}");
}

/// A selection drag builds only the rows on screen.
#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_select_draws_only_rows_on_screen() {
    let result = bench_for("select", 3);
    assert!(count(&result, "conversation") > 0, "{result}");
    assert!(rows_per_draw(&result) <= ON_SCREEN, "{result}");
}

/// Lines appended to the working session's transcript: each draw builds only the rows on screen,
/// the engine parses only the new bytes (a re-read of the whole file would be many times what was
/// appended), and the conversation, which was at its end, is still there.
#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_append_draws_only_rows_on_screen_and_follows() {
    let root = TempRoot::new(TURNS);
    let mut appended = 0usize;
    let mut n = 0;
    let result = run("idle", 4, &root, |root| {
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
        let text = format!("{line}\n");
        file.write_all(text.as_bytes()).expect("append a line");
        appended += text.len();
    });
    check("idle", &result);
    assert!(count(&result, "conversation") > 0, "{result}");
    assert!(rows_per_draw(&result) <= ON_SCREEN, "{result}");
    let parsed = count(&result, "parsed_bytes") as f64;
    assert!(parsed > 0.0, "{result}");
    assert!(
        parsed <= appended as f64 * 1.1,
        "{parsed} parsed, {appended} appended: {result}"
    );
    assert_eq!(
        result["at_end"], true,
        "the conversation left its end: {result}"
    );
}
