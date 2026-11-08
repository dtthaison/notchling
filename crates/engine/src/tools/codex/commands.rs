//! Codex's slash commands (CLI 0.166, descriptions as its own picker shows them). A Chat turn is
//! `codex exec`, which has no slash commands, so Chat does a few itself and runs the rest in
//! Codex's own interface: the terminal sheet opens `codex resume <id>` and types the command there
//! (`CodexCommands`).

use crate::control::slash::SlashCommand;
use std::path::Path;

/// What Chat does with a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// Chat's own model and effort picker.
    ModelPicker,
    /// A new Codex session in the same folder.
    NewSession,
    /// Delete this session, after asking.
    Delete,
    /// Nothing to do: Chat isn't a Codex process to quit.
    Ignore,
    /// Typed into `codex resume` in the terminal sheet.
    Terminal,
}

/// Codex's own commands, in its order.
const COMMANDS: [(&str, &str); 41] = [
    ("model", "choose what model and reasoning effort to use"),
    ("review", "review my current changes and find issues"),
    ("new", "start a new chat during a conversation"),
    (
        "init",
        "create an AGENTS.md file with instructions for Codex",
    ),
    (
        "compact",
        "summarize conversation to prevent hitting the context limit",
    ),
    (
        "status",
        "show current session configuration and token usage",
    ),
    ("diff", "show git diff (including untracked files)"),
    ("mention", "mention a file"),
    ("permissions", "choose what Codex is allowed to do"),
    ("plan", "switch to Plan mode"),
    ("goal", "set or view the goal for a long-running task"),
    ("side", "start a side conversation in an ephemeral fork"),
    ("fork", "fork the current chat"),
    ("resume", "resume a saved chat"),
    ("rename", "rename the current thread"),
    ("copy", "copy the last response or part of it"),
    ("export", "export the conversation as markdown"),
    ("usage", "view account usage or use a usage limit reset"),
    (
        "mcp",
        "list MCP tools; use /mcp verbose or /mcp login <name>",
    ),
    ("apps", "manage apps"),
    ("plugins", "browse plugins"),
    (
        "skills",
        "use skills to improve how Codex performs specific tasks",
    ),
    ("prompts", "run or save reusable prompts and slash commands"),
    ("hooks", "view and manage lifecycle hooks"),
    ("memories", "configure memory use and generation"),
    ("experimental", "toggle experimental features"),
    ("settings", "configure Codex preferences"),
    (
        "worktree",
        "start or continue a conversation in a new worktree",
    ),
    ("app", "continue this session in the Desktop app"),
    (
        "import",
        "import setup, this project, and recent chats from Claude Code",
    ),
    ("cd", "change the current working directory"),
    ("pwd", "show the current working directory"),
    ("ps", "list background terminals"),
    ("subagents", "switch between this session's subagents"),
    ("warnings", "view retained warnings and diagnostic details"),
    ("rollout", "print the rollout file path"),
    ("feedback", "send logs to maintainers"),
    ("archive", "archive this session"),
    ("delete", "permanently delete this session"),
    ("clear", "clear the terminal and start a new chat"),
    ("logout", "log out of Codex"),
];

/// `quit`, the last of Codex's list.
const QUIT: (&str, &str) = ("quit", "exit Codex");

/// How many saved prompts are listed, and how much of each file is read for its description.
const MAX_PROMPTS: usize = 500;
const PROMPT_HEAD_BYTES: u64 = 64 * 1024;

fn command(name: &str, description: &str) -> SlashCommand {
    SlashCommand {
        name: name.to_string(),
        description: description.to_string(),
        argument_hint: None,
    }
}

/// Codex's own commands.
pub fn all() -> Vec<SlashCommand> {
    COMMANDS
        .iter()
        .chain([&QUIT])
        .map(|(name, description)| command(name, description))
        .collect()
}

/// Codex's commands plus the saved prompts in `<codex home>/prompts/<name>.md`, run as
/// `/prompts:<name>`.
pub fn available(codex_home: &Path) -> Vec<SlashCommand> {
    let dir = codex_home.join("prompts");
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|name| name.ends_with(".md"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files.truncate(MAX_PROMPTS);
    let mut out = all();
    for file in files {
        let name = file.trim_end_matches(".md");
        let text = read_head(&dir.join(&file));
        let description =
            prompt_description(&text).unwrap_or_else(|| "your saved prompt".to_string());
        out.push(command(&format!("prompts:{name}"), &description));
    }
    out
}

fn read_head(path: &Path) -> String {
    use std::io::Read;
    let mut bytes = Vec::new();
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file.take(PROMPT_HEAD_BYTES).read_to_end(&mut bytes);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// A prompt file's `description:` front matter, else its first line (cut to 80 characters).
pub fn prompt_description(text: &str) -> Option<String> {
    let lines: Vec<&str> = text
        .split([
            '\n', '\r', '\u{2028}', '\u{2029}', '\u{0b}', '\u{0c}', '\u{85}',
        ])
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.first() == Some(&"---") {
        let front = lines.iter().skip(1).take_while(|l| **l != "---");
        if let Some(line) = front.into_iter().find(|l| l.starts_with("description:")) {
            let value = &line["description:".len()..];
            return Some(
                value
                    .trim_matches(|c: char| matches!(c, ' ' | '"' | '\''))
                    .to_string(),
            );
        }
    }
    lines
        .iter()
        .find(|l| **l != "---")
        .map(|l| l.chars().take(80).collect())
}

/// The command name in `text` ("/review focus on auth" is "review"), if it's one of `among`.
pub fn command_in(text: &str, among: &[SlashCommand]) -> Option<String> {
    let rest = text.strip_prefix('/')?;
    let name = rest.split_whitespace().next().unwrap_or("");
    among
        .iter()
        .any(|c| c.name == name)
        .then(|| name.to_string())
}

pub fn route(name: &str) -> Route {
    match name {
        "model" => Route::ModelPicker,
        "new" | "clear" => Route::NewSession,
        "delete" => Route::Delete,
        "quit" | "exit" => Route::Ignore,
        _ => Route::Terminal,
    }
}

/// What the terminal sheet runs: Codex's interface on this session, or on a new one in the folder
/// when it hasn't started yet. `session` is a Codex session id (see [`super::plan::valid_session_id`]);
/// anything else makes this start a new session rather than carry the text into a shell.
pub fn script(session: &str) -> String {
    if session.is_empty() || !super::plan::valid_session_id(session) {
        "codex".to_string()
    } else {
        format!("codex resume {session}")
    }
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
