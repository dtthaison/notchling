//! Run on a code block: which fenced blocks are a shell command, and what Run sends for one.
//! Pure. Claude Code's `!` mode takes one line, so a block's commands are joined into one: a
//! `\` continuation collapses, independent lines join with ` && `, and anything that needs more
//! than a line (a heredoc, a loop, a quote spanning lines, a script) is not offered.
//!
//! Detection is conservative: Run is a click that types into the session, so a block that only
//! might be a command (prose, Rust, JSON, a diff) gets no button.

use super::data::Tool;

/// The most commands (after joining continuations) a block may hold and still be one line's work.
const MAX_LINES: usize = 10;
/// The most an untagged block may hold: a longer one is more likely output or notes.
const MAX_UNTAGGED_LINES: usize = 4;

/// First words an untagged block's lines may start with, besides a path to a script.
const COMMAND_WORDS: &[&str] = &[
    "git",
    "cargo",
    "npm",
    "npx",
    "pnpm",
    "yarn",
    "swift",
    "make",
    "ls",
    "cd",
    "cat",
    "grep",
    "rg",
    "open",
    "brew",
    "python3",
    "node",
    "claude",
    "codex",
    "notchling",
    "gh",
    "docker",
    "kubectl",
    "ssh",
    "scp",
    "rsync",
    "curl",
    "sudo",
    "bash",
    "sh",
    "zsh",
];

/// A shell run as a line's first word names a script: it needs a path after it, or the line is as
/// likely prose ("sh is not bash") as a command.
const SHELLS: &[&str] = &["bash", "sh", "zsh"];

/// Command words that are also English ("make sure", "open the"): a line with these needs an
/// argument and no long run of plain words after it.
const AMBIGUOUS: &[&str] = &["make", "open", "cat", "node", "claude", "codex"];

/// Words that open or close a shell construct spanning lines.
const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac",
    "function", "select", "{", "}", "(", ")",
];

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// sh, bash, zsh, shell: every line is a command unless prompts say which are.
    Shell,
    /// console, terminal: prompts mark the commands, the rest is output.
    Console,
    /// No language, or plain text.
    Untagged,
}

fn kind(language: &str) -> Option<Kind> {
    let word = language
        .split(|c: char| c.is_whitespace() || c == '{' || c == ',')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match word.as_str() {
        "sh" | "bash" | "zsh" | "shell" => Some(Kind::Shell),
        "console" | "terminal" | "shellsession" | "sh-session" => Some(Kind::Console),
        "" | "text" | "txt" | "code" | "plain" | "plaintext" => Some(Kind::Untagged),
        _ => None,
    }
}

/// `line` without its `! ` or `$ ` prompt, when it has one.
fn prompted(line: &str) -> Option<&str> {
    let line = line.trim_start();
    line.strip_prefix("! ")
        .or_else(|| line.strip_prefix("$ "))
        .map(str::trim)
}

/// The one line to send for a code block of `language` with `text`, without the `!`; `None` when
/// the block is not a shell command.
pub fn command(language: &str, text: &str) -> Option<String> {
    let kind = kind(language)?;
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    if lines
        .iter()
        .find(|l| !l.trim().is_empty())
        .is_some_and(|l| l.starts_with("#!"))
    {
        return None;
    }
    let any_prompt = lines.iter().any(|l| prompted(l).is_some());
    // Without prompts, a console block is bare text like an untagged one.
    let strict = kind == Kind::Untagged || (kind == Kind::Console && !any_prompt);

    let mut logical: Vec<String> = Vec::new();
    let mut continuing = false;
    for raw in lines {
        let line = raw.trim();
        if line.is_empty() {
            continuing = false;
            continue;
        }
        let keep = if continuing {
            line
        } else if let Some(stripped) = prompted(raw) {
            stripped
        } else if any_prompt {
            // Output between the prompted commands: skipped in a shell block, a mix in a bare one.
            if kind == Kind::Untagged {
                return None;
            }
            continue;
        } else if line.starts_with('#') && !strict {
            continue;
        } else {
            line
        };
        let more = keep.ends_with('\\');
        let body = keep.strip_suffix('\\').map_or(keep, str::trim_end);
        if continuing {
            if let Some(last) = logical.last_mut() {
                last.push(' ');
                last.push_str(body.trim_start());
            }
        } else {
            logical.push(body.to_owned());
        }
        continuing = more;
    }
    if logical.is_empty() || logical.iter().any(String::is_empty) {
        return None;
    }
    let limit = if strict && !any_prompt {
        MAX_UNTAGGED_LINES
    } else {
        MAX_LINES
    };
    if logical.len() > limit {
        return None;
    }
    for (i, line) in logical.iter().enumerate() {
        if needs_more_than_a_line(line) || opens_a_quote(line) {
            return None;
        }
        if strict && !any_prompt && !(i > 0 && joins_before(line)) && !looks_like_command(line) {
            return None;
        }
    }
    let mut out = logical[0].clone();
    for pair in logical.windows(2) {
        let joined = ends_with_operator(&pair[0]) || joins_before(&pair[1]);
        out.push_str(if joined { " " } else { " && " });
        out.push_str(&pair[1]);
    }
    Some(out)
}

