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
            Err(fail) => Outcome::Failed(fail.to_string()),
        },
    }
}

fn report(link: &mut dyn Link, id: &str, outcome: &Outcome) -> Result<(), LinkError> {
    let mut args = outcome.to_json();
    args["id"] = json!(id);
    link.call(ops::AGY_SIDECAR_REPORT, args, REPORT_WITHIN)
        .map(|_| ())
}

pub fn serve_once(link: &mut dyn Link, work: &Work) -> Result<usize, LinkError> {
    let reply = link.call(
        ops::AGY_SIDECAR_TAKE,
        json!({"waitMs": TAKE_WAIT_MS}),
        TAKE_WITHIN,
    )?;
    let jobs: Vec<Job> = reply
        .get("jobs")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Job::from_json).collect())
        .unwrap_or_default();
    for job in &jobs {
        let outcome = execute(job, work);
        report(link, &job.id, &outcome)?;
    }
    Ok(jobs.len())
}

pub trait Connect {
    fn connect(&mut self) -> Result<Box<dyn Link>, LinkError>;
}

pub trait Rest {
    fn rest(&mut self, attempt: u32);
}

pub fn serve(connect: &mut dyn Connect, rest: &mut dyn Rest, work: &Work, stop: &dyn Fn() -> bool) {
    let mut attempt = 0;
    while !stop() {
        if let Ok(mut link) = connect.connect() {
            attempt = 0;
            while !stop() {
                if serve_once(&mut *link, work).is_err() {
                    break;
                }
            }
        }
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
