#![cfg(target_os = "macos")]

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notchling_app::chat::bench_fixture::{files_shaped, Shape, WORKING_TRANSCRIPT};
use notchling_engine::bridge::net::files::{iso8601, now_ms};
use serde_json::{json, Value};

const TURNS: usize = 200;

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(shape: Shape) -> TempRoot {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("notchling-scroll-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (path, text) in files_shaped(TURNS, now_ms(), shape) {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("make the folder");
            std::fs::write(&path, text).expect("write the file");
        }
        TempRoot(dir)
    }

    fn real(real: &Path) -> TempRoot {
        let root = TempRoot::new(Shape::Plain);
        std::fs::copy(real, root.path().join(WORKING_TRANSCRIPT)).expect("copy the transcript");
        root
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

#[derive(Default)]
struct Options<'a> {
    env: Vec<(&'a str, &'a str)>,
    append: bool,
}

fn run(mode: &str, seconds: u64, root: &TempRoot, options: Options) -> Value {
    let out = root.path().join("bench.png");
    let mut command = Command::new(env!("CARGO_BIN_EXE_notchling-app"));
    command
        .args(["--render", "chat", "--out"])
        .arg(&out)
        .env("NOTCHLING_DATA_ROOT", root.path())
        .env("NOTCHLING_RENDER_BENCH", seconds.to_string())
        .env("NOTCHLING_RENDER_BENCH_MODE", mode)
        .stdout(Stdio::piped());
    if std::env::var_os("NOTCHLING_BENCH_FORCE_FRAMES").is_none() {
        command.env("NOTCHLING_BENCH_FORCE_FRAMES", "1");
    }
    for (key, value) in &options.env {
        command.env(key, value);
    }
    let mut render = Render(command.spawn().expect("run notchling-app"));
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
    let (mut seen, mut ready, mut json_line, mut n) = (Vec::new(), false, None, 0);
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
        if ready && !ended && options.append {
            n += 1;
            append(root.path(), n);
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

fn append(root: &Path, n: u32) {
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
}

fn scroll(mode: &str, seconds: u64, shape: Shape, options: Options) -> (Value, Value) {
    scroll_in(mode, seconds, &TempRoot::new(shape), options)
}

fn scroll_in(mode: &str, seconds: u64, root: &TempRoot, options: Options) -> (Value, Value) {
    let result = run(mode, seconds, root, options);
    assert_eq!(result["mode"], mode, "{result}");
    assert!(result["frames"].as_u64().unwrap_or(0) > 0, "{result}");
    let report = result["scroll"].clone();
    assert!(report.is_object(), "no scroll report: {result}");
    (report, result)
}

fn count(report: &Value, key: &str) -> i64 {
    report[key]
        .as_i64()
        .unwrap_or_else(|| panic!("no {key}: {report}"))
}

fn assert_smooth(report: &Value, result: &Value) {
    for key in ["stalls", "jumps", "reversals", "lost", "drifts"] {
        assert_eq!(count(report, key), 0, "{key}: {result}");
    }
    let max = report["max_jump"].as_f64().unwrap_or(f64::MAX);
    assert!(max <= 2.0, "max jump {max}: {result}");
}

const ON_SCREEN: f64 = 30.0;

fn rows_per_draw(result: &Value) -> f64 {
    result["rows"].as_f64().unwrap_or(0.0) / result["conversation"].as_f64().unwrap_or(1.0).max(1.0)
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn scrolling_up_from_the_end_moves_from_the_first_step() {
    let (report, result) = scroll("scroll-start", 4, Shape::Plain, Options::default());
    assert!(
        count(&report, "steps") >= 150,
        "the wheel didn't turn: {result}"
    );
    assert_smooth(&report, &result);
    assert_eq!(
        result["at_end"], true,
        "didn't come back to the end: {result}"
    );
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn a_slow_scroll_through_unmeasured_rows_moves_by_the_wheel() {
    let (report, result) = scroll("scroll-slow", 6, Shape::Plain, Options::default());
    assert!(
        count(&report, "steps") >= 300,
        "the wheel didn't turn: {result}"
    );
    assert_smooth(&report, &result);
    assert!(rows_per_draw(&result) <= ON_SCREEN, "{result}");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn scrolling_up_while_lines_are_appended_stays_put() {
    let options = Options {
        append: true,
        ..Options::default()
    };
    let (report, result) = scroll("scroll-slow", 6, Shape::Plain, options);
    assert!(
        count(&report, "steps") >= 300,
        "the wheel didn't turn: {result}"
    );
    assert_smooth(&report, &result);
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn scrolling_through_rows_taller_than_the_window_moves_by_the_wheel() {
    let options = Options {
        env: vec![("NOTCHLING_RENDER_EXPAND", "1")],
        ..Options::default()
    };
    let (report, result) = scroll("scroll-slow", 6, Shape::Tall, options);
    assert!(
        count(&report, "steps") >= 300,
        "the wheel didn't turn: {result}"
    );
    assert_smooth(&report, &result);
    assert!(rows_per_draw(&result) <= ON_SCREEN, "{result}");
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn scrolling_through_open_step_groups_moves_by_the_wheel() {
    let options = Options {
        env: vec![("NOTCHLING_RENDER_EXPAND", "1")],
        ..Options::default()
    };
    let (report, result) = scroll("scroll-slow", 6, Shape::Busy, options);
    assert!(
        count(&report, "steps") >= 300,
        "the wheel didn't turn: {result}"
    );
    assert_smooth(&report, &result);
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn a_momentum_scroll_moves_by_the_wheel() {
    let (report, result) = scroll("scroll-momentum", 8, Shape::Plain, Options::default());
    assert!(
        count(&report, "steps") >= 300,
        "the wheel didn't turn: {result}"
    );
    assert_smooth(&report, &result);
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn a_fling_moves_by_the_wheel() {
    let (report, result) = scroll("scroll-fling", 4, Shape::Plain, Options::default());
    assert!(
        count(&report, "steps") >= 30,
        "the wheel didn't turn: {result}"
    );
    assert_smooth(&report, &result);
}

#[test]
#[ignore = "opens a window; a Mac with a display; needs NOTCHLING_BENCH_REAL"]
fn a_real_transcript_scrolls_by_the_wheel() {
    let Some(path) = std::env::var_os("NOTCHLING_BENCH_REAL") else {
        println!("skipped: NOTCHLING_BENCH_REAL names no transcript");
        return;
    };
    let root = TempRoot::real(Path::new(&path));
    for mode in ["scroll-slow", "scroll-momentum"] {
        let (report, result) = scroll_in(mode, 6, &root, Options::default());
        assert_smooth(&report, &result);
    }
    let options = Options {
        append: true,
        ..Options::default()
    };
    let (report, result) = scroll_in("scroll-slow", 6, &root, options);
    assert_smooth(&report, &result);
}
