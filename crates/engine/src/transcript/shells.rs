//! Background shells: Bash calls Claude Code ran with `run_in_background`. Its tool result names an id
//! and the file the output goes to; a `<task-notification>` with the same id says how it ended
//! (`Conversation::finished_agents`). Starting and stopping processes is not part of the reader.

use super::model::BackgroundShell;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub id: String,
    pub path: String,
}

/// "Command running in background with ID: <id>. Output is being written to: <path>. ..."
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
    // The path ends at ".output", before the sentence's full stop.
    let end = rest.find(".output")? + ".output".len();
    Some(Launch {
        id,
        path: rest[..end].to_string(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellState {
    Running,
    /// The notification's status (completed, failed, killed...), or "ended" when the shell is gone
    /// without one (the session stopped, or the notification hasn't been written yet).
    Ended(String),
}

/// Running while some process still has the output file open for writing. `writers` lists those
/// processes for a path (see `parse_writers`); a notification in `finished` wins over it.
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

/// Processes with the file open for writing, from `lsof -F pca` output: `p<pid>`, `c<command>`, then
/// per file `f<fd>` and `a<mode>` (r, w or u). Claude Code itself never is one, but it is excluded by
/// name anyway, so Stop can't end the session.
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

/// The last `bytes` of a file, for the output panel's tail.
pub fn output_tail(path: &str, bytes: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let size = file.seek(SeekFrom::End(0)).ok()?;
    file.seek(SeekFrom::Start(size.saturating_sub(bytes)))
        .ok()?;
    let mut data = Vec::new();
    file.read_to_end(&mut data).ok()?;
    Some(String::from_utf8_lossy(&data).into_owned())
}
