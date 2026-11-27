use super::SpawnRetrying;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const SESSION: &str = "52afae32-0000-4000-8000-000000000001";

#[derive(Clone, Debug)]
pub enum Step {
    Args(Vec<String>),
    Shell(String),
}

impl Step {
    pub fn args(args: &[&str]) -> Step {
        Step::Args(args.iter().map(|a| a.to_string()).collect())
    }

    fn to_json(&self) -> Value {
        match self {
            Step::Args(args) => json!({"args": args}),
            Step::Shell(line) => json!({"sh": line}),
        }
    }
}

pub struct Session<'a> {
    pub root: &'a Path,
    pub home: &'a Path,
    pub helper: &'a str,
    pub bin: &'a str,
    pub folder: Option<&'a Path>,
    pub job: bool,
    pub step_timeout: Duration,
}

pub fn run_helper() {
    let Ok(bin) = std::env::var("NL_VERIFIED_BIN") else {
        return;
    };
    let mut go = String::new();
    std::io::stdin().lock().read_line(&mut go).unwrap();
    let steps: Vec<Value> =
        serde_json::from_str(&std::env::var("NL_VERIFIED_STEPS").unwrap()).unwrap();
    let limit = Duration::from_millis(
        std::env::var("NL_VERIFIED_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60_000),
    );
    for step in steps {
        let mut command = if let Some(line) = step["sh"].as_str() {
            let mut c = Command::new("sh");
            c.args(["-c", line]);
            c
        } else {
            let mut c = Command::new(&bin);
            for arg in step["args"].as_array().unwrap() {
                c.arg(arg.as_str().unwrap());
            }
            c
        };
        let (code, out, err, timed_out) = run_limited(&mut command, limit);
        println!(
            "STEP {}",
            json!({"code": code, "out": out, "err": err, "timedOut": timed_out})
        );
    }
}

fn drain<R: Read + Send + 'static>(mut pipe: R) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

fn run_limited(command: &mut Command, limit: Duration) -> (Option<i32>, String, String, bool) {
    use std::os::unix::process::CommandExt;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn_retrying();
    let out = drain(child.stdout.take().unwrap());
    let err = drain(child.stderr.take().unwrap());
    let deadline = Instant::now() + limit;
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
            break child.wait().unwrap();
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    (
        status.code(),
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
        timed_out,
    )
}

pub fn plant_claude(root: &Path) -> std::path::PathBuf {
    let claude_dir = root.join(".local/share/claude/versions/1");
    std::fs::create_dir_all(&claude_dir).unwrap();
    let claude = claude_dir.join("claude");
    if !claude.exists() {
        std::fs::copy(std::env::current_exe().unwrap(), &claude).unwrap();
    }
    claude
}

impl Session<'_> {
    pub fn run(&self, steps: &[Step]) -> Vec<Value> {
        let claude = plant_claude(self.root);
        let mut command = Command::new(&claude);
        if let Some(folder) = self.folder {
            command.current_dir(folder);
        }
        let steps: Vec<Value> = steps.iter().map(Step::to_json).collect();
        let mut parent = command
            .args(["--exact", self.helper, "--nocapture", "--test-threads=1"])
            .env("NL_VERIFIED_BIN", self.bin)
            .env("NL_VERIFIED_STEPS", serde_json::to_string(&steps).unwrap())
            .env(
                "NL_VERIFIED_TIMEOUT_MS",
                self.step_timeout.as_millis().to_string(),
            )
            .env("NOTCHLING_DATA_ROOT", self.root)
            .env("HOME", self.home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn_retrying();
        let sessions = self.root.join("claude/sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let file = if self.job {
            json!({
                "sessionId": SESSION,
                "pid": parent.id(),
                "cwd": self.root.join("launch").to_string_lossy(),
                "jobId": "j1",
            })
        } else {
            json!({"sessionId": SESSION, "pid": parent.id()})
        };
        let file_path = sessions.join(format!("{}.json", parent.id()));
        std::fs::write(&file_path, file.to_string()).unwrap();
        writeln!(parent.stdin.take().unwrap(), "go").unwrap();
        let reader = BufReader::new(parent.stdout.take().unwrap());
        let lines: Vec<String> = reader.lines().map_while(Result::ok).collect();
        let _ = parent.wait();
        let _ = std::fs::remove_file(&file_path);
        lines
            .iter()
            .filter_map(|line| line.split_once("STEP ").map(|(_, text)| text.to_owned()))
            .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
            .collect()
    }
}
