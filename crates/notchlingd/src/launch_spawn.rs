use crate::identity::Launches;
use notchling_engine::tools::acp::{Killer, Output, Spawned, Spawner, ToolProcess as AcpProcess};
use notchling_engine::tools::codex::events::{parse_line, Event};
use notchling_engine::tools::codex::plan::TurnSpec;
use notchling_engine::tools::codex::process::{Next, SpawnError, Stream, ToolProcess, ToolSpawner};
use notchling_events::kinds::choices::AgentTool;
use serde_json::Value;
use std::io;
use std::sync::mpsc::channel;
use std::sync::Arc;

pub type StartOf = fn(u32) -> Option<u64>;

#[derive(Clone)]
struct Mark {
    launches: Launches,
    tool: AgentTool,
    pid: u32,
    started: u64,
}

impl Mark {
    fn of(
        launches: &Launches,
        tool: AgentTool,
        pid: Option<u32>,
        start_of: StartOf,
    ) -> Option<Mark> {
        let pid = pid?;
        Some(Mark {
            launches: launches.clone(),
            tool,
            pid,
            started: start_of(pid)?,
        })
    }

    fn register(&self, session: &str) {
        self.launches
            .register(self.pid, self.started, self.tool, session);
    }

    fn forget(&self) {
        self.launches.forget(self.pid, self.started);
    }
}

pub struct CodexLaunches {
    inner: Arc<dyn ToolSpawner>,
    launches: Launches,
    start_of: StartOf,
}

impl CodexLaunches {
    pub fn new(
        inner: Arc<dyn ToolSpawner>,
        launches: Launches,
        start_of: StartOf,
    ) -> CodexLaunches {
        CodexLaunches {
            inner,
            launches,
            start_of,
        }
    }
}

fn resumed(spec: &TurnSpec) -> Option<&str> {
    let args = &spec.args;
    (args.get(1).map(String::as_str) == Some("resume"))
        .then(|| args.len().checked_sub(2).and_then(|i| args.get(i)))
        .flatten()
        .map(String::as_str)
}

impl ToolSpawner for CodexLaunches {
    fn spawn(&self, spec: &TurnSpec) -> Result<Arc<dyn ToolProcess>, SpawnError> {
        let process = self.inner.spawn(spec)?;
        let Some(mark) = Mark::of(
            &self.launches,
            AgentTool::Codex,
            process.pid(),
            self.start_of,
        ) else {
            return Ok(process);
        };
        if let Some(session) = resumed(spec) {
            mark.register(session);
        }
        Ok(Arc::new(Marked { process, mark }))
    }
}

struct Marked {
    process: Arc<dyn ToolProcess>,
    mark: Mark,
}

impl ToolProcess for Marked {
    fn next(&self) -> Next {
        let next = self.process.next();
        match &next {
            Next::Line(line) if line.stream == Stream::Out => {
                if let Event::ThreadStarted { thread_id } = parse_line(&line.text) {
                    self.mark.register(&thread_id);
                }
            }
            Next::Exited(_) => self.mark.forget(),
            Next::Line(_) => {}
        }
        next
    }

    fn interrupt(&self) {
        self.process.interrupt();
    }

    fn terminate(&self) {
        self.process.terminate();
    }

    fn kill(&self) {
        self.process.kill();
    }

    fn pid(&self) -> Option<u32> {
        self.process.pid()
    }
}

impl Drop for Marked {
    fn drop(&mut self) {
        self.mark.forget();
    }
}

pub struct AcpLaunches {
    inner: Arc<dyn Spawner>,
    launches: Launches,
    start_of: StartOf,
}

impl AcpLaunches {
    pub fn new(inner: Arc<dyn Spawner>, launches: Launches, start_of: StartOf) -> AcpLaunches {
        AcpLaunches {
            inner,
            launches,
            start_of,
        }
    }
}

impl Spawner for AcpLaunches {
    fn spawn(&self, program: &str, args: &[String], folder: &str) -> Result<Spawned, String> {
        self.spawn_with_env(program, args, folder, &[])
    }

    fn spawn_with_env(
        &self,
        program: &str,
        args: &[String],
        folder: &str,
        env: &[(String, String)],
    ) -> Result<Spawned, String> {
        let spawned = self.inner.spawn_with_env(program, args, folder, env)?;
        let Some(mark) = Mark::of(
            &self.launches,
            AgentTool::Antigravity,
            spawned.process.pid(),
            self.start_of,
        ) else {
            return Ok(spawned);
        };
        let (tx, rx) = channel();
        let watching = mark.clone();
        let output = spawned.output;
        std::thread::spawn(move || {
            for out in output {
                match &out {
                    Output::Line(text) => {
                        if let Some(session) = answered_session(text) {
                            watching.register(&session);
                        }
                    }
                    Output::Closed(_) => watching.forget(),
                    Output::Oversize => {}
                }
                if tx.send(out).is_err() {
                    break;
                }
            }
            watching.forget();
        });
        Ok(Spawned {
            process: Box::new(Tracked {
                process: spawned.process,
                mark,
            }),
            output: rx,
        })
    }
}

fn answered_session(line: &str) -> Option<String> {
    let value: Value = serde_json::from_str(line).ok()?;
    value
        .pointer("/result/sessionId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

fn loaded_session(line: &str) -> Option<String> {
    let value: Value = serde_json::from_str(line).ok()?;
    (value.get("method").and_then(Value::as_str) == Some("session/load"))
        .then(|| value.pointer("/params/sessionId").and_then(Value::as_str))
        .flatten()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

struct Tracked {
    process: Box<dyn AcpProcess>,
    mark: Mark,
}

impl AcpProcess for Tracked {
    fn write_line(&mut self, line: &str) -> io::Result<()> {
        if let Some(session) = loaded_session(line) {
            self.mark.register(&session);
        }
        self.process.write_line(line)
    }

    fn killer(&self) -> Arc<dyn Killer> {
        self.process.killer()
    }

    fn pid(&self) -> Option<u32> {
        self.process.pid()
    }
}

#[cfg(test)]
#[path = "launch_spawn_tests.rs"]
mod tests;
