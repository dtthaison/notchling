use crate::agy_link::{DaemonLink, Link, LinkError};
use crate::layout::Layout;
use notchling_engine::tools::agy_sidecar::agentapi::{AgentApi, RealAgentApi};
use notchling_engine::tools::agy_sidecar::deliver::{self, Pause, Sent, Sleep, Started, Timing};
use notchling_engine::tools::agy_sidecar::jobs::{Job, Kind, Outcome};
use notchling_engine::tools::agy_sidecar::steps::{SqliteSteps, Steps};
use notchling_engine::tools::agy_sidecar::Roots;
use notchling_local_api::ops;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

pub const CONNECT_WITHIN: Duration = Duration::from_secs(2);
pub const TAKE_WAIT_MS: u64 = 20_000;
pub const TAKE_WITHIN: Duration = Duration::from_secs(30);
pub const REPORT_WITHIN: Duration = Duration::from_secs(10);
pub const RETRIES: u32 = 6;

pub struct Work<'a> {
    pub api: &'a dyn AgentApi,
    pub steps: &'a dyn Steps,
    pub pause: &'a dyn Pause,
    pub timing: Timing,
}

pub fn execute(job: &Job, work: &Work) -> Outcome {
    match &job.kind {
        Kind::Reply { session } => {
            match deliver::send(
                work.api,
                work.steps,
                work.pause,
                session,
                &job.text,
                work.timing,
            ) {
                Ok(Sent::Confirmed { .. }) => Outcome::Delivered {
                    session: Some(session.clone()),
                },
                Ok(Sent::Unconfirmed { .. }) => Outcome::Unconfirmed {
                    session: Some(session.clone()),
                },
                Err(fail) => Outcome::Failed(fail.to_string()),
            }
        }
        Kind::Start { model, title } => match deliver::start(
            work.api,
            work.steps,
            work.pause,
            &job.text,
            model.as_deref(),
            title.as_deref(),
            work.timing,
        ) {
            Ok(Started::Confirmed(id)) => Outcome::Delivered { session: Some(id) },
            Ok(Started::Unconfirmed(id)) => Outcome::Unconfirmed { session: Some(id) },
            Ok(Started::Unknown) => Outcome::Unconfirmed { session: None },
            Err(fail) => Outcome::Failed(fail.to_string()),
        },
    }
}

pub trait Connect {
    fn connect(&mut self) -> Result<Box<dyn Link>, LinkError>;
}

pub trait Rest {
    fn rest(&mut self, attempt: u32);
}

fn report(connect: &mut dyn Connect, rest: &mut dyn Rest, id: &str, outcome: &Outcome) -> bool {
    let mut args = outcome.to_json();
    args["id"] = json!(id);
    for attempt in 0..=RETRIES {
        if attempt > 0 {
            rest.rest(attempt);
        }
        let sent = connect
            .connect()
            .and_then(|mut link| link.call(ops::AGY_SIDECAR_REPORT, args.clone(), REPORT_WITHIN));
        match sent {
            Ok(_) => return true,
            Err(LinkError::Refused(_)) => return false,
            Err(LinkError::Gone) => {}
        }
    }
    false
}

pub fn serve_once(
    connect: &mut dyn Connect,
    rest: &mut dyn Rest,
    work: &Work,
) -> Result<usize, LinkError> {
    let mut link = connect.connect()?;
    let reply = link.call(
        ops::AGY_SIDECAR_TAKE,
        json!({"waitMs": TAKE_WAIT_MS, "max": 1}),
        TAKE_WITHIN,
    )?;
    drop(link);
    let jobs: Vec<Job> = reply
        .get("jobs")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Job::from_json).collect())
        .unwrap_or_default();
    for job in &jobs {
        let outcome = execute(job, work);
        report(connect, rest, &job.id, &outcome);
    }
    Ok(jobs.len())
}

pub fn serve(connect: &mut dyn Connect, rest: &mut dyn Rest, work: &Work, stop: &dyn Fn() -> bool) {
    let mut attempt = 0;
    while !stop() {
        match serve_once(connect, rest, work) {
            Ok(_) => attempt = 0,
            Err(_) => {
                if stop() {
                    return;
                }
                attempt += 1;
                if attempt > RETRIES {
                    return;
                }
                rest.rest(attempt);
            }
        }
    }
}

struct RealConnect {
    socket: PathBuf,
}

impl Connect for RealConnect {
    fn connect(&mut self) -> Result<Box<dyn Link>, LinkError> {
        DaemonLink::connect(&self.socket, CONNECT_WITHIN).map(|l| Box::new(l) as Box<dyn Link>)
    }
}

struct Backoff;

impl Rest for Backoff {
    fn rest(&mut self, attempt: u32) {
        std::thread::sleep(Duration::from_secs(u64::from(attempt.min(5))));
    }
}

pub fn run(layout: &Layout) -> i32 {
    let roots = Roots::under(&layout.home());
    let path_var = std::env::var("PATH").ok();
    let api = RealAgentApi::new(RealAgentApi::find(
        path_var.as_deref(),
        PathBuf::from("agentapi"),
    ));
    let steps = SqliteSteps::new(roots.conversations(), std::env::temp_dir());
    let work = Work {
        api: &api,
        steps: &steps,
        pause: &Sleep,
        timing: Timing::REAL,
    };
    let mut connect = RealConnect {
        socket: layout.socket.clone(),
    };
    serve(&mut connect, &mut Backoff, &work, &|| false);
    0
}

#[cfg(test)]
#[path = "agy_sidecar_run_tests.rs"]
mod tests;
