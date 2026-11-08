//! The command line of `notchling delegate` (Swift `Delegate.Tool`, `Options`, `parse`): which tool,
//! its model and effort, the folder it works in, where generated media is copied, and the limits.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// The other tool a job is handed to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Codex,
    Agy,
}

impl Tool {
    /// `codex`, `agy` or `antigravity`, in any case.
    pub fn from_name(name: &str) -> Option<Tool> {
        match name.to_lowercase().as_str() {
            "codex" => Some(Tool::Codex),
            "agy" | "antigravity" => Some(Tool::Agy),
            _ => None,
        }
    }

    /// The word on the command line and in `--json`.
    pub fn raw(self) -> &'static str {
        match self {
            Tool::Codex => "codex",
            Tool::Agy => "agy",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Tool::Codex => "Codex",
            Tool::Agy => "Antigravity CLI",
        }
    }
}

/// One job.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub tool: Tool,
    pub prompt: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The folder the tool works in (its workspace).
    pub folder: String,
    /// Where generated images and videos are copied; `None` leaves them where the tool saved them.
    pub out: Option<String>,
    /// Lets the tool change files in `folder` (Codex workspace-write, agy accept-edits). Off, it
    /// only reads.
    pub write: bool,
    /// Continue this session instead of starting one.
    pub session: Option<String>,
    pub timeout: Duration,
}

/// The default timeout, in seconds.
pub const DEFAULT_TIMEOUT_SECS: u64 = 1800;

/// The longest timeout taken (ten years): a larger number is a typo, and `Duration` has limits.
const MAX_TIMEOUT_SECS: f64 = 315_360_000.0;

/// Parsed options and whether `--json` was given.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub options: Options,
    pub json: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

fn fail<T>(message: impl Into<String>) -> Result<T, ParseError> {
    Err(ParseError {
        message: message.into(),
    })
}

pub const USAGE: &str = "Usage: notchling delegate <codex|agy> [--model <m>] [--effort <e>] [--folder <dir>] [--out <dir>]\n                         [--write] [--session <id>] [--timeout <seconds>] [--json] -- <prompt>\n       notchling delegate-models [codex|agy]";

/// Nested delegations stop here, so two agents can't keep handing a job back and forth.
pub const MAX_DEPTH: u32 = 2;
pub const DEPTH_VARIABLE: &str = "NOTCHLING_DELEGATE_DEPTH";

/// The depth this run is at, from the variable's value: a count that is not a number is 0. `Err`
/// is the refusal's text once the limit is reached; `Ok` is the depth to give the tool's own
/// processes (this one plus one).
pub fn enter_depth(raw: Option<&str>) -> Result<u32, String> {
    let depth = raw.and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0);
    if depth >= MAX_DEPTH {
        return Err(format!(
            "This job was itself delegated {depth} times; do it yourself instead of delegating again."
        ));
    }
    Ok(depth + 1)
}

/// `args` are what follows `delegate`. `cwd` is the folder relative paths are read against and
/// `home` what `~` stands for.
pub fn parse(args: &[String], cwd: &Path, home: Option<&Path>) -> Result<Parsed, ParseError> {
    let Some(tool) = args.first().and_then(|a| Tool::from_name(a)) else {
        return fail(format!("Name the tool first: codex or agy.\n{USAGE}"));
    };
    let mut options = Options {
        tool,
        prompt: String::new(),
        model: None,
        effort: None,
        folder: absolute("", cwd, home),
        out: None,
        write: false,
        session: None,
        timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
    };
    let mut json = false;
    let mut words: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--" => {
                words.extend(args[i + 1..].iter().cloned());
                break;
            }
            "--write" => options.write = true,
            "--json" => json = true,
            "--model" | "--effort" | "--folder" | "--out" | "--session" | "--timeout"
            | "--prompt-file" => {
                let Some(v) = args.get(i + 1).filter(|v| !v.starts_with("--")) else {
                    return fail(format!("{a} needs a value."));
                };
                i += 1;
                match a {
                    "--model" => options.model = Some(v.clone()),
                    "--effort" => options.effort = Some(v.to_lowercase()),
                    "--folder" => options.folder = absolute(v, cwd, home),
                    "--out" => options.out = Some(absolute(v, cwd, home)),
                    "--session" => options.session = Some(v.clone()),
                    "--timeout" => match v.trim().parse::<f64>() {
                        Ok(t) if t.is_finite() && t > 0.0 => {
                            options.timeout = Duration::from_secs_f64(t.min(MAX_TIMEOUT_SECS));
                        }
                        _ => return fail("--timeout takes seconds."),
                    },
                    _ => match std::fs::read_to_string(absolute(v, cwd, home)) {
                        Ok(text) => words.push(text),
                        Err(_) => return fail(format!("Couldn't read {v}.")),
                    },
                }
            }
            _ if a.starts_with("--") => return fail(format!("Unknown option {a}.\n{USAGE}")),
            _ => words.push(a.to_string()),
        }
        i += 1;
    }
    options.prompt = words.join(" ").trim().to_string();
    if options.prompt.is_empty() {
        return fail(format!("No prompt given.\n{USAGE}"));
    }
    Ok(Parsed { options, json })
}

/// `path` as an absolute, tidied path: `~` is home, a relative path is read against `cwd`, and `.`
/// and `..` are folded away (no symlink is looked at). An empty `path` is `cwd` itself.
pub fn absolute(path: &str, cwd: &Path, home: Option<&Path>) -> String {
    let expanded: PathBuf = match (path, home) {
        ("~", Some(h)) => h.to_path_buf(),
        (p, Some(h)) if p.starts_with("~/") => h.join(&p[2..]),
        (p, _) => PathBuf::from(p),
    };
    let joined = if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    };
    let mut out = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out.to_string_lossy().into_owned()
}

#[cfg(test)]
#[path = "options_tests.rs"]
mod tests;
