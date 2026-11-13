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

fn run(mode: &str, seconds: u64, root: &TempRoot, during: impl FnMut(&Path)) -> Value {
    run_with(mode, seconds, root, &[], during)
}

fn run_with(
    mode: &str,
    seconds: u64,
    root: &TempRoot,
    envs: &[(&str, &str)],
    mut during: impl FnMut(&Path),
) -> Value {
    let out = root.path().join("bench.png");
    let child = Command::new(env!("CARGO_BIN_EXE_notchling-app"))
        .args(["--render", "chat", "--out"])
        .arg(&out)
        .env("NOTCHLING_DATA_ROOT", root.path())
        .env("NOTCHLING_RENDER_BENCH", seconds.to_string())
        .env("NOTCHLING_RENDER_BENCH_MODE", mode)
        .envs(envs.iter().copied())
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

fn bench_with(mode: &str, seconds: u64, envs: &[(&str, &str)]) -> Value {
    let root = TempRoot::new(TURNS);
    let result = run_with(mode, seconds, &root, envs, |_| {});
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

fn count(result: &Value, key: &str) -> u64 {
    result[key]
        .as_u64()
        .unwrap_or_else(|| panic!("no {key}: {result}"))
}

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
        (5..=16).contains(&roots),
        "the clock's once-a-second redraws, not a spinner's ten: {result}"
    );
    for part in ["conversation", "sidebar", "details"] {
        assert!(count(&result, part) <= 2, "{part}: {result}");
    }
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_terminal_draws_no_part() {
    let result = bench_for("terminal", 3);
    assert_eq!(count(&result, "conversation"), 0, "{result}");
    assert_eq!(count(&result, "sidebar"), 0, "{result}");
    assert_eq!(count(&result, "details"), 0, "{result}");
    assert!(count(&result, "frames") >= 30, "{result}");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_typing_draws_no_part() {
    let result = bench_for("typing", 3);
    assert_eq!(count(&result, "conversation"), 0, "{result}");
    assert_eq!(count(&result, "sidebar"), 0, "{result}");
    assert_eq!(count(&result, "details"), 0, "{result}");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_select_draws_no_sidebar_or_details() {
    let result = bench_for("select", 3);
    assert_eq!(count(&result, "sidebar"), 0, "{result}");
    assert_eq!(count(&result, "details"), 0, "{result}");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_select_draws_the_conversation_only() {
    let result = bench_for("select", 3);
    assert!(count(&result, "conversation") > 0, "{result}");
    assert_eq!(count(&result, "sidebar"), 0, "{result}");
    assert_eq!(count(&result, "details"), 0, "{result}");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_says_whether_frames_were_forced() {
    let result = bench_for("idle", 2);
    assert!(result["forced"].is_boolean(), "{result}");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_reload_draws_no_sidebar() {
    let result = bench_for("reload", 3);
    assert!(count(&result, "sidebar") <= 1, "{result}");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_menu_draws_no_sidebar() {
    let result = bench_for("menu", 3);
    assert_eq!(count(&result, "sidebar"), 0, "{result}");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_reload_draws_no_part() {
    let result = bench_for("reload", 3);
    assert_eq!(count(&result, "conversation"), 0, "{result}");
    assert!(count(&result, "sidebar") <= 1, "{result}");
    assert_eq!(count(&result, "details"), 0, "{result}");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_menu_draws_no_details() {
    let result = bench_for("menu", 3);
    assert_eq!(count(&result, "details"), 0, "{result}");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_menu_draws_no_part() {
    let result = bench_for("menu", 3);
    assert_eq!(count(&result, "conversation"), 0, "{result}");
    assert_eq!(count(&result, "sidebar"), 0, "{result}");
    assert_eq!(count(&result, "details"), 0, "{result}");
    assert!(count(&result, "root") > 0, "{result}");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_typing_still_draws_no_part() {
    let result = bench_for("typing", 3);
    assert_eq!(count(&result, "conversation"), 0, "{result}");
    assert_eq!(count(&result, "sidebar"), 0, "{result}");
    assert_eq!(count(&result, "details"), 0, "{result}");
    assert!(count(&result, "root") > 0, "{result}");
}

fn rows_per_draw(result: &Value) -> f64 {
    count(result, "rows") as f64 / count(result, "conversation").max(1) as f64
}

const ON_SCREEN: f64 = 30.0;

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_scroll_draws_only_rows_on_screen() {
    let result = bench_for("scroll", 3);
    assert!(count(&result, "conversation") > 0, "{result}");
    assert!(rows_per_draw(&result) <= ON_SCREEN, "{result}");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_scroll_fade() {
    let result = bench_for("scroll-fade", 2);
    assert!(rows_per_draw(&result) <= ON_SCREEN, "{result}");
    assert!(count(&result, "conversation") <= 20, "{result}");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_hover_draws_only_rows_on_screen() {
    let result = bench_for("hover", 3);
    assert!(rows_per_draw(&result) <= ON_SCREEN, "{result}");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_select_draws_only_rows_on_screen() {
    let result = bench_for("select", 3);
    assert!(count(&result, "conversation") > 0, "{result}");
    assert!(rows_per_draw(&result) <= ON_SCREEN, "{result}");
}

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

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_render_map_draws_no_part_and_changes_no_count() {
    for mode in ["typing", "menu"] {
        let off = bench_with(mode, 3, &[]);
        let on = bench_with(mode, 3, &[("NOTCHLING_RENDER_PARTS", "1")]);
        for key in ["sidebar", "details", "conversation"] {
            assert_eq!(count(&on, key), 0, "{mode} {key} with the map on: {on}");
        }
        assert_eq!(
            count(&on, "conversation"),
            count(&off, "conversation"),
            "{mode}: {on} against {off}"
        );
        assert!(count(&on, "root") > 0, "{mode}: {on}");
    }
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_render_map_scroll_draws_only_the_conversation() {
    let result = bench_with("scroll", 3, &[("NOTCHLING_RENDER_PARTS", "1")]);
    assert!(count(&result, "conversation") > 0, "{result}");
    assert_eq!(count(&result, "sidebar"), 0, "{result}");
    assert_eq!(count(&result, "details"), 0, "{result}");
    assert!(rows_per_draw(&result) <= ON_SCREEN, "{result}");
}
