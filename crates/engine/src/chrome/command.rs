#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    On,
    Off,
}

pub fn parse(text: &str) -> Option<Command> {
    let rest = text.trim().strip_prefix('/')?;
    let mut words = rest.split_whitespace();
    if !words.next()?.eq_ignore_ascii_case("chrome") {
        return None;
    }
    let args: Vec<String> = words.map(str::to_lowercase).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => Some(Command::On),
        ["off" | "disable" | "--no-chrome"] => Some(Command::Off),
        _ => None,
    }
}

pub fn is_chrome(text: &str) -> bool {
    parse(text).is_some()
}
