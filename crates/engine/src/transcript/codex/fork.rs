use super::lineage::{first_meta, of_payload};
use super::stamped::{Stamp, Stamped};
use crate::transcript::util::{parse_object, str_of};
use serde_json::Value;
use std::fs::File;
use std::io::Read;
use std::path::Path;

const SCAN_BYTES: u64 = 3_000_000;

pub fn own_start(path: &str) -> u64 {
    static START: Stamped<u64> = Stamped::new();
    match Stamp::of(Path::new(path)) {
        Some(stamp) => START.get(path, stamp, || read_own_start(path)),
        None => 0,
    }
}

fn read_own_start(path: &str) -> u64 {
    let Some(first) = first_meta(path) else {
        return 0;
    };
    if of_payload(&first).is_none() {
        return 0;
    }
    let marker = first
        .get("subagent_history_start_ordinal")
        .and_then(Value::as_u64);
    let mut head = Vec::new();
    let Ok(file) = File::open(path) else {
        return 0;
    };
    if file.take(SCAN_BYTES).read_to_end(&mut head).is_err() {
        return 0;
    }
    let mut forked = false;
    let mut opened = false;
    let mut closed = false;
    let mut at = 0usize;
    let mut complete = 0usize;
    let mut index = 0usize;
    while let Some(rel) = head[at..].iter().position(|&b| b == b'\n') {
        let line = &head[at..at + rel];
        if index > 0 {
            if let Some(map) = parse_object(line) {
                let obj = Value::Object(map);
                let kind = str_of(&obj, "type");
                let event = obj
                    .get("payload")
                    .and_then(|p| str_of(p, "type"))
                    .filter(|_| kind == Some("event_msg"));
                if kind == Some("session_meta") {
                    forked = true;
                } else if forked {
                    let own = match marker {
                        Some(from) => obj
                            .get("ordinal")
                            .and_then(Value::as_u64)
                            .is_some_and(|n| n >= from),
                        None => event == Some("task_started") && closed,
                    };
                    if own {
                        return at as u64;
                    }
                    match event {
                        Some("task_started") => opened = true,
                        Some("task_complete" | "turn_aborted") => closed = opened,
                        _ => {}
                    }
                }
            }
        }
        at += rel + 1;
        complete = at;
        index += 1;
    }
    if forked {
        complete as u64
    } else {
        0
    }
}
