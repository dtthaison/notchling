//! The hook command line: writing ours, and telling whose a command in settings.json is.
//!
//! Ours is `'<path>/notchling' hook`. The Swift app's is `'<app>/Contents/MacOS/Notchling'
//! --hook` (and, from when the app was called Awake, `.../Awake' --hook`). Anything else is the
//! user's, and is never touched.

use notchling_platform::paths::Os;
use std::path::Path;

/// Whose a hook command is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// `notchling hook`, this program.
    Notchling,
    /// The Swift app's `--hook`.
    Swift,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Notchling => "notchling",
            Origin::Swift => "swift",
        }
    }
}

/// The command for `binary`: quoted for the shell Claude Code runs hooks with.
pub fn hook_command(binary: &Path, os: Os) -> String {
    let path = binary.to_string_lossy();
    match os {
        Os::Windows => format!("\"{}\" hook", path.replace('"', "")),
        _ => format!("'{}' hook", path.replace('\'', "'\\''")),
    }
}

/// Words as a POSIX shell splits them (quotes and backslashes only). `None` for an unbalanced
/// quote.
fn words(command: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut open = false;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                open = true;
                loop {
                    match chars.next()? {
                        '\'' => break,
                        c => word.push(c),
                    }
                }
            }
            '"' => {
                open = true;
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' if matches!(chars.peek(), Some('"' | '\\' | '$' | '`')) => {
                            word.push(chars.next()?)
                        }
                        c => word.push(c),
                    }
                }
            }
            '\\' => {
                open = true;
                word.push(chars.next()?);
            }
            c if c.is_whitespace() => {
                if open || !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                    open = false;
                }
            }
            c => {
                open = true;
                word.push(c);
            }
        }
    }
    if open || !word.is_empty() {
        out.push(word);
    }
    Some(out)
}

fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_lowercase()
}

/// Whose a command is, or `None` for the user's own.
pub fn classify(command: &str) -> Option<Origin> {
    // What the Swift app itself matches on, so anything it would call its own is.
    if command.contains("/Contents/MacOS/Notchling' --hook")
        || command.contains("/Contents/MacOS/Awake' --hook")
    {
        return Some(Origin::Swift);
    }
    let words = words(command)?;
    for (i, pair) in words.windows(2).enumerate() {
        let program = file_name(&pair[0]);
        let rest = &words[i + 2..];
        if matches!(program.as_str(), "notchling" | "awake") && pair[1] == "--hook" {
            return Some(Origin::Swift);
        }
        let ours = matches!(program.as_str(), "notchling" | "notchling.exe");
        if ours && pair[1] == "hook" && rest.first().is_none_or(|w| w.starts_with("--")) {
            return Some(Origin::Notchling);
        }
    }
    None
}

/// The program and its `hook` word without any environment settings or options before or after
/// them (a hook line can carry a value that must not be repeated in a report).
pub fn summary(command: &str) -> String {
    let Some(words) = words(command) else {
        return "(a hook command)".to_string();
    };
    for pair in words.windows(2) {
        let program = file_name(&pair[0]);
        let swift = matches!(program.as_str(), "notchling" | "awake") && pair[1] == "--hook";
        let ours = matches!(program.as_str(), "notchling" | "notchling.exe") && pair[1] == "hook";
        if swift || ours {
            return format!("'{}' {}", pair[0], pair[1]);
        }
    }
    "(a hook command)".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn the_command_is_quoted_and_recognised_as_ours() {
        for (path, os) in [
            ("/usr/local/bin/notchling", Os::Mac),
            ("/Users/me/My Apps/notchling", Os::Linux),
            ("/Users/o'neil/bin/notchling", Os::Mac),
            (r"C:\Program Files\Notchling\notchling.exe", Os::Windows),
        ] {
            let command = hook_command(&PathBuf::from(path), os);
            assert_eq!(classify(&command), Some(Origin::Notchling), "{command}");
        }
        assert_eq!(
            hook_command(&PathBuf::from("/a b/notchling"), Os::Mac),
            "'/a b/notchling' hook"
        );
        assert_eq!(
            hook_command(&PathBuf::from("/o'k/notchling"), Os::Mac),
            "'/o'\\''k/notchling' hook"
        );
    }

    #[test]
    fn the_swift_apps_command_is_recognised_in_either_spelling() {
        for command in [
            "'/Applications/Notchling.app/Contents/MacOS/Notchling' --hook",
            "'/Applications/Notchling.app/Contents/MacOS/Awake' --hook",
            "/Applications/Notchling.app/Contents/MacOS/Notchling --hook",
            "\"/Users/me/Notchling Dev.app/Contents/MacOS/Notchling\" --hook",
        ] {
            assert_eq!(classify(command), Some(Origin::Swift), "{command}");
        }
    }

    #[test]
    fn a_summary_leaves_out_what_surrounds_the_program() {
        assert_eq!(
            summary("TOKEN=abc123 '/x/notchling' hook --socket /tmp/s"),
            "'/x/notchling' hook"
        );
        assert_eq!(
            summary("'/Applications/Notchling.app/Contents/MacOS/Notchling' --hook"),
            "'/Applications/Notchling.app/Contents/MacOS/Notchling' --hook"
        );
        assert_eq!(summary("'unbalanced"), "(a hook command)");
    }

    #[test]
    fn options_after_hook_are_ours_and_other_words_are_not() {
        assert_eq!(
            classify("'/x/notchling' hook --socket /tmp/s"),
            Some(Origin::Notchling)
        );
        assert_eq!(
            classify("NOTCHLING_DATA_ROOT=/tmp/r '/x/notchling' hook"),
            Some(Origin::Notchling)
        );
        for user in [
            "",
            "echo hook",
            "~/bin/lint.sh",
            "notchling hooks",
            "/x/notchling hook extra",
            "/x/notchling status",
            "my-notchling hook",
            "'/x/notchling hook",
            "node ~/hooks/notchling.js --hook",
        ] {
            assert_eq!(classify(user), None, "{user}");
        }
    }
}
