#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    Enabled,
    Connecting,
    NotConnected,
    NotEnabled,
    TurnedOffInMcp,
    Other(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extension {
    Installed,
    NotDetected,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub link: Link,
    pub extension: Extension,
    pub browser: Option<String>,
    pub by_default: Option<bool>,
    pub options: Vec<String>,
    pub selected: usize,
    pub notice: Option<String>,
}

pub const TITLE: &str = "Claude in Chrome";
pub const CLOSE_HINT: &str = "Esc to close";
pub const RECONNECT: &str = "Reconnect extension";
pub const COMMAND_HINT: &str = "Open Claude in Chrome settings";

pub fn is_open(lines: &[String]) -> bool {
    lines.iter().any(|l| l.trim() == TITLE) && lines.iter().any(|l| l.contains(CLOSE_HINT))
}

fn value<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let rest = line.trim_start().strip_prefix(label)?;
    let rest = rest.strip_prefix(':')?;
    Some(
        rest.trim_start_matches(|c: char| !c.is_alphanumeric())
            .trim_end(),
    )
}

fn link_of(text: &str) -> Link {
    match text {
        "enabled" => Link::Enabled,
        t if t.starts_with("connecting") => Link::Connecting,
        "not connected" => Link::NotConnected,
        "not enabled in this session" => Link::NotEnabled,
        "turned off in /mcp" => Link::TurnedOffInMcp,
        other => Link::Other(other.to_string()),
    }
}

fn extension_of(text: &str) -> Extension {
    match text {
        "installed" => Extension::Installed,
        "not detected" => Extension::NotDetected,
        _ => Extension::Unknown,
    }
}

pub fn read(lines: &[String]) -> Option<Menu> {
    if !is_open(lines) {
        return None;
    }
    let title = lines.iter().position(|l| l.trim() == TITLE)?;
    let rows = &lines[title + 1..];
    let find = |label: &str| rows.iter().find_map(|l| value(l, label));
    let link = link_of(find("Status")?);
    let extension = find("Extension").map_or(Extension::Unknown, extension_of);
    let browser = find("Browser").map(str::to_string);
    let by_default = find("By default").map(|v| v == "on");
    let after = rows.iter().position(|l| value(l, "By default").is_some())?;
    let mut blocks = rows[after + 1..]
        .split(|l| l.trim().is_empty())
        .filter(|b| !b.is_empty());
    let options_block = blocks.next().unwrap_or(&[]);
    let mut options = Vec::new();
    let mut selected = 0;
    for line in options_block {
        let text = line.trim();
        let (mark, label) = match text.strip_prefix('❯') {
            Some(rest) => (true, rest.trim()),
            None => (false, text),
        };
        if mark {
            selected = options.len();
        }
        options.push(label.to_string());
    }
    let notice = blocks
        .next()
        .filter(|b| {
            b.first().is_some_and(|l| {
                !l.contains("For one session") && !l.contains("Learn more") && !l.contains("Esc to")
            })
        })
        .map(|b| {
            b.iter()
                .map(|l| l.trim().trim_start_matches(|c: char| !c.is_alphanumeric()))
                .collect::<Vec<_>>()
                .join(" ")
        });
    Some(Menu {
        link,
        extension,
        browser,
        by_default,
        options,
        selected,
        notice,
    })
}

impl Menu {
    pub fn index_of(&self, label: &str) -> Option<usize> {
        self.options.iter().position(|o| o == label)
    }
}
