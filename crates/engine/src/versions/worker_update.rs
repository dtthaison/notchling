use super::update::update_argv;
use super::{can_pin, Method, Tool, ToolState, Version};
use crate::lanes::quote;

pub const TOOLS: [Tool; 3] = [Tool::Claude, Tool::Codex, Tool::Agy];

pub fn strict(v: &Version) -> Option<&str> {
    let text = v.as_str();
    let mut parts = text.split('.');
    for _ in 0..3 {
        let part = parts.next()?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    parts.next().is_none().then_some(text)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Move {
    Run(String),
    HowTo(String),
}

fn installer_words(tool: Tool) -> &'static str {
    match tool {
        Tool::Claude => "the native installer or npm",
        _ => "its standalone installer or npm",
    }
}

fn how_to(machine: &str, tool: Tool, method: Method, to: &Version) -> String {
    let name = tool.title();
    match (tool, method) {
        (Tool::Agy, _) => format!(
            "Antigravity's installer takes no exact version, so Notchling can't set it to {to} on {machine}. Run agy update on {machine}; it installs the newest, which may not be {to}."
        ),
        (_, Method::Brew) => format!(
            "{name} on {machine} came from Homebrew, which can't install an exact version. Install {to} with {} on {machine}, then check again.",
            installer_words(tool)
        ),
        (_, Method::Apt) => format!(
            "{name} on {machine} came from apt, which can't install an exact version. Install {to} with {} on {machine}, then check again.",
            installer_words(tool)
        ),
        (_, Method::Unknown) => format!(
            "Notchling can't tell how {name} was installed on {machine}, so it can't set it to {to}. Install {to} yourself, then check again."
        ),
        (_, other) => format!(
            "{name} on {machine} came from the {}, which Notchling can't set to an exact version. Install {to} yourself, then check again.",
            other.title()
        ),
    }
}

pub fn plan(machine: &str, tool: Tool, state: &ToolState, to: &Version) -> Result<Move, String> {
    if !TOOLS.contains(&tool) {
        return Err(format!(
            "Notchling doesn't update {} this way",
            tool.title()
        ));
    }
    let (version, method) = match state {
        ToolState::NotInstalled => {
            return Err(format!("{} isn't installed on {machine}", tool.title()))
        }
        ToolState::Installed {
            version, method, ..
        } => (version.as_ref(), *method),
    };
    if strict(to).is_none() {
        return Err(format!("{to} isn't a version Notchling installs"));
    }
    if version == Some(to) {
        return Err(format!("{} on {machine} is already {to}", tool.title()));
    }
    if !can_pin(tool, method) {
        return Ok(Move::HowTo(how_to(machine, tool, method, to)));
    }
    let argv = update_argv(tool, method, Some(to))?;
    let words: Vec<String> = argv.iter().map(|w| quote(w)).collect();
    Ok(Move::Run(words.join(" ")))
}

pub fn unread_how_to(machine: &str, tool: Tool, to: &Version) -> Option<String> {
    (tool == Tool::Agy).then(|| how_to(machine, tool, Method::Standalone, to))
}

pub fn busy_script(tool: Tool) -> String {
    let (name, marks): (&str, &[&str]) = match tool {
        Tool::Claude => ("claude", &["/claude-code/", "/claude/versions/"]),
        Tool::Codex => ("codex", &["/@openai/codex/", "/.codex/packages/"]),
        other => (other.command(), &[]),
    };
    let extra: String = marks
        .iter()
        .map(|m| format!(" || index($1 $2,\"{m}\")"))
        .collect();
    format!(
        "ps -axo args= 2>/dev/null | awk 'function b(s,  a,n){{n=split(s,a,\"/\");return a[n]}} b($1)==\"{name}\" || b($2)==\"{name}\" || b($2)==\"{name}.js\"{extra} {{c++}} END{{print \"busy=\" c+0}}'"
    )
}

pub fn parse_busy(text: &str) -> Option<usize> {
    text.lines()
        .take(8)
        .find_map(|line| line.trim().strip_prefix("busy="))
        .and_then(|n| n.parse().ok())
}

pub fn busy_text(machine: &str, tool: Tool, count: usize) -> String {
    format!(
        "{} is running on {machine} ({count} process{}). Try again when it is done.",
        tool.title(),
        if count == 1 { "" } else { "es" }
    )
}

pub fn failure_text(tool: Tool, machine: &str, out: &str, err: &str) -> String {
    let text = if err.trim().is_empty() { out } else { err };
    let tail: Vec<&str> = text
        .lines()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(4)
        .collect();
    let shown = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
    let mut line = format!("{} didn't install on {machine}", tool.title());
    if !shown.is_empty() {
        line.push_str(": ");
        line.push_str(&shown);
    }
    if text.contains("EACCES") {
        line.push_str("\nnpm can't write its global folder as this user. Give npm a prefix inside the home folder, then try again.");
    }
    line
}
