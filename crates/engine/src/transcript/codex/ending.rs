use super::fork::own_start;
use super::stamped::{Stamp, Stamped};
use crate::transcript::util::{parse_object, str_of};
use serde_json::Value;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const TAIL_BYTES: u64 = 262_144;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    Unknown,
    Running,
    Completed,
    Failed,
    Stopped,
}

pub fn ending(path: &str) -> Ending {
    static KEPT: Stamped<Ending> = Stamped::new();
    match Stamp::of(Path::new(path)) {
        Some(stamp) => KEPT.get(path, stamp, || read_ending(path)),
        None => Ending::Unknown,
    }
}

fn read_ending(path: &str) -> Ending {
    let Ok(mut file) = File::open(path) else {
        return Ending::Unknown;
    };
    let Ok(size) = file.seek(SeekFrom::End(0)) else {
        return Ending::Unknown;
    };
    let floor = own_start(path);
    let from = size.saturating_sub(TAIL_BYTES).max(floor);
    if from >= size || file.seek(SeekFrom::Start(from)).is_err() {
        return Ending::Unknown;
    }
    let mut tail = Vec::new();
    if file.read_to_end(&mut tail).is_err() {
        return Ending::Unknown;
    }
    let mut lines = tail.split(|&b| b == b'\n');
    if from > floor {
        lines.next();
    }
    let mut state = Ending::Unknown;
    for line in lines {
        let has = |word: &str| {
            let needle = format!("\"type\":\"{word}\"");
            line.windows(needle.len()).any(|w| w == needle.as_bytes())
        };
        if has("task_started") {
            state = Ending::Running;
        } else if has("task_complete") {
            state = if complete_failed(line) {
                Ending::Failed
            } else {
                Ending::Completed
            };
        } else if has("turn_aborted") {
            state = Ending::Stopped;
        } else if has("error") && is_event(line, "error") {
            state = Ending::Failed;
        }
    }
    state
}

fn is_event(line: &[u8], word: &str) -> bool {
    parse_object(line).is_some_and(|map| {
        let obj = Value::Object(map);
        str_of(&obj, "type") == Some("event_msg")
            && obj
                .get("payload")
                .and_then(|p| str_of(p, "type"))
                .is_some_and(|t| t == word)
    })
}

fn complete_failed(line: &[u8]) -> bool {
    parse_object(line)
        .and_then(|map| {
            Value::Object(map)
                .get("payload")
                .and_then(|p| p.get("error"))
                .map(|e| !e.is_null())
        })
        .unwrap_or(false)
}