/// The text Run sends: a leading `!` puts Claude Code in shell mode.
pub fn message(command: &str) -> String {
    format!("! {command}")
}

/// Whether a code block offers Run: only where the reply box takes a `!` message (Claude Code,
/// not Codex or Antigravity, which have no shell mode), the session takes replies (`can_reply`:
/// acting on, not read-only or exited) and it is not a subagent's read-only transcript.
pub fn available(tool: Tool, can_reply: bool, in_subagent: bool) -> bool {
    tool == Tool::Claude && can_reply && !in_subagent
}

/// A line that begins or ends a construct that spans lines (`if`, `for`, a function, a heredoc).
fn needs_more_than_a_line(line: &str) -> bool {
    let first = line.split_whitespace().next().unwrap_or("");
    KEYWORDS.contains(&first)
        || line.ends_with('{')
        || line.ends_with('(')
        || line.contains("<<")
        || line.ends_with("; do")
        || line.ends_with("; then")
}

/// Whether the line ends inside an open quote, so its command goes on to the next line.
fn opens_a_quote(line: &str) -> bool {
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (None, '\\') | (Some('"'), '\\') => {
                chars.next();
            }
            (None, '\'' | '"') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            _ => {}
        }
    }
    quote.is_some()
}

/// A line that goes on from the one before it (`&& next`, `| next`).
fn joins_before(line: &str) -> bool {
    line.starts_with("&&") || line.starts_with('|') || line.starts_with(';')
}

fn ends_with_operator(line: &str) -> bool {
    line.ends_with("&&") || line.ends_with('|') || line.ends_with(';') || line.ends_with('&')
}

/// Whether a line of a block with no language reads as a command.
fn looks_like_command(line: &str) -> bool {
    let mut words = line.split_whitespace();
    let Some(first) = words.next() else {
        return false;
    };
    let rest: Vec<&str> = words.collect();
    let script = first.starts_with("./") || first.starts_with("~/") || first.starts_with("tools/");
    if !script && !COMMAND_WORDS.contains(&first) {
        return false;
    }
    if SHELLS.contains(&first) && !rest.first().is_some_and(|a| is_path(a)) {
        return false;
    }
    // A sentence ends in a word and a full stop; a command may end in `.` or `..` alone.
    if let Some(last) = rest.last().copied().or(Some(first)) {
        let word = last.trim_end_matches(['.', '?', '!', ':']);
        if word.len() < last.len() && !word.is_empty() && word.chars().all(char::is_alphabetic) {
            return false;
        }
    }
    if AMBIGUOUS.contains(&first) {
        let plain = |w: &&str| w.chars().all(|c| c.is_ascii_lowercase());
        if rest.is_empty() || (rest.len() >= 3 && rest.iter().all(plain)) {
            return false;
        }
    }
    true
}

/// An argument that is a path to a file (`/opt/x.sh`, `./x.sh`, `~/x.sh`, `tools/x.sh`).
fn is_path(arg: &str) -> bool {
    let arg = arg.trim_matches(['\'', '"']);
    ["/", "./", "../", "~/", "tools/"]
        .iter()
        .any(|p| arg.starts_with(p))
}

