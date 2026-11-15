use super::glance::has;
use super::model::BackgroundShell;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

const STOP_TOOLS: [&str; 2] = ["TaskStop", "KillShell"];

const STOP_KEYS: [&str; 3] = ["task_id", "shell_id", "bash_id"];

const STOPPED: &str = "stopped";

fn stopped_id(tool: &str, input: &Map<String, Value>) -> Option<String> {
    if !STOP_TOOLS.contains(&tool) {
        return None;
    }
    STOP_KEYS
        .iter()
        .find_map(|key| input.get(*key).and_then(Value::as_str))
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

#[derive(Default)]
pub struct Stops(HashMap<String, String>);

impl Stops {
    pub fn call(&mut self, tool_use_id: &str, tool: &str, input: &Map<String, Value>) {
        if tool_use_id.is_empty() {
            return;
        }
        if let Some(target) = stopped_id(tool, input) {
            self.0.insert(tool_use_id.to_string(), target);
        }
    }

    pub fn result(
        &mut self,
        tool_use_id: Option<&str>,
        is_error: bool,
        into: &mut HashMap<String, String>,
    ) {
        let Some(target) = tool_use_id.and_then(|id| self.0.remove(id)) else {
            return;
        };
        if !is_error {
            into.insert(target, STOPPED.to_string());
        }
    }

    pub fn waits_in(&self, line: &[u8]) -> bool {
        self.0.keys().any(|id| has(line, id.as_bytes()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub id: String,
    pub path: String,
}

pub fn launch(result: &str) -> Option<Launch> {
    let lead = "Command running in background with ID: ";
    let after_id = result.find(lead)? + lead.len();
    let id: String = result[after_id..]
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if id.is_empty() {
        return None;
    }
    let to = "Output is being written to: ";
    let rest = &result[result.find(to)? + to.len()..];
    let end = rest.find(".output")? + ".output".len();
    Some(Launch {
        id,
        path: rest[..end].to_string(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellState {
    Running,
    Ended(String),
}

pub fn state(
    shell: &BackgroundShell,
    finished: &HashMap<String, String>,
    writers: &dyn Fn(&str) -> Vec<i32>,
) -> ShellState {
    if let Some(status) = finished.get(&shell.id) {
        return ShellState::Ended(status.clone());
    }
    if writers(&shell.output_path).is_empty() {
        ShellState::Ended("ended".to_string())
    } else {
        ShellState::Running
    }
}

pub fn parse_writers(text: &str) -> Vec<i32> {
    let mut result = Vec::new();
    let mut pid: Option<i32> = None;
    let mut command = String::new();
    for line in text.split('\n') {
        let Some(tag) = line.chars().next() else {
            continue;
        };
        let value = &line[tag.len_utf8()..];
        match tag {
            'p' => {
                pid = value.parse().ok();
                command.clear();
            }
            'c' => command = value.to_string(),
            'a' => {
                let Some(pid) = pid else { continue };
                if (value == "w" || value == "u")
                    && !result.contains(&pid)
                    && !["claude", "node", "Notchling"].contains(&command.as_str())
                {
                    result.push(pid);
                }
            }
            _ => {}
        }
    }
    result
}

const NOT_WRITERS: [&str; 3] = ["claude", "node", "Notchling"];

pub fn lsof_args(paths: &[String]) -> Vec<String> {
    ["-w", "-F", "pcan", "--"]
        .into_iter()
        .map(str::to_owned)
        .chain(paths.iter().cloned())
        .collect()
}

pub fn parse_writers_by_name(
    text: &str,
    wanted: &[(String, Vec<String>)],
) -> HashMap<String, Vec<i32>> {
    let mut found: HashMap<String, Vec<i32>> = wanted
        .iter()
        .map(|(path, _)| (path.clone(), Vec::new()))
        .collect();
    let (mut pid, mut command, mut access) = (None, String::new(), String::new());
    for line in text.split('\n') {
        let Some(tag) = line.chars().next() else {
            continue;
        };
        let value = &line[tag.len_utf8()..];
        match tag {
            'p' => {
                pid = value.parse::<i32>().ok();
                command.clear();
                access.clear();
            }
            'c' => command = value.to_string(),
            'f' => access.clear(),
            'a' => access = value.to_string(),
            'n' => {
                let writing = access == "w" || access == "u";
                access.clear();
                let Some(pid) = pid else { continue };
                if !writing || NOT_WRITERS.contains(&command.as_str()) {
                    continue;
                }
                let path = match wanted {
                    [(only, _)] => Some(only.clone()),
                    _ => which(value, wanted),
                };
                if let Some(path) = path {
                    let pids = found.entry(path).or_default();
                    if !pids.contains(&pid) {
                        pids.push(pid);
                    }
                }
            }
            _ => {}
        }
    }
    found
}

fn which(name: &str, wanted: &[(String, Vec<String>)]) -> Option<String> {
    if let Some((path, _)) = wanted
        .iter()
        .find(|(path, names)| path == name || names.iter().any(|n| n == name))
    {
        return Some(path.clone());
    }
    let base = |p: &str| p.rsplit('/').next().map(str::to_owned);
    let mut same = wanted.iter().filter(|(path, _)| base(path) == base(name));
    match (same.next(), same.next()) {
        (Some((path, _)), None) => Some(path.clone()),
        _ => None,
    }
}

pub fn output_tail(path: &str, bytes: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let size = file.seek(SeekFrom::End(0)).ok()?;
    file.seek(SeekFrom::Start(size.saturating_sub(bytes)))
        .ok()?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).ok()?;
    Some(String::from_utf8_lossy(&data).into_owned())
}
