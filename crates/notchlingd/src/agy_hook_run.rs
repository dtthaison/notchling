use crate::agy_link::{DaemonLink, Link};
use crate::layout::Layout;
use notchling_engine::tools::agy_sidecar::{
    ASK_WAIT_SECS, INVOCATION_TIMEOUT_SECS, PRE_INVOCATION, PRE_TOOL, TOOL_TIMEOUT_SECS,
};
use notchling_local_api::ops;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::sync::mpsc::channel;
use std::time::Duration;

pub const READ_WITHIN: Duration = Duration::from_secs(1);
const MAX_INPUT: u64 = 4 * 1024 * 1024;

pub fn known(event: &str) -> bool {
    event == PRE_TOOL || event == PRE_INVOCATION
}

pub fn wait_for(event: &str) -> Duration {
    if event == PRE_TOOL {
        Duration::from_secs(ASK_WAIT_SECS)
    } else {
        Duration::from_secs(1)
    }
}

pub fn connect_within(event: &str) -> Duration {
    if event == PRE_TOOL {
        Duration::from_secs(2)
    } else {
        Duration::from_secs(1)
    }
}

pub fn call_within(event: &str) -> Duration {
    if event == PRE_TOOL {
        wait_for(event) + Duration::from_secs(5)
    } else {
        Duration::from_secs(2)
    }
}

pub fn worst_case(event: &str) -> Duration {
    READ_WITHIN + connect_within(event) + call_within(event)
}

pub fn allowed(event: &str) -> Duration {
    Duration::from_secs(if event == PRE_TOOL {
        TOOL_TIMEOUT_SECS
    } else {
        INVOCATION_TIMEOUT_SECS
    })
}

pub fn decide(link: &mut dyn Link, event: &str, input: &[u8]) -> Option<Value> {
    if !known(event) {
        return None;
    }
    let text = std::str::from_utf8(input).ok()?;
    let wait = wait_for(event);
    let args = json!({
        "event": event,
        "input": text,
        "waitMs": wait.as_millis() as u64,
    });
    let reply = link.call(ops::AGY_HOOK, args, call_within(event)).ok()?;
    reply.get("output").filter(|o| o.is_object()).cloned()
}

fn read_input(within: Duration) -> Option<Vec<u8>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut input = Vec::new();
        let read = std::io::stdin().take(MAX_INPUT).read_to_end(&mut input);
        let _ = tx.send(read.ok().map(|_| input));
    });
    rx.recv_timeout(within).ok().flatten()
}

pub fn run(layout: &Layout, event: &str) -> i32 {
    if !known(event) {
        return 0;
    }
    let Some(input) = read_input(READ_WITHIN) else {
        return 0;
    };
    let Ok(mut link) = DaemonLink::connect(&layout.socket, connect_within(event)) else {
        return 0;
    };
    if let Some(out) = decide(&mut link, event, &input) {
        if let Ok(text) = serde_json::to_vec(&out) {
            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(&text);
            let _ = stdout.flush();
        }
    }
    0
}

#[cfg(test)]
#[path = "agy_hook_run_tests.rs"]
mod tests;