/// Where Run puts a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunHow {
    /// Sent to the session as a `!` message, whose output shows in the conversation.
    Send,
    /// Run in Chat's terminal, because it asks for input (a password, a secret, a tty) that a `!`
    /// message could not give it and that must not reach the transcript.
    Terminal,
}

/// How `command` runs: in the terminal when it is interactive, else as a message.
pub fn how(command: &str) -> RunHow {
    if is_interactive(command) {
        RunHow::Terminal
    } else {
        RunHow::Send
    }
}

/// First words of commands that need a terminal whatever their arguments.
const NEEDS_TTY: &[&str] = &[
    "sudo", "su", "passwd", "vim", "vi", "nvim", "nano", "emacs", "less", "more", "man", "top",
    "htop", "tmux", "screen", "sftp",
];
/// Programs that start a session of their own when given no arguments.
const REPLS: &[&str] = &[
    "claude", "codex", "python3", "python", "node", "bash", "sh", "zsh",
];
/// `ssh` options that take a value, so the value is not the host.
const SSH_VALUED: &[&str] = &[
    "-b", "-c", "-D", "-E", "-e", "-F", "-I", "-i", "-J", "-L", "-l", "-m", "-O", "-o", "-p", "-Q",
    "-R", "-S", "-W", "-w",
];

/// Whether the command asks for input from a terminal: a tty (`ssh -t`, `docker run -it`),
/// a prompt (`read -s`, `sudo`, a login), an editor or pager, or a program that starts a session.
fn is_interactive(command: &str) -> bool {
    if reads_a_prompt(command) {
        return true;
    }
    command
        .split(['|', ';', '&'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .any(segment_is_interactive)
}

/// `read -p`, `read -s`, `read -rsp`: wherever it appears, even inside the quotes of an `ssh`.
fn reads_a_prompt(command: &str) -> bool {
    let words: Vec<&str> = command.split_whitespace().collect();
    words.windows(2).any(|w| {
        w[0].trim_start_matches(['\'', '"']) == "read"
            && w[1].starts_with('-')
            && w[1].chars().any(|c| c == 'p' || c == 's')
    })
}

fn segment_is_interactive(segment: &str) -> bool {
    let words: Vec<&str> = segment
        .split_whitespace()
        // `VAR=value command` runs the command.
        .skip_while(|w| !w.starts_with('-') && w.contains('=') && !w.starts_with('/'))
        .collect();
    let Some((&first, args)) = words.split_first() else {
        return false;
    };
    let first = first.trim_start_matches(['\'', '"', '(']);
    if NEEDS_TTY.contains(&first) {
        return true;
    }
    if REPLS.contains(&first) && args.is_empty() {
        return true;
    }
    match first {
        "ssh" => ssh_is_interactive(args),
        "docker" | "podman" | "kubectl" => {
            args.iter().any(|a| matches!(*a, "run" | "exec"))
                && args
                    .iter()
                    .any(|a| matches!(*a, "-i" | "-t" | "-it" | "-ti" | "--tty" | "--stdin"))
        }
        "git" => {
            args.iter()
                .any(|a| matches!(*a, "-i" | "--interactive" | "-p" | "--patch"))
                && args
                    .iter()
                    .any(|a| matches!(*a, "rebase" | "add" | "checkout" | "stash"))
        }
        "gh" | "npm" | "docker-login" | "yarn" | "pnpm" | "vercel" | "op" => args
            .iter()
            .take(3)
            .any(|a| matches!(*a, "login" | "signin")),
        _ => false,
    }
}

/// `ssh` asks for a terminal with `-t`, and gives one when it has no remote command.
fn ssh_is_interactive(args: &[&str]) -> bool {
    let mut positional = 0;
    let mut skip = false;
    for arg in args {
        if skip {
            skip = false;
            continue;
        }
        if let Some(flags) = arg.strip_prefix('-').filter(|f| !f.is_empty()) {
            if flags.chars().all(|c| c == 't') || (!flags.starts_with('-') && flags.contains('t')) {
                return true;
            }
            skip = SSH_VALUED.contains(arg);
            continue;
        }
        positional += 1;
    }
    // Only the host: a login shell.
    positional <= 1
}
