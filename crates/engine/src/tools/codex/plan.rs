//! What a Codex turn runs: the argv, the folder, the environment and how the exit is judged
//! (`CodexRunner.arguments`, `run` and the termination handler). Pure: nothing here starts a process.

use std::path::{Path, PathBuf};

/// The sandbox a Chat turn runs in: exec can't ask for approval, so the session's folder is
/// writable and the rest of the disk isn't.
pub const WORKSPACE_WRITE: &str = "workspace-write";

/// The longest prompt one turn takes. It travels as one argument, and a single argument over
/// 128 KB is refused by Linux's `execve`.
pub const MAX_PROMPT_BYTES: usize = 100 * 1024;

/// The model and effort picked for a session's next turns; `None` keeps Codex's default
/// (`CodexPick`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pick {
    pub model: Option<String>,
    pub effort: Option<String>,
}

/// One turn: a new session in `folder` when `session` is `None`, else the next turn of `session`.
pub struct Turn<'a> {
    pub prompt: &'a str,
    pub session: Option<&'a str>,
    pub folder: &'a str,
    pub pick: &'a Pick,
    /// "Allow Network in Codex Turns" (the `codexNetwork` setting, on unless turned off).
    pub network: bool,
}

/// A process to start: the program, its arguments and folder, and the environment changes
/// (`Some` sets a variable, `None` removes it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: Vec<(String, Option<String>)>,
}

/// How a process ended. `signaled` is an uncaught signal (a stop on purpose).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub signaled: bool,
}

/// `codex exec` arguments. `network` only matters in the workspace-write sandbox (read-only has no
/// such table); Codex 0.166 reads it as `[sandbox_workspace_write] network_access`. The session id
/// and prompt stay last. A prompt that starts with `-` follows a `--`, so it is a prompt and not a
/// flag (the Swift app passed it bare).
pub fn arguments(
    prompt: &str,
    session: Option<&str>,
    folder: &str,
    pick: &Pick,
    sandbox: &str,
    network: bool,
) -> Vec<String> {
    let mut common: Vec<String> = vec![
        "--json".into(),
        "--skip-git-repo-check".into(),
        "-c".into(),
        format!("sandbox_mode=\"{sandbox}\""),
    ];
    if network && sandbox == WORKSPACE_WRITE {
        common.push("-c".into());
        common.push("sandbox_workspace_write.network_access=true".into());
    }
    // A picked model or effort holds from this turn on; Codex keeps its own default otherwise.
    if let Some(model) = &pick.model {
        common.push("-m".into());
        common.push(model.clone());
    }
    if let Some(effort) = &pick.effort {
        common.push("-c".into());
        common.push(format!("model_reasoning_effort=\"{effort}\""));
    }
    let mut args: Vec<String> = vec!["exec".into()];
    if session.is_some() {
        args.push("resume".into());
    }
    args.extend(common);
    if session.is_none() {
        args.push("-C".into());
        args.push(folder.into());
    }
    if prompt.starts_with('-') {
        args.push("--".into());
    }
    if let Some(session) = session {
        args.push(session.into());
    }
    args.push(prompt.into());
    args
}

/// The process for `turn`. `path` is the PATH the tools Codex runs are found in (an app opened from
/// Finder has only the system one); `NO_COLOR` is dropped, as for `claude`.
pub fn turn_spec(codex: &Path, turn: &Turn<'_>, path: Option<&str>) -> TurnSpec {
    let mut env = vec![("NO_COLOR".to_string(), None)];
    if let Some(path) = path {
        env.push(("PATH".to_string(), Some(path.to_string())));
    }
    TurnSpec {
        program: codex.to_path_buf(),
        args: arguments(
            turn.prompt,
            turn.session,
            turn.folder,
            turn.pick,
            WORKSPACE_WRITE,
            turn.network,
        ),
        cwd: turn.folder.to_string(),
        env,
    }
}

/// The message for a turn that did not end well, or `None` for one that did. Stopped on purpose
/// (SIGINT, or Codex's own exit code 130 for it) isn't a failure. `last_error` is what the turn
/// printed last that looked like an error.
pub fn failure_of(exit: &ExitInfo, last_error: &str) -> Option<String> {
    if exit.signaled || exit.code == Some(0) || exit.code == Some(130) {
        return None;
    }
    if !last_error.is_empty() {
        return Some(last_error.to_string());
    }
    Some(match exit.code {
        Some(code) => format!("Codex exited with code {code}."),
        None => "Codex exited without a status.".to_string(),
    })
}

/// A session id as Codex writes it (a UUID). A caller's id becomes an argument and is matched
/// against file names, so it is held to letters, digits and `-`, and never starts with `-`.
pub fn valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('-')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// A model slug (`gpt-6.1-sol`): letters, digits, `.`, `_`, `-`, starting with a letter or digit.
pub fn valid_model(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 64
        && model.starts_with(|c: char| c.is_ascii_alphanumeric())
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// An effort level (`low`, `xhigh`): it sits inside quotes in a `-c` value, so nothing else.
pub fn valid_effort(effort: &str) -> bool {
    !effort.is_empty()
        && effort.len() <= 24
        && effort
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

/// The folders tools Codex runs usually live in, added to a PATH that lacks them.
const TOOL_DIRS: [&str; 3] = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"];

/// `current` with the usual tool folders (and `~/.local/bin`) appended when absent.
pub fn augmented_path(current: &str, home: &Path) -> String {
    let local = home
        .join(".local")
        .join("bin")
        .to_string_lossy()
        .into_owned();
    let mut parts: Vec<String> = current
        .split(':')
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect();
    for extra in TOOL_DIRS.iter().map(|d| d.to_string()).chain([local]) {
        if !parts.contains(&extra) {
            parts.push(extra);
        }
    }
    parts.join(":")
}

/// The `codex` command: the usual install places (Swift's order), then the PATH's absolute
/// entries. The Swift app also asked a login shell; a daemon launches no shell for this.
pub fn find_binary(home: &Path, path: &str, is_file: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let usual = [
        home.join(".local").join("bin").join("codex"),
        PathBuf::from("/opt/homebrew/bin/codex"),
        PathBuf::from("/usr/local/bin/codex"),
        home.join(".npm-global").join("bin").join("codex"),
    ];
    usual
        .into_iter()
        .chain(
            path.split(':')
                .filter(|dir| Path::new(dir).is_absolute())
                .map(|dir| Path::new(dir).join("codex")),
        )
        .find(|candidate| is_file(candidate))
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
