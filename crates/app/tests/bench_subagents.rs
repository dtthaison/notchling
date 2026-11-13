#![cfg(target_os = "macos")]

use std::fs::{File, FileTimes};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use notchling_app::chat::bench_fixture::{
    files, subagent_files, subagent_id, SUBAGENTS_FOLDER, WORKING_TRANSCRIPT,
};
use notchling_engine::bridge::net::files::now_ms;
use serde_json::Value;

const TURNS: usize = 200;
const SUBAGENTS: usize = 40;
const RUNNING: [usize; 2] = [12, 33];
const BYTES: usize = 3_000_000;

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> TempRoot {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("notchling-bench-sub-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let write = |path: &str, text: &str| {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("make the folder");
            std::fs::write(&path, text).expect("write the file");
        };
        for (path, text) in files(TURNS, now_ms()) {
            write(&path, &text);
        }
        for (path, text) in subagent_files(SUBAGENTS, BYTES) {
            write(&path, &text);
        }
        let old = SystemTime::now() - Duration::from_secs(7200);
        for n in (0..SUBAGENTS).filter(|n| !RUNNING.contains(n)) {
            let file = dir
                .join(SUBAGENTS_FOLDER)
                .join(format!("agent-{}.jsonl", subagent_id(n)));
            File::options()
                .write(true)
                .open(file)
                .and_then(|f| f.set_times(FileTimes::new().set_modified(old)))
                .expect("age the file");
        }
        assert!(dir.join(WORKING_TRANSCRIPT).is_file());
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

fn run(mode: &str, seconds: u64, root: &TempRoot) -> Value {
    let out = root.path().join("bench.png");
    let child = Command::new(env!("CARGO_BIN_EXE_notchling-app"))
        .args(["--render", "chat", "--out"])
        .arg(&out)
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
    let mut json_line = None;
    loop {
        assert!(
            Instant::now() < deadline,
            "the render took too long: {seen:?}"
        );
        while let Ok(line) = heard.try_recv() {
            if let Some(text) = line.strip_prefix("bench-json: ") {
                json_line = Some(text.to_owned());
            }
            seen.push(line);
        }
        if render.0.try_wait().expect("wait").is_some() {
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

fn count(result: &Value, key: &str) -> u64 {
    result[key].as_u64().unwrap_or(0)
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_reload_over_forty_subagents_reads_no_subagent_file_in_a_draw() {
    let root = TempRoot::new();
    let result = run("reload", seconds(), &root);
    assert!(count(&result, "frames") > 0, "{result}");
    assert!(
        count(&result, "parsed_bytes") <= 1_000_000,
        "a reload re-read subagent files: {result}"
    );
}

#[test]
#[ignore = "opens a window; a Mac with a display"]
fn bench_idle_over_forty_subagents_reads_no_subagent_file() {
    let root = TempRoot::new();
    let result = run("idle", seconds(), &root);
    assert!(count(&result, "frames") > 0, "{result}");
    assert!(
        count(&result, "parsed_bytes") <= 1_000_000,
        "an idle window re-read subagent files: {result}"
    );
}
