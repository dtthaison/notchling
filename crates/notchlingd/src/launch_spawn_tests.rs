use crate::identity::Launches;
use crate::launch_spawn::{resumed, AcpLaunches, CodexLaunches, Probe};
use notchling_engine::tools::acp::{Killer, Output, Spawned, Spawner, ToolProcess as AcpProcess};
use notchling_engine::tools::codex::plan::{ExitInfo, TurnSpec};
use notchling_engine::tools::codex::process::{
    Next, SpawnError, Stream, ToolLine, ToolProcess, ToolSpawner,
};
use notchling_events::kinds::choices::AgentTool;
use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

const STARTED: u64 = 5_000;

const NOW: i64 = 1_000_000;

fn start_of(pid: u32) -> Option<u64> {
    (pid == 700).then_some(STARTED)
}

fn probe() -> Probe {
    Probe {
        start_of,
        started_ms: |pid| (pid == 700).then_some(NOW - 5),
        now_ms: || NOW,
    }
}

fn spec(args: &[&str]) -> TurnSpec {
    TurnSpec {
        program: PathBuf::from("codex"),
        args: args.iter().map(|a| a.to_string()).collect(),
        cwd: "/work".to_owned(),
        env: Vec::new(),
    }
}

struct Scripted {
    pid: Option<u32>,
    lines: Mutex<VecDeque<Next>>,
}

impl ToolProcess for Scripted {
    fn next(&self) -> Next {
        self.lines
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Next::Exited(ExitInfo {
                code: Some(0),
                signaled: false,
            }))
    }

    fn interrupt(&self) {}

    fn pid(&self) -> Option<u32> {
        self.pid
    }
}

struct Spawns(Mutex<Option<Arc<dyn ToolProcess>>>);

impl ToolSpawner for Spawns {
    fn spawn(&self, _: &TurnSpec) -> Result<Arc<dyn ToolProcess>, SpawnError> {
        Ok(self.0.lock().unwrap().take().expect("one process"))
    }
}

fn out_line(text: &str) -> Next {
    Next::Line(ToolLine {
        stream: Stream::Out,
        text: text.to_owned(),
    })
}

fn codex(pid: Option<u32>, lines: Vec<Next>, launches: &Launches) -> CodexLaunches {
    let process: Arc<dyn ToolProcess> = Arc::new(Scripted {
        pid,
        lines: Mutex::new(lines.into()),
    });
    CodexLaunches::new(
        Arc::new(Spawns(Mutex::new(Some(process)))),
        launches.clone(),
        probe(),
    )
}

