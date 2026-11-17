use crate::ask::simple_commands;
use serde_json::Value;
use std::path::Path;

fn command_line(request: &Value) -> Option<&str> {
    let call = request.get("params")?.get("toolCall")?;
    call.get("rawInput")
        .and_then(|v| v.get("CommandLine"))
        .or_else(|| call.get("args").and_then(|v| v.get("CommandLine")))
        .and_then(Value::as_str)
}

fn plain_words(command: &str) -> bool {
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match c {
            '\n' => return false,
            '\'' => loop {
                match chars.next() {
                    None | Some('\n') => return false,
                    Some('\'') => break,
                    Some(_) => {}
                }
            },
            '"' => loop {
                match chars.next() {
                    None | Some('\n' | '$' | '`' | '!') => return false,
                    Some('"') => break,
                    Some('\\') => match chars.next() {
                        None | Some('\n' | '$' | '`' | '!') => return false,
                        Some(_) => {}
                    },
                    Some(_) => {}
                }
            },
            '\\' => match chars.next() {
                None | Some('\n') => return false,
                Some(_) => {}
            },
            c if c.is_ascii_alphanumeric() || " -_./,:@+%".contains(c) => {}
            _ => return false,
        }
    }
    true
}

pub fn allows(request: &Value, binary: &Path) -> bool {
    let Some(binary) = binary.to_str().filter(|b| b.starts_with('/')) else {
        return false;
    };
    let Some(command) = command_line(request) else {
        return false;
    };
    if !plain_words(command) {
        return false;
    }
    let Some(commands) = simple_commands(command) else {
        return false;
    };
    let [words] = commands.as_slice() else {
        return false;
    };
    let [program, ask, wait, ..] = words.as_slice() else {
        return false;
    };
    program == binary && ask == "ask" && wait == "--wait"
}

#[cfg(test)]
#[path = "agy_ask_tests.rs"]
mod tests;
