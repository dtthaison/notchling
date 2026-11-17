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

fn redirects_or_backgrounds(command: &str) -> bool {
    let mut chars = command.chars().peekable();
    let mut previous = ' ';
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                }
            }
            '"' => {
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => {
                            chars.next();
                        }
                        _ => {}
                    }
                }
            }
            '\\' => {
                chars.next();
            }
            '<' | '>' => return true,
            '&' if previous != '&' && chars.peek() != Some(&'&') => return true,
            _ => {}
        }
        previous = c;
    }
    false
}

pub fn allows(request: &Value, binary: &Path) -> bool {
    let Some(command) = command_line(request) else {
        return false;
    };
    if redirects_or_backgrounds(command) {
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
    (Path::new(program) == binary || program == "notchling") && ask == "ask" && wait == "--wait"
}

#[cfg(test)]
#[path = "agy_ask_tests.rs"]
mod tests;
