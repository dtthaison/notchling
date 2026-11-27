const PROMPT: &str = "\x1b[38;5;141mnotchling\x1b[0m \x1b[90mgit:(\x1b[31mmain\x1b[90m)\x1b[0m \x1b[32m\u{276f}\x1b[0m ";

const FIRST: &str =
    "ls\r\nCargo.toml  \x1b[1;34mcrates\x1b[0m  \x1b[1;34mdocs\x1b[0m  README.md\r\n";

#[derive(Debug, PartialEq, Eq)]
pub struct Sample {
    pub bytes: Vec<u8>,
    pub expanded: bool,
    pub focus: bool,
}

pub fn sample(how: &str) -> Sample {
    let words: Vec<&str> = how.split(',').map(str::trim).collect();
    let has = |w: &str| words.contains(&w);
    let tail = if has("over") {
        "clear\x1b[D"
    } else if has("typed") {
        "clear"
    } else if has("wide") {
        "echo \u{65e5}\u{672c}\x1b[2D"
    } else if has("wide-end") {
        "echo \u{65e5}\u{672c}"
    } else {
        ""
    };
    Sample {
        bytes: format!("{PROMPT}{FIRST}{PROMPT}{tail}").into_bytes(),
        expanded: has("expanded"),
        focus: has("focus"),
    }
}

#[cfg(test)]
#[path = "terminal_sample_tests.rs"]
mod tests;
