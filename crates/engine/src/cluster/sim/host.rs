use serde_json::{json, Value};

use crate::cluster::id::NodeId;
use crate::cluster::members::Term;
use crate::cluster::wire::WireError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Started {
    pub node: NodeId,
    pub job: String,
    pub term: Term,
    pub at_ms: u64,
}

#[derive(Clone, Debug, Default)]
pub struct SimHost {
    pub started: Vec<Started>,
    pub double_start: bool,
}

pub const START_OPS: [&str; 4] = ["session.new", "codex.send", "agy.open", "shell.run"];

impl SimHost {
    pub fn run(
        &mut self,
        node: &NodeId,
        op: &str,
        args: &Value,
        term: &Term,
        now_ms: u64,
    ) -> Result<Value, WireError> {
        if START_OPS.contains(&op) {
            let Some(job) = args.get("job").and_then(|j| j.as_str()) else {
                return Err(WireError::new("bad_args", "no job"));
            };
            let times = if self.double_start { 2 } else { 1 };
            for _ in 0..times {
                self.started.push(Started {
                    node: node.clone(),
                    job: job.to_string(),
                    term: term.clone(),
                    at_ms: now_ms,
                });
            }
            return Ok(json!({"session": format!("{op}:{job}")}));
        }
        Ok(json!({}))
    }
}
