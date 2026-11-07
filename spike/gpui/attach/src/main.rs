//! S5: drive a scratch background Claude Code session through `claude attach` in a
//! pty, the way `ReplySender` does, and check the message reached the transcript.
//! Usage: `attach --scratch`. Never touches any other session.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use spike_core::transcript::{bracketed, tail_has_user_text, CLEAR_INPUT};

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("HOME"))
}

fn claude() -> PathBuf {
    let local = home().join(".local/bin/claude");
    if local.exists() { local } else { PathBuf::from("/usr/local/bin/claude") }
}

/// `ChatModel.jobID(inLaunchOutput:)`: the 8-hex id after the first " · ".
fn job_id(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let id = line.split(" · ").nth(1)?.split_whitespace().next()?;
        (id.len() == 8 && id.chars().all(|c| c.is_ascii_hexdigit())).then(|| id.to_string())
    })
}

/// `TranscriptReader.projectFolder(for:)`.
fn project_folder(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

fn state(id: &str) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(home().join(".claude/jobs").join(id).join("state.json")).ok()?;
    serde_json::from_str(&text).ok()
}

fn transcripts(s: &serde_json::Value) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(p) = s["linkScanPath"].as_str() {
        out.push(PathBuf::from(p));
    }
    if let (Some(cwd), Some(sid)) = (s["cwd"].as_str(), s["sessionId"].as_str()) {
        out.push(home().join(".claude/projects").join(project_folder(cwd)).join(format!("{sid}.jsonl")));
    }
    out
}

fn run(args: &[&str], cwd: &Path) -> (bool, String) {
    match Command::new(claude()).args(args).current_dir(cwd).output() {
        Ok(o) => (o.status.success(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))),
        Err(e) => (false, e.to_string()),
    }
}

/// Stops and removes the scratch job and its folder on every exit path.
struct Cleanup {
    dir: PathBuf,
    id: Option<String>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(id) = &self.id {
            let (_, out) = run(&["stop", id], &self.dir);
            eprintln!("claude stop {id}: {}", out.trim());
            let (_, out) = run(&["rm", id], &self.dir);
            eprintln!("claude rm {id}: {}", out.trim());
        }
        let _ = std::fs::remove_dir_all(&self.dir);
        eprintln!("removed {}", self.dir.display());
    }
}

fn scratch() -> Result<(), (&'static str, String)> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    // Under the spike folder: inside a folder Claude Code already trusts, so the
    // scratch session isn't refused as untrusted. Ignored by git (`/.scratch-*`).
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(format!(".scratch-{stamp:x}"));
    std::fs::create_dir_all(&dir).map_err(|e| ("unverified", e.to_string()))?;
    let dir = dir.canonicalize().map_err(|e| ("unverified", e.to_string()))?;
    let mut cleanup = Cleanup { dir: dir.clone(), id: None };

    let (_, out) = run(&["--bg", "--name", "spike-attach", "say ok"], &dir);
    eprintln!("claude --bg: {}", out.trim());
    if out.contains("not trusted") {
        return Err(("unverified", format!("scratch folder refused: {}", out.trim())));
    }
    let id = job_id(&out).ok_or(("unverified", format!("no job id in: {}", out.trim())))?;
    cleanup.id = Some(id.clone());

    let started = Instant::now();
    let mut last = String::new();
    let st = loop {
        if started.elapsed() > Duration::from_secs(120) {
            return Err(("unverified", format!("job never went idle (last state {last:?})")));
        }
        if let Some(s) = state(&id) {
            let now = s["state"].as_str().unwrap_or("").to_string();
            if now != last {
                eprintln!("state: {now}");
                last = now.clone();
            }
            if !now.is_empty() && now != "working" && now != "starting" && s["linkScanPath"].is_string() {
                break s;
            }
        }
        sleep(Duration::from_millis(500));
    };

    let pty = native_pty_system()
        .openpty(PtySize { rows: 40, cols: 120, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| ("no", format!("openpty: {e}")))?;
    let mut cmd = CommandBuilder::new(claude());
    cmd.args(["attach", &id]);
    cmd.cwd(&dir);
    cmd.env_remove("NO_COLOR");
    let mut child = pty.slave.spawn_command(cmd).map_err(|e| ("no", format!("spawn attach: {e}")))?;
    drop(pty.slave);
    // Drain the screen and never answer it (device-attribute queries included).
    let mut reader = pty.master.try_clone_reader().map_err(|e| ("no", e.to_string()))?;
    let screen = std::thread::spawn(move || {
        let mut all = Vec::new();
        let mut buf = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 { break; }
            all.extend_from_slice(&buf[..n]);
        }
        all
    });
    let mut writer = pty.master.take_writer().map_err(|e| ("no", e.to_string()))?;

    let needle = format!("spike ping {stamp:x}");
    sleep(Duration::from_millis(1200));
    for (i, chunk) in [CLEAR_INPUT.to_string(), bracketed(&needle), "\r".to_string()].iter().enumerate() {
        if i > 0 { sleep(Duration::from_millis(150)); }
        writer.write_all(chunk.as_bytes()).map_err(|e| ("no", format!("write: {e}")))?;
        writer.flush().ok();
    }
    sleep(Duration::from_millis(800));
    let _ = child.kill(); // detach: the session keeps running without its client
    let _ = child.wait();
    drop(writer);
    drop(pty.master);
    let bytes = screen.join().unwrap_or_default();
    eprintln!("attach screen: {} bytes", bytes.len());

    let paths = transcripts(&st);
    eprintln!("transcripts: {paths:?}");
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        let fresh = state(&id).map(|s| transcripts(&s)).unwrap_or_default();
        for p in paths.iter().chain(fresh.iter()) {
            if tail_has_user_text(p, &needle, 2_000_000).unwrap_or(false) {
                eprintln!("found {needle:?} in {}", p.display());
                return Ok(());
            }
        }
        sleep(Duration::from_millis(500));
    }
    Err(("no", format!("{needle:?} not in the transcript after 60 s")))
}

fn main() {
    if std::env::args().nth(1).as_deref() != Some("--scratch") {
        eprintln!("usage: attach --scratch");
        std::process::exit(2);
    }
    match scratch() {
        Ok(()) => println!("S5 yes"),
        Err((kind, why)) => println!("S5 {kind}: {why}"),
    }
}
