use notchling_platform::paths::Os;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Notchling,
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

pub fn hook_command(binary: &Path, os: Os) -> String {
    let path = binary.to_string_lossy();
    match os {
        Os::Windows => format!("\"{}\" hook", path.replace('"', "")),
        _ => format!("'{}' hook", path.replace('\'', "'\\''")),
    }
}

fn words(command: &str) -> Option<(Vec<String>, bool)> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut open = false;
    let mut control = false;
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
                        c => {
                            control |= is_substitution(c, chars.peek());
                            word.push(c);
                        }
                    }
                }
            }
            '\\' => {
                open = true;
                word.push(chars.next()?);
            }
            c if c.is_whitespace() => {
                control |= c == '\n' || c == '\r';
                if open || !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                    open = false;
                }
            }
            c => {
                open = true;
                control |= matches!(c, ';' | '&' | '|' | '<' | '>' | '(' | ')')
                    || is_substitution(c, chars.peek());
                word.push(c);
            }
        }
    }
    if open || !word.is_empty() {
        out.push(word);
    }
    Some((out, control))
}

fn is_substitution(c: char, next: Option<&char>) -> bool {
    c == '`' || (c == '$' && next == Some(&'('))
}

fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_lowercase()
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _)| {
        name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn call(command: &str) -> Option<(String, String, Vec<String>)> {
    let (mut words, control) = words(command.trim())?;
    if control {
        return None;
    }
    let skip = words.iter().take_while(|w| is_assignment(w)).count();
    let mut rest = words.split_off(skip).into_iter();
    let program = rest.next()?;
    let first = rest.next()?;
    Some((program, first, rest.collect()))
}

pub fn classify(command: &str) -> Option<Origin> {
    if command.contains("/Contents/MacOS/Notchling' --hook")
        || command.contains("/Contents/MacOS/Awake' --hook")
    {
        return Some(Origin::Swift);
    }
    let (program, first, rest) = call(command)?;
    match file_name(&program).as_str() {
        "notchling" | "awake" if first == "--hook" => Some(Origin::Swift),
        "notchling" | "notchling.exe"
            if first == "hook" && rest.first().is_none_or(|w| w.starts_with("--")) =>
        {
            Some(Origin::Notchling)
        }
        _ => None,
    }
}

pub fn summary(command: &str) -> String {
    let Some((program, first, _)) = call(command) else {
        return "(a hook command)".to_string();
    };
    let name = file_name(&program);
    let swift = matches!(name.as_str(), "notchling" | "awake") && first == "--hook";
    let ours = matches!(name.as_str(), "notchling" | "notchling.exe") && first == "hook";
    if swift || ours {
        format!("'{program}' {first}")
    } else {
        "(a hook command)".to_string()
    }
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
    fn the_exact_forms_are_ours_whatever_the_quoting() {
        for command in [
            "'/x/notchling' hook",
            "\"/x/notchling\" hook",
            "/x/notchling hook",
            "  '/x/notchling' hook  ",
            "'/x/note ling/notchling' hook",
            "\"/x/o'k/notchling\" hook",
            "'/x/notchling' hook --socket /tmp/s",
            "NOTCHLING_DATA_ROOT=/tmp/r '/x/notchling' hook",
            "A=1 B_2='x y' /x/notchling hook --socket '/tmp/a b'",
            "\"C:\\Program Files\\Notchling\\notchling.exe\" hook",
            "'/x/notchling' hook --socket /tmp/s'a;b'",
        ] {
            assert_eq!(classify(command), Some(Origin::Notchling), "{command}");
        }
    }

    #[test]
    fn a_wrapper_or_a_second_command_around_ours_is_not_ours() {
        for user in [
            "tee /tmp/x | '/x/notchling' hook",
            "'/x/notchling' hook | tee /tmp/x",
            "'/x/notchling' hook --socket /tmp/s | tee /tmp/x",
            "sh -c '/x/notchling hook'",
            "sh -c \"'/x/notchling' hook\"",
            "bash -lc '/x/notchling hook --socket /tmp/s'",
            "/x/notchling hook; rm -rf x",
            "/x/notchling hook --socket /tmp/s; rm -rf x",
            "'/x/notchling' hook --socket /tmp/s&& rm -rf x",
            "'/x/notchling' hook --socket /tmp/s & rm -rf x",
            "/x/notchling hook && echo done",
            "/x/notchling hook || true",
            "echo start; '/x/notchling' hook",
            "cd /tmp && '/x/notchling' hook",
            "(cd /tmp; '/x/notchling' hook)",
            "env FOO=1 '/x/notchling' hook",
            "exec '/x/notchling' hook",
            "nohup '/x/notchling' hook",
            "time '/x/notchling' hook",
            "echo $('/x/notchling' hook)",
            "echo `'/x/notchling' hook`",
            "echo \"$('/x/notchling' hook)\"",
            "'/x/notchling' hook --socket $(rm -rf x)",
            "'/x/notchling' hook --socket `rm x`",
            "'/x/notchling' hook --socket /tmp/s > /tmp/out",
            "'/x/notchling' hook --socket /tmp/s < /tmp/in",
            "'/x/notchling' hook\nrm -rf x",
            "echo hi\n'/x/notchling' hook",
        ] {
            assert_eq!(classify(user), None, "{user}");
        }
    }

    #[test]
    fn a_swift_command_is_not_taken_from_inside_a_wrapper_either() {
        assert_eq!(classify("tee /tmp/x | /x/notchling --hook"), None);
        assert_eq!(classify("/x/notchling --hook; rm x"), None);
        assert_eq!(classify("/x/notchling --hook"), Some(Origin::Swift));
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
