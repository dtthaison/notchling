#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    On,
    Off,
}

pub fn parse(text: &str) -> Option<Command> {
    let rest = text.trim().strip_prefix('/')?;
    let (word, args) = match rest.split_once(char::is_whitespace) {
        Some((word, args)) => (word, args.trim()),
        None => (rest, ""),
    };
    if !word.eq_ignore_ascii_case("chrome") {
        return None;
    }
    match args.to_lowercase().as_str() {
        "off" | "disable" | "--no-chrome" => Some(Command::Off),
        _ => Some(Command::On),
    }
}

pub fn is_chrome(text: &str) -> bool {
    parse(text).is_some()
}