#[test]
fn a_new_codex_session_is_registered_when_its_thread_starts_and_forgotten_when_it_ends() {
    let launches = Launches::default();
    let spawner = codex(
        Some(700),
        vec![
            out_line(r#"{"type":"thread.started","thread_id":"thread-1"}"#),
            out_line(r#"{"type":"turn.started"}"#),
        ],
        &launches,
    );
    let process = spawner.spawn(&spec(&["exec", "--json", "hello"])).unwrap();
    assert_eq!(launches.find(700, Some(STARTED)), None);
    assert!(matches!(process.next(), Next::Line(_)));
    assert_eq!(
        launches.find(700, Some(STARTED)),
        Some((AgentTool::Codex, "thread-1".to_owned()))
    );
    assert!(matches!(process.next(), Next::Line(_)));
    assert!(matches!(process.next(), Next::Exited(_)));
    assert_eq!(launches.find(700, Some(STARTED)), None);
}

#[test]
fn a_resumed_codex_session_is_registered_at_once_and_forgotten_when_the_process_goes() {
    let launches = Launches::default();
    let spawner = codex(Some(700), Vec::new(), &launches);
    let process = spawner
        .spawn(&spec(&["exec", "resume", "--json", "thread-9", "go on"]))
        .unwrap();
    assert_eq!(
        launches.find(700, Some(STARTED)),
        Some((AgentTool::Codex, "thread-9".to_owned()))
    );
    drop(process);
    assert_eq!(launches.find(700, Some(STARTED)), None);
}

#[test]
fn a_process_with_no_pid_or_no_start_time_is_not_registered() {
    let launches = Launches::default();
    let no_pid = codex(
        None,
        vec![out_line(r#"{"type":"thread.started","thread_id":"t"}"#)],
        &launches,
    );
    let process = no_pid.spawn(&spec(&["exec", "resume", "x", "y"])).unwrap();
    process.next();
    assert_eq!(launches.find(700, Some(STARTED)), None);
    let no_start = codex(
        Some(701),
        vec![out_line(r#"{"type":"thread.started","thread_id":"t"}"#)],
        &launches,
    );
    let process = no_start.spawn(&spec(&["exec", "x"])).unwrap();
    process.next();
    assert_eq!(launches.find(701, Some(STARTED)), None);
}

struct Pipe {
    pid: Option<u32>,
    written: Arc<Mutex<Vec<String>>>,
    fail: bool,
}

struct NoKill;

impl Killer for NoKill {
    fn kill(&self) {}
}

impl AcpProcess for Pipe {
    fn write_line(&mut self, line: &str) -> io::Result<()> {
        if self.fail {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"));
        }
        self.written.lock().unwrap().push(line.to_owned());
        Ok(())
    }

    fn killer(&self) -> Arc<dyn Killer> {
        Arc::new(NoKill)
    }

    fn pid(&self) -> Option<u32> {
        self.pid
    }
}

struct AcpSpawns {
    fail: bool,
    pid: Option<u32>,
    output: Mutex<Option<Receiver<Output>>>,
    written: Arc<Mutex<Vec<String>>>,
}

impl Spawner for AcpSpawns {
    fn spawn(&self, _: &str, _: &[String], _: &str) -> Result<Spawned, String> {
        Ok(Spawned {
            process: Box::new(Pipe {
                pid: self.pid,
                written: self.written.clone(),
                fail: self.fail,
            }),
            output: self.output.lock().unwrap().take().expect("one process"),
        })
    }
}

fn acp(
    pid: Option<u32>,
    launches: &Launches,
) -> (AcpLaunches, Sender<Output>, Arc<Mutex<Vec<String>>>) {
    let (tx, rx) = channel();
    let written = Arc::new(Mutex::new(Vec::new()));
    let inner = AcpSpawns {
        fail: false,
        pid,
        output: Mutex::new(Some(rx)),
        written: written.clone(),
    };
    (
        AcpLaunches::new(Arc::new(inner), launches.clone(), probe()),
        tx,
        written,
    )
}

#[test]
fn a_new_antigravity_session_is_registered_from_its_answer_and_forgotten_when_it_closes() {
    let launches = Launches::default();
    let (spawner, tx, _) = acp(Some(700), &launches);
    let spawned = spawner.spawn("agy", &["acp".to_owned()], "/work").unwrap();
    tx.send(Output::Line(
        r#"{"id":1,"result":{"protocolVersion":1}}"#.into(),
    ))
    .unwrap();
    assert!(matches!(spawned.output.recv().unwrap(), Output::Line(_)));
    assert_eq!(launches.find(700, Some(STARTED)), None);
    tx.send(Output::Line(
        r#"{"id":2,"result":{"sessionId":"agy-1"}}"#.into(),
    ))
    .unwrap();
    assert!(matches!(spawned.output.recv().unwrap(), Output::Line(_)));
    assert_eq!(
        launches.find(700, Some(STARTED)),
        Some((AgentTool::Antigravity, "agy-1".to_owned()))
    );
    tx.send(Output::Closed(None)).unwrap();
    assert!(matches!(spawned.output.recv().unwrap(), Output::Closed(_)));
    assert_eq!(launches.find(700, Some(STARTED)), None);
}

#[test]
fn a_loaded_antigravity_session_is_registered_when_it_is_asked_for_and_the_line_still_goes_out() {
    let launches = Launches::default();
    let (spawner, _tx, written) = acp(Some(700), &launches);
    let mut spawned = spawner.spawn("agy", &["acp".to_owned()], "/work").unwrap();
    let load = r#"{"jsonrpc":"2.0","id":3,"method":"session/load","params":{"sessionId":"agy-7","cwd":"/work"}}"#;
    spawned.process.write_line(load).unwrap();
    assert_eq!(
        launches.find(700, Some(STARTED)),
        Some((AgentTool::Antigravity, "agy-7".to_owned()))
    );
    assert_eq!(written.lock().unwrap().as_slice(), [load]);
}

#[test]
fn an_antigravity_process_with_no_pid_passes_straight_through() {
    let launches = Launches::default();
    let (spawner, tx, _) = acp(None, &launches);
    let spawned = spawner.spawn("agy", &["acp".to_owned()], "/work").unwrap();
    tx.send(Output::Line(
        r#"{"id":2,"result":{"sessionId":"agy-1"}}"#.into(),
    ))
    .unwrap();
    assert!(matches!(spawned.output.recv().unwrap(), Output::Line(_)));
    assert_eq!(launches.find(700, Some(STARTED)), None);
}

#[test]
fn the_session_of_a_resume_is_found_among_the_options_and_never_guessed() {
    let session = |args: &[&str]| resumed(&spec(args)).map(str::to_owned);
    assert_eq!(
        session(&["exec", "resume", "--json", "thread-9", "go on"]).as_deref(),
        Some("thread-9")
    );
    assert_eq!(
        session(&[
            "exec",
            "resume",
            "--json",
            "--skip-git-repo-check",
            "-c",
            "sandbox_mode=\"read-only\"",
            "-m",
            "gpt-x",
            "-c",
            "model_reasoning_effort=\"high\"",
            "--",
            "thread-9",
            "--json",
        ])
        .as_deref(),
        Some("thread-9")
    );
    assert_eq!(session(&["exec", "resume", "--json", "thread-9"]), None);
    assert_eq!(session(&["exec", "--json", "hello"]), None);
    assert_eq!(session(&["resume", "a", "b"]), None);
    assert_eq!(session(&["exec", "resume", "--json"]), None);
}

#[test]
fn a_pid_that_started_after_the_spawn_returned_is_not_ours() {
    let launches = Launches::default();
    let reused = Probe {
        started_ms: |_| Some(NOW + 5),
        ..probe()
    };
    let process: Arc<dyn ToolProcess> = Arc::new(Scripted {
        pid: Some(700),
        lines: Mutex::new(VecDeque::new()),
    });
    let spawner = CodexLaunches::new(
        Arc::new(Spawns(Mutex::new(Some(process)))),
        launches.clone(),
        reused,
    );
    spawner
        .spawn(&spec(&["exec", "resume", "--json", "thread-9", "go on"]))
        .unwrap();
    assert_eq!(launches.find(700, Some(STARTED)), None);
}

#[test]
fn an_antigravity_process_that_is_dropped_forgets_its_session_even_while_its_output_is_open() {
    let launches = Launches::default();
    let (spawner, tx, _) = acp(Some(700), &launches);
    let Spawned {
        mut process,
        output,
    } = spawner.spawn("agy", &["acp".to_owned()], "/work").unwrap();
    let load = r#"{"id":3,"method":"session/load","params":{"sessionId":"agy-7"}}"#;
    process.write_line(load).unwrap();
    assert!(launches.find(700, Some(STARTED)).is_some());
    drop(process);
    assert_eq!(launches.find(700, Some(STARTED)), None);
    drop((tx, output));
}

#[test]
fn a_load_that_could_not_be_written_registers_nothing() {
    let launches = Launches::default();
    let (tx, rx) = channel();
    let inner = AcpSpawns {
        fail: true,
        pid: Some(700),
        output: Mutex::new(Some(rx)),
        written: Arc::new(Mutex::new(Vec::new())),
    };
    let spawner = AcpLaunches::new(Arc::new(inner), launches.clone(), probe());
    let mut spawned = spawner.spawn("agy", &["acp".to_owned()], "/work").unwrap();
    let load = r#"{"id":3,"method":"session/load","params":{"sessionId":"agy-7"}}"#;
    assert!(spawned.process.write_line(load).is_err());
    assert_eq!(launches.find(700, Some(STARTED)), None);
    drop(tx);
}
