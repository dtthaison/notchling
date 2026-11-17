use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use crate::versions::Tool;

pub const EVERY: Duration = Duration::from_secs(600);
pub const TIMEOUT: Duration = Duration::from_secs(5);
pub const AGY_LOGIN_FILE: &str = ".gemini/jetski-standalone-oauth-token";
pub const CODEX_COMMAND: &str = "codex login --device-auth";
pub const AGY_COMMAND: &str = "agy";
pub const KEYCHAIN_PROGRAM: &str = "/usr/bin/security";
pub const KEYCHAIN_ARGS: [&str; 5] = ["find-generic-password", "-s", "gemini", "-a", "antigravity"];
const KEYCHAIN_REDIRECTS: [&str; 4] = [">/dev/null", "2>/dev/null", "2>", "1"];
pub const FORBIDDEN: [&str; 4] = ["refresh", "logout", "setup-token", "--with-api-key"];

pub fn codex_signed_in(exit: i32, stdout: &str) -> bool {
    let text = stdout.to_ascii_lowercase();
    exit == 0 && text.contains("logged in") && !text.contains("not logged in")
}

pub fn codex_state(exit: i32, text: &str) -> Option<bool> {
    if codex_signed_in(exit, text) {
        return Some(true);
    }
    (exit != 0 && text.to_ascii_lowercase().contains("not logged in")).then_some(false)
}

pub const CLAUDE_PROFILE_FILE: &str = ".claude.json";

#[derive(serde::Deserialize)]
struct ClaudeConfig {
    #[serde(rename = "oauthAccount")]
    account: Option<serde_json::Map<String, serde_json::Value>>,
}

pub fn claude_profile_signed_in(text: &str) -> bool {
    serde_json::from_str::<ClaudeConfig>(text)
        .ok()
        .and_then(|config| config.account)
        .is_some_and(|account| !account.is_empty())
}

pub fn agy_signed_in(files: &dyn Fn(&Path) -> bool, home: &Path) -> bool {
    files(&home.join(AGY_LOGIN_FILE))
}

pub fn agy_state(os: &str, files: &dyn Fn(&Path) -> bool, home: &Path) -> Option<bool> {
    if agy_signed_in(files, home) {
        Some(true)
    } else if os == "macos" {
        None
    } else {
        Some(false)
    }
}

pub fn keychain_argv() -> Vec<String> {
    std::iter::once(KEYCHAIN_PROGRAM)
        .chain(KEYCHAIN_ARGS)
        .map(String::from)
        .collect()
}

pub fn keychain_command() -> String {
    keychain_argv().join(" ")
}

pub fn keychain_state(exit: i32) -> Option<bool> {
    (exit == 0).then_some(true)
}

fn security_allowed(words: &[String], i: usize) -> bool {
    let rest = &words[i + 1..];
    words[i] == KEYCHAIN_PROGRAM
        && rest.len() >= KEYCHAIN_ARGS.len()
        && rest[..KEYCHAIN_ARGS.len()] == KEYCHAIN_ARGS
        && rest[KEYCHAIN_ARGS.len()..]
            .iter()
            .all(|w| KEYCHAIN_REDIRECTS.contains(&w.as_str()))
}

fn is_security(word: &str) -> bool {
    word.rsplit('/').next() == Some("security")
}

fn commands(text: &str) -> impl Iterator<Item = Vec<String>> + '_ {
    let cut = |c: char| matches!(c, ';' | '&' | '|' | '(' | ')' | '`' | '\n' | '\r');
    text.split(cut).map(|part| {
        part.split_whitespace()
            .map(|w| w.trim_matches(|c| matches!(c, '\'' | '"')).to_string())
            .filter(|w| !w.is_empty())
            .collect()
    })
}

pub fn read_only(text: &str) -> bool {
    commands(text).all(|words: Vec<String>| {
        words.iter().enumerate().all(|(i, word)| {
            !FORBIDDEN.contains(&word.as_str())
                && (word != "login" || words.get(i + 1).map(String::as_str) == Some("status"))
                && (!is_security(word) || security_allowed(&words, i))
        })
    })
}

pub fn argv_read_only(argv: &[String]) -> bool {
    read_only(&argv.join(" "))
}

pub trait StatusRunner: Send + Sync {
    fn run(&self, argv: &[String], timeout: Duration) -> Option<(i32, String)>;
}

pub struct Sources<'a> {
    pub codex: Option<&'a str>,
    pub agy_installed: bool,
    pub os: &'a str,
    pub home: &'a Path,
}

pub fn probe(
    src: &Sources,
    run: &dyn StatusRunner,
    files: &dyn Fn(&Path) -> bool,
) -> BTreeMap<Tool, bool> {
    let mut out = BTreeMap::new();
    if let Some(path) = src.codex {
        let argv = vec![path.to_string(), "login".to_string(), "status".to_string()];
        if argv_read_only(&argv) {
            let state = run
                .run(&argv, TIMEOUT)
                .and_then(|(exit, text)| codex_state(exit, &text));
            if let Some(state) = state {
                out.insert(Tool::Codex, state);
            }
        }
    }
    if src.agy_installed {
        let state = agy_state(src.os, files, src.home).or_else(|| {
            let argv = keychain_argv();
            if src.os != "macos" || !argv_read_only(&argv) {
                return None;
            }
            run.run(&argv, TIMEOUT)
                .and_then(|(exit, _)| keychain_state(exit))
        });
        if let Some(state) = state {
            out.insert(Tool::Agy, state);
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flip {
    pub tool: Tool,
    pub signed_in: bool,
    pub seen_before: bool,
}

#[derive(Default)]
pub struct Tracker {
    last: BTreeMap<Tool, bool>,
}

impl Tracker {
    pub fn update(&mut self, now: &BTreeMap<Tool, bool>) -> Vec<Flip> {
        let mut flips = Vec::new();
        for (tool, signed_in) in now {
            let before = self.last.insert(*tool, *signed_in);
            let changed = match before {
                Some(was) => was != *signed_in,
                None => !*signed_in,
            };
            if changed {
                flips.push(Flip {
                    tool: *tool,
                    signed_in: *signed_in,
                    seen_before: before.is_some(),
                });
            }
        }
        flips
    }
}

pub fn title_of(tool: Tool) -> &'static str {
    match tool {
        Tool::Codex => "Codex",
        Tool::Agy => "Antigravity",
        Tool::Claude => "Claude Code",
        Tool::Notchling => "Notchling",
    }
}

pub fn how_to(tool: Tool, node: &str) -> String {
    match tool {
        Tool::Agy => format!(
            "In your own terminal on {node} run {AGY_COMMAND} and sign in with the same Google login as the Antigravity app. Notchling never signs a tool in or renews a login."
        ),
        _ => format!(
            "In your own terminal on {node} run {CODEX_COMMAND}. Notchling never signs a tool in or renews a login."
        ),
    }
}

pub fn needed_text(tool: Tool, node: &str) -> (String, String) {
    (
        format!("{} isn't signed in on {node}", title_of(tool)),
        match tool {
            Tool::Agy => format!(
                "Run {AGY_COMMAND} there, in that machine's own terminal, and sign in with the same Google login as the Antigravity app."
            ),
            _ => format!("Run {CODEX_COMMAND} there, in that machine's own terminal."),
        },
    )
}

#[cfg(test)]
#[path = "logins_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "logins_keychain_tests.rs"]
mod keychain_tests;

#[cfg(test)]
#[path = "logins_own_tests.rs"]
mod own_tests;
